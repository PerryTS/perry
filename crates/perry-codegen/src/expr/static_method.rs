//! StaticMethodCall.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::Result;
use perry_hir::types::Type as HirType;
use perry_hir::Expr;

use crate::nanbox::double_literal;
use crate::native_value::MaterializationReason;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::{
    downgrade_buffer_aliases_in_expr, emit_v8_export_call, emit_v8_member_method_call,
    import_origin_suffix_ns, lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx,
};

/// A static-call site's guard (`js_class_static_call_guard`): the declared
/// body runs directly only while the property the call reads is still that
/// declaration's function object, a fact carried by the class function
/// objects' shapes. The site's memo is a runtime `StaticCallMemo` (four
/// words; thread-local when the program starts workers, so each agent arms
/// its own) holding the (pinned) class function objects. The hit is inline,
/// one shape word per class the read consults:
///
/// ```text
///   [receiver == memo.c_value]                                  else MISS
///   [[memo.c + PROPS] + SHAPE]      == low half of memo.key     else MISS
///   [[[memo.owner + PROPS] + SHAPE] == high half of memo.key    else MISS]
///   MISS: ok = miss_fn(miss_args..., memo)  (re-validates, re-arms)
/// ```
///
/// Never armed, both object words point at the site's constant, whose
/// own-property word points at itself and whose shape word (0) never equals
/// a half of the unarmed key (all ones).
///
/// `same_owner`: the class the call names declares the body itself, so its
/// one shape word is both halves. `receiver_bits`: a site whose receiver is a
/// value must also be looking at the memo's class function object. Returns
/// the i1 "the body may run directly".
pub(crate) fn emit_static_call_guard(
    ctx: &mut FnCtx<'_>,
    receiver_bits: Option<&str>,
    same_owner: bool,
    miss_fn: &str,
    miss_args: &[(crate::types::LlvmType, String)],
) -> String {
    use crate::types::I1;
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let site_name = crate::expr::inline_cache_global_name(ctx, site);
    let memo = format!("@{site_name}_smemo");
    let unarmed = format!("@{site_name}_sunarmed");
    let tls = if crate::codegen::program_has_worker() {
        "thread_local "
    } else {
        ""
    };
    // `[unarmed + PROPS]` is `unarmed`; `[unarmed + SHAPE]` is 0.
    let props_words = crate::runtime_abi::CLOSURE_PROPS_OFFSET / 8;
    debug_assert!(crate::runtime_abi::OBJECT_SHAPE_OFFSET + 4 <= 8 * props_words);
    ctx.typed_parse_rodata.push(format!(
        "{unarmed} = private constant {{ [{props_words} x i64], ptr }} {{ [{props_words} x i64] zeroinitializer, ptr {unarmed} }}, align 8"
    ));
    ctx.typed_parse_rodata.push(format!(
        "{memo} = private {tls}global {{ i64, ptr, ptr, i64 }} {{ i64 -1, ptr {unarmed}, ptr {unarmed}, i64 0 }}, align 8"
    ));
    let mut args: Vec<(crate::types::LlvmType, &str)> =
        miss_args.iter().map(|(t, v)| (*t, v.as_str())).collect();
    args.push((PTR, &memo));
    // The inline hit reads little-endian LP64 layouts (8-byte memo words, the
    // key's halves); other targets always ask the runtime.
    let triple = ctx.target_triple;
    let lp64_le = (triple.starts_with("x86_64") || triple.starts_with("aarch64"))
        && !triple.starts_with("aarch64_be")
        && !triple.contains("32");
    if !lp64_le {
        let ok = ctx.block().call(I32, miss_fn, &args);
        return ctx.block().icmp_ne(I32, &ok, "0");
    }
    let miss_idx = ctx.new_block("static_guard.miss");
    let join_idx = ctx.new_block("static_guard.join");
    let miss_l = ctx.block_label(miss_idx);
    let join_l = ctx.block_label(join_idx);
    // One test per block, each expected to pass: the hit falls straight
    // through to the direct call, every miss branches out of line.
    let test = |ctx: &mut FnCtx<'_>, pass: &str, last: bool| {
        let pass = ctx
            .block()
            .call(I1, "llvm.expect.i1", &[(I1, pass), (I1, "true")]);
        if last {
            ctx.block().cond_br(&pass, &join_l, &miss_l);
        } else {
            let next = ctx.new_block("static_guard.check");
            let next_l = ctx.block_label(next);
            ctx.block().cond_br(&pass, &next_l, &miss_l);
            ctx.current_block = next;
        }
    };
    let memo_word = |ctx: &mut FnCtx<'_>, ty: crate::types::LlvmType, offset: usize| -> String {
        let p = ctx
            .block()
            .gep(crate::types::I8, &memo, &[(I64, &offset.to_string())]);
        ctx.block().load(ty, &p)
    };
    let shape_matches = |ctx: &mut FnCtx<'_>, fo_offset: usize, key_offset: usize| -> String {
        let fo = memo_word(ctx, PTR, fo_offset);
        let props = crate::runtime_abi::CLOSURE_PROPS_OFFSET.to_string();
        let pp = ctx.block().gep(crate::types::I8, &fo, &[(I64, &props)]);
        let bag = ctx.block().load(PTR, &pp);
        let shape = crate::runtime_abi::OBJECT_SHAPE_OFFSET.to_string();
        let sp = ctx.block().gep(crate::types::I8, &bag, &[(I64, &shape)]);
        let w = ctx.block().load(I32, &sp);
        let k = memo_word(ctx, I32, key_offset);
        ctx.block().icmp_eq(I32, &w, &k)
    };
    if let Some(bits) = receiver_bits {
        let v = memo_word(ctx, I64, crate::runtime_abi::STATIC_CALL_MEMO_VALUE_OFFSET);
        let same = ctx.block().icmp_eq(I64, bits, &v);
        test(ctx, &same, false);
    }
    let key = crate::runtime_abi::STATIC_CALL_MEMO_KEY_OFFSET;
    let c_ok = shape_matches(ctx, crate::runtime_abi::STATIC_CALL_MEMO_C_OFFSET, key);
    test(ctx, &c_ok, same_owner);
    if !same_owner {
        let o_ok = shape_matches(
            ctx,
            crate::runtime_abi::STATIC_CALL_MEMO_OWNER_OFFSET,
            key + 4,
        );
        test(ctx, &o_ok, true);
    }
    let hit_pred = ctx.block().label.clone();
    ctx.current_block = miss_idx;
    let ok = ctx.block().call(I32, miss_fn, &args);
    let ok = ctx.block().icmp_ne(I32, &ok, "0");
    let miss_pred = ctx.block().label.clone();
    ctx.block().br(&join_l);
    ctx.current_block = join_idx;
    ctx.block()
        .phi(I1, &[("true", &hit_pred), (&ok, &miss_pred)])
}

fn downgrade_unknown_call_args(ctx: &mut FnCtx<'_>, args: &[Expr]) {
    for arg in args {
        downgrade_buffer_aliases_in_expr(ctx, arg, MaterializationReason::UnknownCallEscape);
    }
}

fn lower_imported_value_method(
    ctx: &mut FnCtx<'_>,
    name: &str,
    method: &str,
    args: &[Expr],
) -> Result<String> {
    let receiver = Expr::ExternFuncRef {
        name: name.to_string(),
        param_types: vec![],
        return_type: HirType::Any,
    };
    let mut operands = vec![&receiver];
    operands.extend(args.iter());
    crate::rooting::with_operands_rooted(ctx, &operands, |ctx, values| {
        let (recv_box, lowered_args) = values.split_first().expect("receiver operand");
        let (args_ptr, args_len) = if lowered_args.is_empty() {
            ("null".to_string(), "0".to_string())
        } else {
            let buf = ctx.func.alloca_entry_array(DOUBLE, lowered_args.len());
            let blk = ctx.block();
            for (i, value) in lowered_args.iter().enumerate() {
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                blk.store(DOUBLE, value, &slot);
            }
            (buf, lowered_args.len().to_string())
        };
        let method_idx = ctx.strings.intern(method);
        let entry = ctx.strings.entry(method_idx);
        let bytes_global = format!("@{}", entry.bytes_global);
        let name_len = entry.byte_len.to_string();
        Ok(ctx.block().call(
            DOUBLE,
            "js_native_call_method",
            &[
                (DOUBLE, recv_box),
                (PTR, &bytes_global),
                (I64, &name_len),
                (PTR, &args_ptr),
                (I64, &args_len),
            ],
        ))
    })
}

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::StaticMethodCall {
            class_name,
            method_name,
            args,
        } => {
            downgrade_unknown_call_args(ctx, args);
            // HIR may classify an uppercase object import as a static call.
            // Lexical variable imports win over unrelated same-named class
            // metadata, including when that class has this static method.
            if ctx.imported_vars.contains(class_name) {
                return lower_imported_value_method(ctx, class_name, method_name, args);
            }
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
                // The shape is the authority: the body runs directly only while
                // the property a call reads (`C.m`, on C or the class it is
                // inherited from) is still this declaration's function object.
                // The check precedes the arguments, as the property read does;
                // otherwise the call reads the property and calls its value on
                // the class function object (pinned, so it survives the
                // argument evaluation).
                // A `static { }` block's synthetic method is not a member: the
                // class initializer calls it directly, it has no property.
                let static_cid = if method_name.starts_with("__perry_static_init_") {
                    None
                } else {
                    ctx.class_ids.get(class_name).copied()
                };
                // The class whose ClassBody declares the body this call runs.
                let owner_cid = {
                    let mut cur = class_name.clone();
                    let mut found = 0u32;
                    for _ in 0..32 {
                        let Some(c) = ctx.classes.get(&cur) else {
                            break;
                        };
                        if c.static_methods.iter().any(|m| m.name == *method_name) {
                            found = ctx.class_ids.get(&cur).copied().unwrap_or(0);
                            break;
                        }
                        match c.extends_name.clone() {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                    found
                };
                let name_idx = ctx.strings.intern(method_name);
                let name_entry = ctx.strings.entry(name_idx);
                let name_bytes = format!("@{}", name_entry.bytes_global);
                let name_len = name_entry.byte_len.to_string();
                let guard = match static_cid {
                    Some(cid) => {
                        let cid_str = cid.to_string();
                        // Per-site memo of the class function objects' shapes
                        // that proved the declaration (validated per use).
                        let body_i64 = ctx.block().ptrtoint(&format!("@{}", fn_name), I64);
                        let ok = emit_static_call_guard(
                            ctx,
                            None,
                            owner_cid == cid,
                            "js_class_static_call_guard",
                            &[
                                (I32, cid_str.clone()),
                                (I32, owner_cid.to_string()),
                                (PTR, name_bytes.clone()),
                                (I64, name_len.clone()),
                                (I64, body_i64),
                            ],
                        );
                        let recv_idx = ctx.new_block("static_call.receiver");
                        let args_idx = ctx.new_block("static_call.args");
                        let recv_label = ctx.block_label(recv_idx);
                        let args_label = ctx.block_label(args_idx);
                        let pre_label = ctx.block().label.clone();
                        ctx.block().cond_br(&ok, &args_label, &recv_label);
                        ctx.current_block = recv_idx;
                        let recv = ctx
                            .block()
                            .call(DOUBLE, "js_class_value", &[(I32, &cid_str)]);
                        let recv_pred = ctx.block().label.clone();
                        ctx.block().br(&args_label);
                        ctx.current_block = args_idx;
                        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        let recv = ctx
                            .block()
                            .phi(DOUBLE, &[(&undef, &pre_label), (&recv, &recv_pred)]);
                        Some((ok, recv))
                    }
                    None => None,
                };
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                let raw_args = lowered.clone();
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
                        let has_synthetic_arguments = ctx
                            .method_has_synthetic_arguments
                            .get(&key)
                            .copied()
                            .unwrap_or(false);
                        // #8162: a static body with BOTH a user `...rest` and an
                        // `arguments` read declares `[a, rest, arguments]` — TWO
                        // trailing array slots, so two come off the declared
                        // count. Read off the static-method HIR: statics have
                        // their own list, and the registry key differs from the
                        // source name, so `method_has_user_rest` (instance-only)
                        // does not apply here.
                        let has_user_rest = ctx
                            .classes
                            .get(class_name)
                            .and_then(|c| c.static_methods.iter().find(|m| m.name == *method_name))
                            .map(|m| {
                                m.params
                                    .iter()
                                    .any(|p| p.is_rest && p.arguments_object.is_none())
                            })
                            .unwrap_or(false);
                        let trailing_slots = if has_synthetic_arguments && has_user_rest {
                            2
                        } else {
                            1
                        };
                        let fixed = declared_count.saturating_sub(trailing_slots);
                        let all_actual = std::mem::take(&mut lowered);
                        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        for i in 0..fixed {
                            lowered
                                .push(all_actual.get(i).cloned().unwrap_or_else(|| undef.clone()));
                        }
                        // (bundled slice, mark as arguments object), in callee
                        // param order: the user rest slot takes the args PAST
                        // the fixed params; the synthesized `arguments` slot
                        // takes EVERY passed arg and is marked, without which
                        // the callee's `arguments` is an ordinary Array.
                        let mut bundles: Vec<(&[String], bool)> = Vec::new();
                        if has_user_rest || !has_synthetic_arguments {
                            bundles.push((all_actual.get(fixed..).unwrap_or(&[]), false));
                        }
                        if has_synthetic_arguments {
                            bundles.push((all_actual.as_slice(), true));
                        }
                        for (packed, mark) in bundles {
                            let arr_handle = ctx.block().call(
                                I64,
                                "js_array_alloc",
                                &[(I32, &packed.len().to_string())],
                            );
                            // js_array_push_f64 may realloc and return a
                            // possibly-new handle; thread it.
                            let mut handle_cur = arr_handle;
                            for value in packed {
                                handle_cur = ctx.block().call(
                                    I64,
                                    "js_array_push_f64",
                                    &[(I64, &handle_cur), (DOUBLE, value)],
                                );
                            }
                            if mark {
                                handle_cur = ctx.block().call(
                                    I64,
                                    "js_array_mark_arguments_object",
                                    &[(I64, &handle_cur)],
                                );
                            }
                            let arr_box = nanbox_pointer_inline(ctx.block(), &handle_cur);
                            lowered.push(arr_box);
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
                let Some((ok, recv)) = guard else {
                    return Ok(ctx.block().call(DOUBLE, &fn_name, &arg_slices));
                };
                let direct_idx = ctx.new_block("static_call.direct");
                let generic_idx = ctx.new_block("static_call.property");
                let join_idx = ctx.new_block("static_call.join");
                let direct_label = ctx.block_label(direct_idx);
                let generic_label = ctx.block_label(generic_idx);
                let join_label = ctx.block_label(join_idx);
                ctx.block().cond_br(&ok, &direct_label, &generic_label);
                ctx.current_block = direct_idx;
                if !owns_method {
                    let cid_str = static_cid.unwrap_or(0).to_string();
                    ctx.block()
                        .call_void("js_static_this_arm_classref", &[(I32, &cid_str)]);
                }
                let direct = ctx.block().call(DOUBLE, &fn_name, &arg_slices);
                let direct_pred = ctx.block().label.clone();
                ctx.block().br(&join_label);
                ctx.current_block = generic_idx;
                let (args_ptr, args_len) = if raw_args.is_empty() {
                    ("null".to_string(), "0".to_string())
                } else {
                    let buf = ctx.func.alloca_entry_array(DOUBLE, raw_args.len());
                    let blk = ctx.block();
                    for (i, value) in raw_args.iter().enumerate() {
                        let slot = blk.gep(DOUBLE, &buf, &[(I64, &i.to_string())]);
                        blk.store(DOUBLE, value, &slot);
                    }
                    (buf, raw_args.len().to_string())
                };
                let via_property = ctx.block().call(
                    DOUBLE,
                    "js_native_call_method",
                    &[
                        (DOUBLE, &recv),
                        (PTR, &name_bytes),
                        (I64, &name_len),
                        (PTR, &args_ptr),
                        (I64, &args_len),
                    ],
                );
                let generic_pred = ctx.block().label.clone();
                ctx.block().br(&join_label);
                ctx.current_block = join_idx;
                return Ok(ctx.block().phi(
                    DOUBLE,
                    &[(&direct, &direct_pred), (&via_property, &generic_pred)],
                ));
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
                    if ctx
                        .imported_vars
                        .contains(&crate::namespace_member_var_key(class_name, method_name))
                    {
                        ctx.pending_declares.push((fn_name.clone(), DOUBLE, vec![]));
                        // Preserve JavaScript evaluation order: fetch the
                        // callable namespace member before evaluating any
                        // argument, then root it across that argument window.
                        let closure_box = ctx.block().call(DOUBLE, &fn_name, &[]);
                        let arg_refs: Vec<&Expr> = args.iter().collect();
                        let lowered_args = std::cell::RefCell::new(Vec::<String>::new());
                        let result = crate::rooting::with_rooted_accumulator(
                            ctx,
                            crate::rooting::Repr::Boxed,
                            &closure_box,
                            crate::rooting::any_operand_may_collect(ctx, args.iter()),
                            |ctx, _closure| {
                                crate::rooting::with_operands_rooted(
                                    ctx,
                                    &arg_refs,
                                    |_ctx, vals| {
                                        *lowered_args.borrow_mut() = vals.to_vec();
                                        Ok(())
                                    },
                                )
                            },
                            |ctx, closure_box| {
                                let lowered = lowered_args.borrow();
                                let closure_handle = {
                                    let blk = ctx.block();
                                    unbox_to_i64(blk, closure_box)
                                };
                                // #3527: namespace members backed by exported
                                // closure getters need the same arbitrary-arity
                                // array dispatch as ordinary closure values.
                                // Effect's `Layer.mergeAll` is a rest closure
                                // and OpenCode passes 18 layers here.
                                Ok(crate::lower_call::emit_closure_handle_call(
                                    ctx,
                                    &closure_handle,
                                    &lowered,
                                ))
                            },
                        )?;
                        return Ok(result);
                    }
                    // Function-declaration namespace members need the same ABI
                    // adaptation as the lowercase namespace-call path. In
                    // particular, a rest declaration receives its trailing
                    // arguments as one array parameter; passing the source
                    // arguments directly makes `Event.inventory(...defs)` read
                    // a scalar as an array and return `undefined`.
                    let scoped_func_key = crate::namespace_member_func_key(class_name, method_name);
                    let scoped_declared_count = ctx
                        .imported_func_param_counts
                        .get(&scoped_func_key)
                        .copied();
                    let declared_count = scoped_declared_count
                        .or_else(|| ctx.imported_func_param_counts.get(method_name).copied())
                        .unwrap_or(args.len());
                    let has_rest = if scoped_declared_count.is_some() {
                        ctx.imported_func_has_rest.contains(&scoped_func_key)
                    } else {
                        ctx.imported_func_has_rest.contains(method_name)
                    };
                    if has_rest {
                        let fixed_count = declared_count.saturating_sub(1);
                        let (lowered, guard) = crate::lower_call::lower_rest_call_args_rooted(
                            ctx,
                            args,
                            fixed_count,
                            &[crate::lower_call::RestBundle {
                                from: fixed_count,
                                mark_arguments_object: false,
                            }],
                        )?;
                        let arg_kinds: Vec<crate::types::LlvmType> =
                            std::iter::repeat_n(DOUBLE, lowered.len()).collect();
                        ctx.pending_declares
                            .push((fn_name.clone(), DOUBLE, arg_kinds));
                        return Ok(crate::lower_call::emit_rooted_call(
                            ctx, &fn_name, &lowered, guard,
                        ));
                    }

                    let (mut lowered, guard) =
                        crate::lower_call::lower_call_args_rooted(ctx, args)?;
                    lowered.resize(
                        lowered.len().max(declared_count),
                        double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                    );
                    let arg_kinds: Vec<crate::types::LlvmType> =
                        std::iter::repeat_n(DOUBLE, lowered.len()).collect();
                    ctx.pending_declares
                        .push((fn_name.clone(), DOUBLE, arg_kinds));
                    return Ok(crate::lower_call::emit_rooted_call(
                        ctx, &fn_name, &lowered, guard,
                    ));
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
                return lower_imported_value_method(ctx, class_name, method_name, args);
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
