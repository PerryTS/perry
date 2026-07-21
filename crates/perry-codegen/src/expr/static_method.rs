//! StaticMethodCall.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::nanbox::double_literal;
use crate::native_value::MaterializationReason;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    downgrade_buffer_aliases_in_expr, emit_v8_export_call, emit_v8_member_method_call,
    import_origin_suffix_ns, lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx,
};

fn downgrade_unknown_call_args(ctx: &mut FnCtx<'_>, args: &[Expr]) {
    for arg in args {
        downgrade_buffer_aliases_in_expr(ctx, arg, MaterializationReason::UnknownCallEscape);
    }
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::StaticMethodCall {
            class_name,
            method_name,
            args,
        } => {
            downgrade_unknown_call_args(ctx, args);
            // Built-in static methods that the runtime provides directly.
            if class_name == "AbortSignal" && method_name == "timeout" {
                let ms = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(0.0)
                };
                let blk = ctx.block();
                let signal_handle = blk.call(I64, "js_abort_signal_timeout", &[(DOUBLE, &ms)]);
                return Ok(nanbox_pointer_inline(blk, &signal_handle));
            }
            // #2582: `AbortSignal.abort(reason?)` — returns a pre-aborted signal.
            if class_name == "AbortSignal" && method_name == "abort" {
                let reason = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let signal_handle = blk.call(I64, "js_abort_signal_abort", &[(DOUBLE, &reason)]);
                return Ok(nanbox_pointer_inline(blk, &signal_handle));
            }
            // #2582: `AbortSignal.any([signals])` — combined signal.
            if class_name == "AbortSignal" && method_name == "any" {
                let arr_box = if !args.is_empty() {
                    lower_expr(ctx, &args[0])?
                } else {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                };
                let blk = ctx.block();
                let arr_handle = unbox_to_i64(blk, &arr_box);
                let signal_handle = blk.call(I64, "js_abort_signal_any", &[(I64, &arr_handle)]);
                return Ok(nanbox_pointer_inline(blk, &signal_handle));
            }
            let key = (
                class_name.clone(),
                crate::codegen::static_method_registry_key(method_name),
            );
            if let Some(fn_name) = ctx.methods.get(&key).cloned() {
                // Inherited static (`D.f()` resolving to a parent's body): arm
                // the one-shot static-`this` override with the DISPATCH base
                // class-ref so the body's `js_static_this_resolve` prologue
                // sees `this === D` (spec OrdinaryCallBindThis), not the
                // lexical defining class. Own methods skip the arm — the
                // prologue's lexical fallback is already the right receiver.
                let owns_method = ctx
                    .classes
                    .get(class_name)
                    .map(|c| c.static_methods.iter().any(|m| m.name == *method_name))
                    .unwrap_or(true);
                if !owns_method {
                    if let Some(&cid) = ctx.class_ids.get(class_name) {
                        let cid_str = cid.to_string();
                        ctx.block()
                            .call_void("js_static_this_arm_classref", &[(I32, &cid_str)]);
                    }
                }
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                // Issue #894: static methods with synthetic `...arguments`
                // rest params (or any user-declared rest param) need their
                // trailing args bundled into an array. Without this,
                // `Cls.pipe(a, b)` on a body that reads `arguments`
                // emits a 2-scalar call against a 1-rest-array signature,
                // leaving `arguments` bound to whichever scalar landed
                // in the rest slot — `arguments.length` then reads garbage
                // or hits the codegen-fallback undefined.
                let has_rest = ctx.method_has_rest.get(&key).copied().unwrap_or(false);
                if has_rest {
                    let declared_count = ctx.method_param_counts.get(&key).copied().unwrap_or(0);
                    if declared_count > 0 {
                        let fixed = declared_count.saturating_sub(1);
                        if lowered.len() >= fixed {
                            let trailing: Vec<String> = lowered.split_off(fixed);
                            let arr_handle = ctx.block().call(
                                I64,
                                "js_array_alloc",
                                &[(I32, &trailing.len().to_string())],
                            );
                            // js_array_push_f64 may realloc and return a
                            // possibly-new handle; thread it.
                            let mut handle_cur = arr_handle;
                            for v in &trailing {
                                handle_cur = ctx.block().call(
                                    I64,
                                    "js_array_push_f64",
                                    &[(I64, &handle_cur), (DOUBLE, v)],
                                );
                            }
                            let arr_box = nanbox_pointer_inline(ctx.block(), &handle_cur);
                            lowered.push(arr_box);
                        }
                        // Pad fixed slots with undefined when caller under-supplied.
                        while lowered.len() < declared_count {
                            // Insert undefined at the rest-slot's predecessor.
                            let undef = double_literal(f64::from_bits(0x7FFC_0000_0000_0001));
                            let idx = lowered.len().saturating_sub(1);
                            lowered.insert(idx, undef);
                        }
                    }
                } else {
                    // Issue #235: a static method called with fewer args than
                    // declared (`C.f()` for `static f(a = 1)`, or
                    // `C.m([x] = [])`) must hand the callee `undefined` for the
                    // missing slots — otherwise the LLVM function reads an
                    // uninitialized parameter register (0.0), so its
                    // default-param prologue (`if (p === undefined) p = …`) and
                    // destructuring (`GetIterator(p)`) never fire.
                    let declared_count = ctx.method_param_counts.get(&key).copied().unwrap_or(0);
                    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                    while lowered.len() < declared_count {
                        lowered.push(undef.clone());
                    }
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                return Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices));
            }
            // #310: when the receiver is a namespace alias from an
            // `import { Foo } from "pkg"` where the source module did
            // `export * as Foo from "./Foo"`, the HIR's "uppercase Ident
            // looks like a class" rule lifts `Foo.method(...)` to
            // StaticMethodCall — but `Foo` isn't actually a class, so
            // the methods-table lookup above misses. The CLI driver's
            // namespace-import walk has already registered every export
            // of the namespace target file under its own name in
            // `import_function_prefixes`, so the function call resolves
            // to the same `perry_fn_<src>__<method>` symbol a
            // `import * as Foo from "pkg/Foo"` would have used.
            if ctx.namespace_imports.contains(class_name) {
                // Issue #678 followup (namespace branch): `import * as ns
                // from "<v8-module>"; ns.member(args)` with no companion
                // Named import — the V8 module has no static export list
                // so `import_function_prefixes` has no entry for
                // `method_name`. Probe the namespace-level V8 specifier
                // map first; on a hit, route the member call through the
                // bridge using the namespace's specifier. Without this,
                // ramda / date-fns / jose / effect wildcard-namespace
                // members fell to the `double_literal(0.0)` stub below.
                if let Some(specifier) = ctx.namespace_v8_specifiers.get(class_name).cloned() {
                    let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                    for a in args {
                        lowered.push(lower_expr(ctx, a)?);
                    }
                    return Ok(emit_v8_export_call(ctx, &specifier, method_name, &lowered));
                }
                // Issue #5922 (companion to #680): prefer the per-namespace
                // map so `Context.a` and `Option.a` resolve to their own
                // sources even when both namespaces export a member with
                // the same bare name. Falls back to the flat
                // `import_function_prefixes` for namespaces with no
                // overlapping conflicts (e.g. plain `import * as X`, which
                // this branch also serves).
                let source_prefix_opt = ctx
                    .namespace_member_prefixes
                    .get(&(class_name.to_string(), method_name.to_string()))
                    .cloned()
                    .or_else(|| ctx.import_function_prefixes.get(method_name).cloned());
                if let Some(source_prefix) = source_prefix_opt {
                    // Issue #678 followup: V8-fallback namespace member route —
                    // the origin module emits no native symbol, so dispatch
                    // through the runtime bridge.
                    if let Some(specifier) =
                        ctx.import_function_v8_specifiers.get(method_name).cloned()
                    {
                        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                        for a in args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        return Ok(emit_v8_export_call(ctx, &specifier, method_name, &lowered));
                    }
                    // Issue #678/#5924: namespace member resolved through a
                    // re-export rename uses the origin name as the symbol
                    // suffix. Namespace-scoped lookup first so a rename in a
                    // different namespace imported into this file can't
                    // clobber this namespace's unrenamed member of the same
                    // name.
                    let origin_suffix = import_origin_suffix_ns(
                        ctx.import_function_origin_names,
                        ctx.namespace_member_origin_names,
                        class_name,
                        method_name,
                    );
                    let fn_name = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                    // Issue #321: var-shaped exports (e.g. `export const succeed
                    // = (v) => new EffectInst(v)`) emit a ZERO-ARG getter
                    // `perry_fn_<src>__<name>()` returning the closure. The
                    // previous code emitted a 1-arg direct call against that
                    // 0-arg symbol — the source returned the function pointer
                    // unchanged and the consumer saw `typeof Effect.succeed(42)
                    // === "function"` (the closure itself, not the EffectInst).
                    // Mirror the `lower_call.rs` var-shaped branch: fetch the
                    // closure via the zero-arg getter, then dispatch through
                    // `js_closure_callN` with the user args. Without this every
                    // `Effect.succeed`/`Effect.runSync` etc. on the native
                    // `compilePackages: ["effect"]` path returned a closure,
                    // which `runSync` then read `._tag` off and threw
                    // `Cannot read properties of undefined`.
                    //
                    // Both wildcard imports and namespaces reached through a
                    // re-export use the same getter ABI.
                    if ctx.imported_vars.contains(method_name) {
                        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                        for a in args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        if lowered.len() > 16 {
                            bail!(
                                "perry-codegen: namespace static-method closure call with {} args (max 16)",
                                lowered.len()
                            );
                        }
                        ctx.pending_declares.push((fn_name.clone(), DOUBLE, vec![]));
                        let closure_box = ctx.block().call(DOUBLE, &fn_name, &[]);
                        let blk = ctx.block();
                        let closure_handle = unbox_to_i64(blk, &closure_box);
                        let runtime_fn = format!("js_closure_call{}", lowered.len());
                        let mut call_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, &closure_handle)];
                        for v in &lowered {
                            call_args.push((DOUBLE, v.as_str()));
                        }
                        return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
                    }
                    let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                    for a in args {
                        lowered.push(lower_expr(ctx, a)?);
                    }
                    let arg_kinds: Vec<crate::types::LlvmType> =
                        std::iter::repeat_n(DOUBLE, args.len()).collect();
                    ctx.pending_declares
                        .push((fn_name.clone(), DOUBLE, arg_kinds));
                    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    return Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices));
                }
            }
            // Issue #818 (Effect.succeed pattern): the receiver is a NAMED
            // import (`import { Effect } from 'effect'`), not a namespace
            // alias. The HIR's "uppercase Ident looks like a class" rule
            // lifts `Effect.succeed(args)` to StaticMethodCall, but `Effect`
            // isn't a perry class and isn't in `namespace_imports`. When the
            // class_name resolves to a V8-fallback specifier, route through
            // the bridge: load the module, get the named member as an
            // object, then call .method on it. Without this the call fell
            // to the `double_literal(0.0)` stub below — Effect's
            // `Effect.succeed(42)` returned the literal `0` instead of the
            // tagged Effect instance.
            if let Some(specifier) = ctx.import_function_v8_specifiers.get(class_name).cloned() {
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                // The V8 module's top-level export uses the *imported* name
                // (the name in the source module). If the local alias differs
                // from the imported name, fall back to the local name — the
                // specifier-registration code in compile.rs registers both
                // when local != imported, so for a Named import the lookup
                // key here is the consumer-visible alias which equals the
                // remote name when no `as` rename is present.
                let member = ctx
                    .import_function_origin_names
                    .get(class_name)
                    .cloned()
                    .unwrap_or_else(|| class_name.clone());
                return Ok(emit_v8_member_method_call(
                    ctx,
                    &specifier,
                    &member,
                    method_name,
                    &lowered,
                ));
            }
            // #4831 (Stripe-style `StripeResource.extend(...)`): the receiver
            // is an imported *function* (or class-ref) that carries the called
            // method as a DYNAMIC own property — e.g. `function StripeResource()
            // {}; StripeResource.extend = protoExtend;` in one module, then
            // `StripeResource.extend({...})` in another. The HIR's "uppercase
            // imported Ident looks like a class" rule lifts this to a
            // `StaticMethodCall`, but there is no compile-time class static to
            // resolve (`ctx.methods` miss above), it isn't a namespace import,
            // and it isn't a V8-fallback specifier — so the pre-fix code fell
            // here and returned the literal `0`. That made every Stripe resource
            // method (`stripe.products.create`, etc.) `undefined`/non-callable.
            //
            // When the receiver name resolves to a materializable imported value
            // (a native `import_function_prefixes` symbol or a `class_ids`
            // class-ref), route the call through the runtime method dispatcher:
            // materialize the receiver, read the named method off its dynamic
            // props, and invoke it with `this` bound to the receiver. This is
            // the same dispatch the same-module `Base.extend()` path already
            // uses, and it is strictly better than the `0` stub for every other
            // case that reached here. Related: #4656 (general prototype-chain
            // `[[Get]]` inheritance); this fix is scoped to the cross-module
            // dynamic-method-on-imported-function call shape.
            if ctx.import_function_prefixes.contains_key(class_name)
                || ctx.class_ids.contains_key(class_name)
            {
                let recv_box = lower_expr(
                    ctx,
                    &Expr::ExternFuncRef {
                        name: class_name.clone(),
                        param_types: vec![],
                        return_type: HirType::Any,
                    },
                )?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                let (args_ptr, args_len) = if lowered_args.is_empty() {
                    ("null".to_string(), "0".to_string())
                } else {
                    let n = lowered_args.len();
                    let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                    {
                        let blk = ctx.block();
                        for (i, value) in lowered_args.iter().enumerate() {
                            let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                            blk.store(DOUBLE, value, &slot);
                        }
                    }
                    (buf, n.to_string())
                };
                let method_idx = ctx.strings.intern(method_name);
                let entry = ctx.strings.entry(method_idx);
                let bytes_global = format!("@{}", entry.bytes_global);
                let name_len = entry.byte_len.to_string();
                return Ok(ctx.block().call(
                    DOUBLE,
                    "js_native_call_method",
                    &[
                        (DOUBLE, &recv_box),
                        (PTR, &bytes_global),
                        (I64, &name_len),
                        (PTR, &args_ptr),
                        (I64, &args_len),
                    ],
                ));
            }
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            Ok(double_literal(0.0))
        }

        // -------- super.method(args) --------
        // Walk the current class's parent chain for the named method
        // (skipping the current class itself, even if it overrides
        // the same name) and emit a direct call to the resolved
        // perry_method_<modprefix>__<parent>__<name> with `this`.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}
