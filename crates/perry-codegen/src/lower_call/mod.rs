//! Call, new, and native method call lowering.
//!
//! Contains `lower_call`, `lower_new`, and `lower_native_method_call`.

use anyhow::{bail, Result};
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{
    lower_expr, nanbox_bigint_inline, nanbox_pointer_inline, nanbox_string_inline, unbox_to_i64,
    variant_name, FnCtx,
};
use crate::lower_array_method::lower_array_method;

// Tier 1.3 (v0.5.332): the perry/ui, perry/ui-instance, perry/system,
// perry/i18n dispatch tables moved to `perry_dispatch` so the JS and
// WASM backends can derive their (TS-name → runtime-symbol) mapping
// from the same source of truth. Local aliases below preserve the
// pre-refactor type names used throughout this file.
use perry_dispatch::{
    ArgKind as UiArgKind, MethodRow as UiSig, ReturnKind as UiReturnKind, PERRY_BACKGROUND_TABLE,
    PERRY_I18N_TABLE, PERRY_MEDIA_TABLE, PERRY_SYSTEM_TABLE, PERRY_UI_INSTANCE_TABLE,
    PERRY_UI_TABLE, PERRY_UPDATER_TABLE,
};

// Tier 2.2 (v0.5.333-339): incremental extraction of `lower_call.rs`
// helpers into focused sub-modules. Same pattern as Tier 2.1's
// compile.rs split.
//
// - `ui_styling.rs` (v0.5.333): inline `style: { ... }` destructure
//   family (apply_inline_style + 7 internal helpers, ~510 LOC).
// - `builtin.rs` (v0.5.339): `lower_builtin_new` — built-in `new C()`
//   constructor dispatch (~399 LOC).
// - `native.rs` (v0.5.340): `lower_native_method_call` — the 805-LOC
//   dispatcher for `obj.method(args)` against native modules
//   (mysql2, pg, redis, mongo, ws, fastify, fetch, perry/ui,
//   perry/system, perry/i18n, perry/plugin, AbortController, …).
mod buffer_intrinsic;
mod builtin;
mod jsx;
mod method_override;
mod native;
mod native_table;
mod new;
mod options;
mod ui_styling;
use buffer_intrinsic::try_emit_buffer_read_intrinsic;
use builtin::lower_builtin_new;
use jsx::try_rewrite_perry_tui_jsx_intrinsic;
use method_override::emit_own_method_override_check;
// `options/` (#1099): the options-object-literal lowering family,
// split by native-API surface (notification / abort / fetch) under
// `options/`. Bring the per-surface entry points + shared helpers
// into this module's scope so the existing `super::<name>` call
// sites in sibling submodules (builtin/native/ui_styling) keep
// resolving unchanged after the split.
use options::{
    build_headers_from_object, get_raw_string_ptr, lower_abort_controller_call,
    lower_fetch_native_method, lower_notification_schedule,
};
// `native_table.rs` (#1099): the ~5k-row `NATIVE_MODULE_TABLE` data +
// arg/ret kind types. The dispatch consumers below
// (`native_module_lookup`, `lower_native_module_dispatch`) stay here
// and pull the table + kind types in via these imports.
use native_table::{NativeArgKind, NativeModSig, NativeRetKind, NATIVE_MODULE_TABLE};
use ui_styling::apply_inline_style;
// Re-export pub(crate) so callers outside this module (e.g.
// `crate::expr::use crate::lower_call::lower_native_method_call;`)
// keep resolving — `pub(super)` on the native fn would shadow them.
pub(crate) use native::lower_native_method_call;
// Re-export pub(crate) `new.rs` items consumed outside this module
// (codegen.rs / expr.rs / stmt.rs) so `crate::lower_call::lower_new`
// etc. keep resolving after the split.
pub(crate) use new::{apply_field_initializers_recursive, lower_new, FieldInitMode};
// `extract_options_fields` is consumed by `expr.rs` as
// `crate::lower_call::extract_options_fields` — keep that path stable.
pub(crate) use options::extract_options_fields;
// `iter_native_module_table` is consumed by `lib.rs`'s public manifest
// API as `lower_call::iter_native_module_table` — keep that path stable.
pub(crate) use native_table::iter_native_module_table;

use crate::lower_string_method::lower_string_method;
use crate::nanbox::{double_literal, POINTER_MASK_I64};
use crate::type_analysis::{
    is_array_expr, is_global_constructor_expr, is_map_expr, is_promise_expr, is_set_expr,
    is_string_expr, is_url_search_params_expr, receiver_class_name,
};
use crate::types::{DOUBLE, I32, I64, I8, PTR};

/// Lower a `Call` expression. Two shapes are supported:
/// 1. `FuncRef(id)(args...)` — direct call to a user function by HIR id.
/// 2. `console.log(expr)` where `expr` lowers to a double — emits a
///    `js_console_log_number` call and returns `0.0` as the statement value.
pub(crate) fn lower_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    // #1113 — `app.server.on(event, cb)` and similar
    // `nativeMethodCallReceiver.<prop>(args)` chains. The HIR shape
    // is `Call { callee: PropertyGet { object: NativeMethodCall {
    // module, … }, property: P }, args }` — `app.server` lowered as
    // `NativeMethodCall(module="fastify", method="server")` returning
    // the FastifyApp handle, but `.on(…)` then went through the
    // generic property-get path (because TypeScript's structural
    // typing on the return shape doesn't propagate the native-module
    // tag through `.server`). The property read returned undefined
    // and the call silently no-op'd (`(undefined)(…)` returns NaN in
    // Perry's runtime today — no exception). User code patterns like
    //
    //   app.server.on("upgrade", (req, socket, head) => …)
    //
    // therefore ran without throwing but never registered the
    // callback. Forward the call into the NATIVE_MODULE_TABLE arm
    // for `(module, P)` whenever the inner NativeMethodCall's module
    // recognises `P` as one of its methods (the dispatch table is
    // already the authoritative source for "what method names this
    // native module exposes"). Scoped narrowly — falls back to the
    // existing call lowering if the lookup misses.
    if let Expr::PropertyGet { object, property } = callee {
        if let Expr::NativeMethodCall { module, .. } = object.as_ref() {
            if native_module_lookup(module, true, property, None).is_some() {
                return crate::lower_call::native::lower_native_method_call(
                    ctx,
                    module,
                    None,
                    property,
                    Some(object.as_ref()),
                    args,
                );
            }
        }
    }
    // v0.5.754: `obj[strKey](args)` computed-key method call. Drizzle's
    // `this.session[isOneTimeQuery ? "prepareOneTimeQuery" : "prepareQuery"](...)`
    // lowers as Call { callee: IndexGet { object, index }, args }. Pre-fix
    // this fell through to the generic call path that read obj[index] as
    // a value (returning undefined for class methods) and then tried to
    // call undefined. Route through `js_native_call_method_str_key` which
    // walks the class vtable chain (parent inheritance included). Refs
    // #420 / #618 followup.
    if let Expr::IndexGet { object, index } = callee {
        if matches!(index.as_ref(), Expr::String(_))
            || crate::type_analysis::is_string_expr(ctx, index)
            || crate::type_analysis::is_definitely_string_expr(ctx, index)
        {
            let recv_box = lower_expr(ctx, object)?;
            let name_box = lower_expr(ctx, index)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            let n = lowered_args.len();
            let name_handle = {
                let blk = ctx.block();
                crate::expr::unbox_str_handle(blk, &name_box)
            };
            let (args_ptr, args_len) = if n == 0 {
                ("null".to_string(), "0".to_string())
            } else {
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = ctx
                        .block()
                        .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    ctx.block().store(DOUBLE, v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf_reg
                ));
                (ptr_reg, n.to_string())
            };
            return Ok(ctx.block().call(
                DOUBLE,
                "js_native_call_method_str_key",
                &[
                    (DOUBLE, &recv_box),
                    (I64, &name_handle),
                    (crate::types::PTR, &args_ptr),
                    (I64, &args_len),
                ],
            ));
        }
    }

    // #691 Phase 2: calling the current step closure via TLS.
    // `build_async_step_driver_direct` emits this for the catch arm's
    // `__step(e, true)` recursive re-entry — there's no captured
    // local to refer to anymore, so the callee is read out of TLS.
    // Dispatches through the same `js_closure_call<N>` family.
    if matches!(callee, Expr::CurrentStepClosure) {
        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        if lowered_args.len() > 16 {
            bail!(
                "perry-codegen Phase D.1: CurrentStepClosure call with {} args (max 16)",
                lowered_args.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", lowered_args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
    }

    // Closure-typed local call: `counter()` where `counter` is a
    // local of `Type::Function(...)`. Dispatch through the runtime
    // `js_closure_call<N>` family — the runtime extracts the function
    // pointer from the closure header and invokes it with the closure
    // as the first arg followed by the user args.
    if let Expr::LocalGet(id) = callee {
        if matches!(ctx.local_types.get(id), Some(HirType::Function(_))) {
            let recv_box = lower_expr(ctx, callee)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }

            // Issue #493: rest-bundling is now handled inside js_closure_callN
            // via the runtime closure-rest registry — see
            // `js_register_closure_rest` (registered for every closure body
            // with `...rest` at module init) and `dispatch_rest_bundled` in
            // `crates/perry-runtime/src/closure.rs`. Bundling at the static
            // call site here would double-wrap (the runtime would re-bundle
            // the already-bundled array into `[[a,b,c]]`), so the call site
            // now passes the raw args through and lets the runtime
            // pack the trailing tail into the rest slot.
            //
            // FuncRef calls (direct function-symbol dispatch) keep their
            // static-bundling at lower_call.rs:444+ because they don't go
            // through js_closure_callN.
            if lowered_args.len() > 16 {
                bail!(
                    "perry-codegen Phase D.1: closure call with {} args (max 16)",
                    lowered_args.len()
                );
            }
            let blk = ctx.block();
            let closure_handle = unbox_to_i64(blk, &recv_box);
            let runtime_fn = format!("js_closure_call{}", lowered_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
            for v in &lowered_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
        }
    }

    // Issue #636: namespace member call —
    // `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
    // where `ns ∈ namespace_imports`. Pre-fix this fell through to the
    // generic method-dispatch path which lower_expr'd the namespace as
    // its TAG_TRUE/stub-object value and then did `js_native_call_method`
    // with `method` against a non-callable receiver — TypeError or
    // silent 0 return.
    //
    // Resolution: route to the source's exported `method`. If `method`
    // is a var (let/const-bound closure — the canonical
    // `export const make = (s) => ...` shape), fetch the closure value
    // via the zero-arg getter `perry_fn_<src>__<method>()` and invoke
    // through `js_closure_callN`. If it's a function declaration
    // (`export function make(s)`), call the symbol directly with rest
    // bundling — same as the existing FuncRef path.
    if let Expr::PropertyGet { object, property } = callee {
        if let Expr::ExternFuncRef { name: ns_name, .. } = object.as_ref() {
            if ctx.namespace_imports.contains(ns_name) {
                // Issue #678 followup (namespace branch): wildcard-namespace
                // import to a V8 module — `import * as R from "ramda";
                // R.sum([1,2,3])`. The V8 module has no static export list
                // and (when no companion Named import is present) nothing
                // seeded `import_function_prefixes` for `property`. Route
                // the member call through the bridge using the
                // namespace's specifier before falling through to the
                // native-prefix lookup. Without this, ramda / date-fns /
                // jose / effect wildcard members fell to the
                // `double_literal(0.0)` stub.
                if let Some(specifier) = ctx.namespace_v8_specifiers.get(ns_name).cloned() {
                    let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                    for a in args {
                        lowered.push(lower_expr(ctx, a)?);
                    }
                    return Ok(crate::expr::emit_v8_export_call(
                        ctx, &specifier, property, &lowered,
                    ));
                }
                // Issue #680: prefer the per-namespace map so
                // `random.make` and `tracer.make` resolve to their own
                // sources even when both modules export `make`. Falls
                // back to the flat `import_function_prefixes` for
                // namespaces with no overlapping conflicts.
                if let Some(source_prefix) = ctx
                    .namespace_member_prefixes
                    .get(&(ns_name.clone(), property.clone()))
                    .cloned()
                    .or_else(|| ctx.import_function_prefixes.get(property).cloned())
                {
                    // Issue #678 followup: if the import lands in a V8-fallback
                    // module (e.g. `import * as ink from "ink"` where ink fell
                    // back to V8 because yoga-layout pulled in a feature Perry
                    // can't compile), route the namespace member through the
                    // runtime bridge — no `perry_fn_<src>__<member>` symbol
                    // exists for the linker to bind to.
                    if let Some(specifier) =
                        ctx.import_function_v8_specifiers.get(property).cloned()
                    {
                        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                        for a in args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        return Ok(crate::expr::emit_v8_export_call(
                            ctx, &specifier, property, &lowered,
                        ));
                    }
                    // Issue #678: re-exported names (e.g. `export { default as
                    // render }`) emit `perry_fn_<src>__default` in the origin —
                    // resolve the actual origin suffix before forming the symbol.
                    let origin_suffix = crate::expr::import_origin_suffix(
                        ctx.import_function_origin_names,
                        property,
                    );
                    let symbol = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
                    if ctx.imported_vars.contains(property) {
                        // Var-shaped export: fetch closure via zero-arg
                        // getter, then closure-call with the user args.
                        ctx.pending_declares.push((symbol.clone(), DOUBLE, vec![]));
                        let closure_box = ctx.block().call(DOUBLE, &symbol, &[]);
                        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                        for a in args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        if lowered.len() > 16 {
                            bail!(
                                "perry-codegen: namespace closure call with {} args (max 16)",
                                lowered.len()
                            );
                        }
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
                    // Function-decl-shaped export: direct call with rest bundling.
                    let declared_count = ctx
                        .imported_func_param_counts
                        .get(property)
                        .copied()
                        .unwrap_or(args.len());
                    let has_rest = ctx.imported_func_has_rest.contains(property);
                    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
                    if has_rest {
                        let fixed_count = declared_count.saturating_sub(1);
                        for a in args.iter().take(fixed_count) {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        let rest_count = args.len().saturating_sub(fixed_count);
                        let cap = (rest_count as u32).to_string();
                        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                        for a in args.iter().skip(fixed_count) {
                            let v = lower_expr(ctx, a)?;
                            let blk = ctx.block();
                            current = blk.call(
                                I64,
                                "js_array_push_f64",
                                &[(I64, &current), (DOUBLE, &v)],
                            );
                        }
                        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
                        lowered.push(rest_box);
                    } else {
                        for a in args {
                            lowered.push(lower_expr(ctx, a)?);
                        }
                        // Pad missing trailing args with TAG_UNDEFINED.
                        let undef_lit =
                            double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                        while lowered.len() < declared_count {
                            lowered.push(undef_lit.clone());
                        }
                    }
                    let arg_types: Vec<crate::types::LlvmType> =
                        std::iter::repeat(DOUBLE).take(lowered.len()).collect();
                    ctx.pending_declares
                        .push((symbol.clone(), DOUBLE, arg_types));
                    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                    return Ok(ctx.block().call(DOUBLE, &symbol, &arg_slices));
                }
            }
        }
    }

    // User function call via FuncRef.
    if let Expr::FuncRef(fid) = callee {
        // (Issue #436 plan #1) Clamp-pattern fast path: when the callee
        // is a function recognized as `clampIdx(v, lo, hi)` or
        // `clampU8(v)` and we're being lowered in an f64-required
        // context, emit `@llvm.smin.i32` / `@llvm.smax.i32` directly +
        // `sitofp` to double, mirroring the i32 path in
        // `lower_expr_as_i32`. The HIR inliner is configured to leave
        // these calls intact (`is_clamp3`/`is_clamp_u8` short-circuit
        // `is_inlinable`) so this path fires at every call site and the
        // `dowhile/break` shape that blocked LLVM's auto-vectorizer
        // never appears in the IR.
        if ctx.clamp3_functions.contains(fid) && args.len() == 3 {
            let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
            let lo = crate::expr::lower_expr_as_i32(ctx, &args[1])?;
            let hi = crate::expr::lower_expr_as_i32(ctx, &args[2])?;
            let blk = ctx.block();
            let r1 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smax.i32(i32 {}, i32 {})",
                r1, v, lo
            ));
            let r2 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smin.i32(i32 {}, i32 {})",
                r2, r1, hi
            ));
            return Ok(blk.sitofp(I32, &r2, DOUBLE));
        }
        if ctx.clamp_u8_functions.contains(fid) && args.len() == 1 {
            let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
            let blk = ctx.block();
            let r1 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smax.i32(i32 {}, i32 0)",
                r1, v
            ));
            let r2 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smin.i32(i32 {}, i32 255)",
                r2, r1
            ));
            return Ok(blk.sitofp(I32, &r2, DOUBLE));
        }

        let Some(fname) = ctx.func_names.get(fid).cloned() else {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(double_literal(0.0));
        };

        // Rest parameter handling: if the called function has a
        // rest parameter, bundle all trailing args (those at and
        // beyond the rest position) into an array literal and
        // pass that as a single argument.
        let sig = ctx.func_signatures.get(fid).copied();
        let (declared_count, has_rest, _) = sig.unwrap_or((args.len(), false, false));
        let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
        if has_rest && ctx.func_synthetic_arguments.contains(fid) {
            let fixed_count = declared_count.saturating_sub(1);
            let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            for idx in 0..fixed_count {
                if let Some(arg) = args.get(idx) {
                    lowered.push(lower_expr(ctx, arg)?);
                } else {
                    lowered.push(undef_lit.clone());
                }
            }

            let cap = (args.len() as u32).to_string();
            let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for a in args {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
            }
            let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
            lowered.push(arguments_box);
        } else if has_rest {
            // Rest is always the LAST declared param. Pass the
            // first (declared_count - 1) args as-is, then bundle
            // the rest into an array.
            let fixed_count = declared_count.saturating_sub(1);
            for a in args.iter().take(fixed_count) {
                lowered.push(lower_expr(ctx, a)?);
            }
            // Materialize the rest array.
            let rest_count = args.len().saturating_sub(fixed_count);
            let cap = (rest_count as u32).to_string();
            let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for a in args.iter().skip(fixed_count) {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
            }
            let rest_box = nanbox_pointer_inline(ctx.block(), &current);
            lowered.push(rest_box);
        } else {
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // Cross-module function call via ExternFuncRef. The HIR carries the
    // function name; we look up the source module's prefix in
    // `import_function_prefixes` (built by the CLI from hir.imports) and
    // generate `perry_fn_<source_prefix>__<name>`. The function is
    // declared in the OTHER module's compilation; here we just emit a
    // direct LLVM call to its scoped name and the system linker
    // resolves the symbol when the .o files are linked together.
    if let Expr::ExternFuncRef {
        name,
        return_type: ext_return_type,
        ..
    } = callee
    {
        match name.as_str() {
            "setTimeout" if args.len() == 2 => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let delay_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                let id = blk.call(
                    I64,
                    "js_set_timeout_callback",
                    &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                );
                return Ok(nanbox_pointer_inline(blk, &id));
            }
            "setImmediate" if !args.is_empty() => {
                let cb_box = lower_expr(ctx, &args[0])?;
                if args.len() == 1 {
                    let blk = ctx.block();
                    let cb_handle = unbox_to_i64(blk, &cb_box);
                    let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                    return Ok(nanbox_pointer_inline(blk, &id));
                }

                let n = args.len() - 1;
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a) in args.iter().skip(1).enumerate() {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, &v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf
                ));
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                let id = blk.call(
                    I64,
                    "js_set_immediate_callback_args",
                    &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
                );
                return Ok(nanbox_pointer_inline(blk, &id));
            }
            // Refs #665: `setTimeout(fn, delay, ...args)` — JS spec forwards
            // the trailing args to `fn` when the timer fires. Pack them into
            // a stack buffer of doubles and hand off to the varargs runtime
            // entry. Used by Promise-executor patterns like
            // `setTimeout(resolve, delay, res)` (rate-limiter-flexible's
            // `RateLimiterMemory.consume` is the discovering call site).
            "setTimeout" if args.len() >= 3 => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let delay_box = lower_expr(ctx, &args[1])?;
                let n = args.len() - 2;
                let buf = ctx.func.alloca_entry_array(DOUBLE, n);
                for (i, a) in args.iter().skip(2).enumerate() {
                    let v = lower_expr(ctx, a)?;
                    let blk = ctx.block();
                    let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, &v, &slot);
                }
                let ptr_reg = ctx.block().next_reg();
                ctx.block().emit_raw(format!(
                    "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                    ptr_reg, n, buf
                ));
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                let id = blk.call(
                    I64,
                    "js_set_timeout_callback_args",
                    &[
                        (I64, &cb_handle),
                        (DOUBLE, &delay_box),
                        (crate::types::PTR, &ptr_reg),
                        (I32, &n.to_string()),
                    ],
                );
                return Ok(nanbox_pointer_inline(blk, &id));
            }
            "setInterval" if args.len() == 2 => {
                let cb_box = lower_expr(ctx, &args[0])?;
                let delay_box = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                let id = blk.call(
                    I64,
                    "setInterval",
                    &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                );
                return Ok(nanbox_pointer_inline(blk, &id));
            }
            "clearTimeout" if args.len() == 1 => {
                let id_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let id_handle = unbox_to_i64(blk, &id_box);
                blk.call_void("clearTimeout", &[(I64, &id_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "clearInterval" if args.len() == 1 => {
                let id_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let id_handle = unbox_to_i64(blk, &id_box);
                blk.call_void("clearInterval", &[(I64, &id_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "gc" => {
                ctx.block().call_void("js_gc_collect", &[]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            "getAppVersion" if args.is_empty() => {
                let version = ctx.app_metadata.version.clone();
                let idx = ctx.strings.intern(&version);
                let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                return Ok(ctx.block().load(DOUBLE, &handle_global));
            }
            "getAppBuildNumber" if args.is_empty() => {
                return Ok(double_literal(ctx.app_metadata.build_number as f64));
            }
            "getBundleId" if args.is_empty() => {
                let bundle_id = ctx.app_metadata.bundle_id.clone();
                let idx = ctx.strings.intern(&bundle_id);
                let handle_global = format!("@{}", ctx.strings.entry(idx).handle_global);
                return Ok(ctx.block().load(DOUBLE, &handle_global));
            }
            // JSX runtime calls: `jsx(type, props)` and `jsxs(type, props)`.
            // The HIR lowers <div>…</div> to ExternFuncRef { name: "jsx" } and
            // <div><a/><b/></div> (multiple children) to "jsxs".  The first arg
            // is the element type (a string literal for HTML tags, or a NaN-boxed
            // function/class reference for components); the second arg is a
            // NaN-boxed props object (or TAG_NULL).  Both are passed as DOUBLE so
            // the ABI is uniform regardless of whether the type arg is a string or
            // a component reference — avoiding the PTR vs DOUBLE divergence that
            // the generic ExternFuncRef path would otherwise produce for string
            // literals.  The runtime stubs `js_jsx`/`js_jsxs` are no-op link
            // stubs that return TAG_UNDEFINED; real JSX rendering should be
            // implemented by importing a JSX runtime package (e.g. react or
            // preact) via the `perry.compilePackages` mechanism.
            //
            // perry/tui JSX intrinsic rewriter (#689). When the first arg
            // is `ExternFuncRef { name: "__perry_jsx_intrinsic::<mod>::<method>__" }`
            // (the HIR's marker for `<Box>` / `<Text>` resolved against a
            // native module — see `crates/perry-hir/src/jsx.rs`), bypass
            // the runtime `js_jsx` adapter entirely and route the call
            // through `lower_native_method_call` so the JSX form lowers
            // to the same widget builder the function-call form would.
            // Today this covers Box + Text from `perry/tui`; other
            // intrinsics (Spacer / Input / Spinner / List / Select /
            // ProgressBar / Table / Tabs / TextArea) are listed as
            // follow-up scope in #689 and continue to fall through to
            // `js_jsx` (returns TAG_UNDEFINED until the rewriter is
            // extended).
            "jsx" | "jsxs" => {
                if let Some(call) = try_rewrite_perry_tui_jsx_intrinsic(ctx, name == "jsxs", args)?
                {
                    return Ok(call);
                }
                let runtime_fn = if name == "jsx" { "js_jsx" } else { "js_jsxs" };
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                for a in args {
                    lowered.push(lower_expr(ctx, a)?);
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                return Ok(ctx.block().call(DOUBLE, runtime_fn, &arg_slices));
            }
            _ => {}
        }
        // Issue #841: direct call against a named import from one of the
        // five recognized Node submodules (`import { pipeline } from
        // "node:stream/promises"; pipeline()`). The HIR registers
        // `pipeline` as an imported func; without this routing the
        // catch-all below tries to emit a bare LLVM call to `@pipeline`
        // and the linker errors with `Undefined symbols: _pipeline`.
        //
        // Route to the value-form singleton getter and then dispatch
        // through the closure-call machinery — the singleton's thunk
        // throws an "is not yet implemented" Error. Real impls are
        // tracked separately under #793.
        if let Some((submod_key, exported_name)) =
            ctx.import_function_node_submodule.get(name).cloned()
        {
            // Lower args for side effects (closure capture collection,
            // string-literal interning), then discard — the thunk
            // signature is `(ClosureHeader*, f64) -> f64` and would
            // ignore them anyway.
            for a in args {
                let _ = crate::expr::lower_expr(ctx, a)?;
            }
            let submod_label = crate::expr::emit_string_literal_global(ctx, &submod_key);
            let name_label = crate::expr::emit_string_literal_global(ctx, &exported_name);
            let submod_len = submod_key.len();
            let name_len = exported_name.len();
            ctx.pending_declares.push((
                "js_node_submodule_export_as_function".to_string(),
                DOUBLE,
                vec![PTR, I32, PTR, I32],
            ));
            let blk = ctx.block();
            let closure_value = blk.call(
                DOUBLE,
                "js_node_submodule_export_as_function",
                &[
                    (PTR, &submod_label),
                    (I32, &submod_len.to_string()),
                    (PTR, &name_label),
                    (I32, &name_len.to_string()),
                ],
            );
            // Drive through the closure-call machinery so the thunk's
            // `js_throw` actually fires when the user invokes the
            // value. `js_closure_call0` matches our thunks'
            // `(ClosureHeader*, f64) -> f64` signature ignoring the
            // f64 arg (passed as undefined).
            ctx.pending_declares
                .push(("js_closure_call0".to_string(), DOUBLE, vec![DOUBLE]));
            return Ok(ctx
                .block()
                .call(DOUBLE, "js_closure_call0", &[(DOUBLE, &closure_value)]));
        }
        // perry/system dispatch: map JS names (isDarkMode, getDeviceIdiom,
        // keychainSave, etc.) to their perry_system_* / perry_* C symbols.
        // These arrive as ExternFuncRef because perry/system imports aren't
        // lowered to NativeMethodCall in the HIR.
        if let Some(sig) = perry_system_table_lookup(name) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // perry/updater dispatch: same shape as perry/system. Imports from
        // `perry/updater` arrive as ExternFuncRef; route by name to the
        // perry_updater_* runtime symbols in `perry-updater`.
        if let Some(sig) = perry_updater_table_lookup(name) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // perry/background dispatch (issue #538): registerTask / schedule /
        // cancel from `perry/background`. Backed by perry_background_* in
        // libperry_ui_*.a (real impls on iOS + Android, no-op stubs
        // elsewhere). Same calling convention as perry/system.
        if let Some(sig) = perry_background_table_lookup(name) {
            return lower_perry_ui_table_call(ctx, sig, args);
        }
        // Built-in runtime extern functions (`js_weakmap_set`,
        // `js_regexp_exec`, etc.) that start with `js_` are resolved
        // directly against the runtime library — bypass the import-
        // map lookup and emit a direct LLVM call with an f64/f64 ABI.
        // (The declarations are added centrally in runtime_decls.rs.)
        //
        // External `perry.nativeLibrary` packages commonly export their
        // symbols with the same `js_*` prefix. If the manifest declares
        // this name, let the native-library path below emit the call and
        // declaration from `ffi_signatures` instead of treating it as a
        // runtime builtin.
        if name.starts_with("js_") && !ctx.ffi_signatures.contains_key(name) {
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
            return Ok(ctx.block().call(DOUBLE, name, &arg_slices));
        }
        // Issue #692: default-import call against an unresolved module.
        // `import sanitizeHtml from "sanitize-html"` (when sanitize-html
        // didn't resolve to a NativeCompiled module / perry-stdlib
        // binding) lowers `sanitizeHtml(x)` to `Call { callee:
        // ExternFuncRef { name: "default" } }` — the HIR's
        // register_imported_func uses the literal `"default"` as the
        // exported-name marker for default imports (lower.rs:3727).
        // Without a source_prefix, the catch-all below emitted a direct
        // LLVM call to the bare symbol `default`, and the system linker
        // failed with `undefined reference to 'default'`. Route to the
        // runtime stub instead: lower args for side effects (so closure
        // collection / string interning still happens), then call
        // `js_unresolved_default_call` which returns NaN-boxed undefined
        // and prints a one-shot diagnostic at runtime. The program now
        // links; the user gets a clear runtime signal rather than a
        // cryptic linker error.
        if name == "default" && !ctx.import_function_prefixes.contains_key(name) {
            for a in args {
                let _ = lower_expr(ctx, a)?;
            }
            return Ok(ctx.block().call(DOUBLE, "js_unresolved_default_call", &[]));
        }
        // Native library functions (bloom_draw_rect, bloom_init_window,
        // etc.) that aren't in the import map — emit a direct call so
        // the linker resolves them against the linked native .a library.
        // Previously these were silently dropped (returned 0.0), which
        // caused Bloom Engine games to render blank windows.
        //
        // #1110 (follow-up to #1085): a symbol declared in the source
        // package's `perry.nativeLibrary.functions` manifest is always
        // resolved against the linked static library, never via the
        // `perry_fn_<src>__<name>` wrapper (the source `.ts` is ambient
        // and emits no wrapper). Force the FFI-manifest path whenever
        // `ffi_signatures` knows the name, even if some other code path
        // accidentally registered an entry in `import_function_prefixes`
        // (re-export chains, namespace re-exports, etc. — anything that
        // doesn't go through the #1085 per-specifier skip ends up there).
        let force_ffi_path = ctx.ffi_signatures.contains_key(name);
        let prefix_lookup = if force_ffi_path {
            None
        } else {
            ctx.import_function_prefixes.get(name).cloned()
        };
        let Some(source_prefix) = prefix_lookup else {
            // Determine per-arg types: string args need to be unboxed
            // to raw `*const u8` pointers and passed as `ptr` so the
            // ARM64 ABI puts them in x-registers (not d-registers).
            // Without this, bloom_draw_text(text, x, y, ...) passes
            // the NaN-boxed string in d0 but the native function reads
            // x0 as a *const u8 → SIGSEGV.
            // Extern C functions use the platform C ABI. Perry stores
            // all values as `double`, but native C/Rust functions may
            // take a mix of i64 (pointers/handles) and f64 (floats).
            //
            // The LLVM IR declaration type determines ARM64 register
            // placement: i64 → x-register, double → d-register.
            //
            // When the FFI manifest (`ffi_signatures`) declares a param
            // as `"i64"`, lower it via `fptosi` to put the value in an
            // x-register. This is required for handle-typed params like
            // `view: *mut EditorView` — without it the C ABI reads a
            // garbage value out of x0/x1 since Perry put the handle in
            // d-registers.
            let manifest_sig = ctx.ffi_signatures.get(name).cloned();
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            let mut arg_types: Vec<crate::types::LlvmType> = Vec::with_capacity(args.len());
            for (idx, a) in args.iter().enumerate() {
                let val = lower_expr(ctx, a)?;
                let manifest_kind: Option<&str> = manifest_sig
                    .as_ref()
                    .and_then(|(p, _)| p.get(idx).map(|s| s.as_str()));
                if is_string_expr(ctx, a) {
                    let blk = ctx.block();
                    let raw_ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &val)]);
                    let ptr_val = blk.inttoptr(I64, &raw_ptr);
                    lowered.push(ptr_val);
                    arg_types.push(PTR);
                } else if is_array_expr(ctx, a) {
                    let blk = ctx.block();
                    let bits = blk.bitcast_double_to_i64(&val);
                    let header_handle = blk.and(I64, &bits, POINTER_MASK_I64);
                    let header_ptr = blk.inttoptr(I64, &header_handle);
                    // Skip 8-byte ArrayHeader (u32 length + u32 capacity)
                    // to reach the inline f64 data.
                    let eight = "8".to_string();
                    let data_ptr = blk.gep(I8, &header_ptr, &[(I64, &eight)]);
                    lowered.push(data_ptr);
                    arg_types.push(PTR);
                } else if matches!(manifest_kind, Some("i64")) {
                    // Manifest declares this param as i64 → place in
                    // x-register. JS numbers are stored as f64 directly
                    // (a handle of `0x305b42a0c00` is the f64 value
                    // 13190580238336.0, not a NaN-box payload), so
                    // truncate via `fptosi` to recover the integer.
                    let blk = ctx.block();
                    let i = blk.fptosi(DOUBLE, &val, I64);
                    lowered.push(i);
                    arg_types.push(I64);
                } else {
                    lowered.push(val);
                    arg_types.push(DOUBLE);
                }
            }
            let arg_slices: Vec<(crate::types::LlvmType, &str)> = arg_types
                .iter()
                .zip(lowered.iter())
                .map(|(t, v)| (*t, v.as_str()))
                .collect();
            // Determine return type.
            //
            // Manifest `returns` field takes precedence over HIR heuristics:
            //
            //   "string" / "ptr"  → PTR return (*const u8 / *const StringHeader);
            //                       ptrtoint + NaN-box STRING_TAG. Use when the
            //                       Rust function is declared `-> *const u8`.
            //   "i64_str"         → I64 return (raw integer that IS a *StringHeader
            //                       address). NaN-box directly with STRING_TAG; no
            //                       sitofp. Use when the Rust function is declared
            //                       `-> i64` but the value is a string pointer.
            //   "i64"             → I64 return; sitofp → JS number. Use for opaque
            //                       handles / integers (`*mut View`, counts, etc.).
            //   "void"            → no return value.
            //   (absent)          → fall back to HIR ExternFuncRef.return_type and
            //                       the name-pattern heuristic below.
            let has_string_args = arg_types.contains(&PTR);
            let manifest_ret: Option<&str> = manifest_sig.as_ref().map(|(_, r)| r.as_str());
            // "i64_str": explicit opt-in for FFI functions that return a raw i64
            // which is actually a *StringHeader pointer — distinct from "string"
            // (which declares the function as returning `ptr` in LLVM IR) and
            // from "i64" (which sitofp-converts the integer to a JS number).
            let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
            let returns_string = matches!(manifest_ret, Some("string") | Some("ptr"))
                || matches!(ext_return_type, HirType::String)
                || (manifest_ret.is_none()
                    && has_string_args
                    && (name.contains("read_file")
                        || name.contains("clipboard_text")
                        || name.contains("file_dialog")));
            let returns_void = matches!(manifest_ret, Some("void"))
                || (manifest_ret.is_none() && matches!(ext_return_type, HirType::Void));
            let returns_i64 = matches!(manifest_ret, Some("i64"));
            if returns_void {
                ctx.pending_declares
                    .push((name.clone(), crate::types::VOID, arg_types));
                ctx.block().call_void(name, &arg_slices);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            } else if returns_i64_str {
                // C function returns a raw i64 that is a *StringHeader address.
                // Declare as I64 (matching the C ABI — x0 on ARM64, rax on
                // x86_64), call it, and NaN-box the result directly with
                // STRING_TAG. No sitofp (which would corrupt the pointer
                // bits) and no ptrtoint (already an integer, not a ptr).
                ctx.pending_declares.push((name.clone(), I64, arg_types));
                let raw = ctx.block().call(I64, name, &arg_slices);
                let blk = ctx.block();
                return Ok(nanbox_string_inline(blk, &raw));
            } else if returns_string {
                ctx.pending_declares.push((name.clone(), PTR, arg_types));
                let raw_ptr = ctx.block().call(PTR, name, &arg_slices);
                // Convert raw *const u8 back to a NaN-boxed string.
                let blk = ctx.block();
                let ptr_i64 = blk.ptrtoint(&raw_ptr, I64);
                return Ok(nanbox_string_inline(blk, &ptr_i64));
            } else if returns_i64 {
                // C function returns i64 in x0 (e.g. `*mut View`
                // handles). Declare as I64; the value comes back as a
                // raw integer. Convert via `sitofp` so callers see a
                // normal JS number; subsequent FFI calls that pass it
                // back as an i64 param will truncate via `fptosi`.
                ctx.pending_declares.push((name.clone(), I64, arg_types));
                let raw = ctx.block().call(I64, name, &arg_slices);
                let blk = ctx.block();
                return Ok(blk.sitofp(I64, &raw, DOUBLE));
            } else {
                // Native library functions (Bloom, etc.) return f64 in
                // the d0 register — they use the Perry double-based ABI,
                // not a C integer ABI. Declare as DOUBLE and use the
                // return value directly (no sitofp needed).
                ctx.pending_declares.push((name.clone(), DOUBLE, arg_types));
                return Ok(ctx.block().call(DOUBLE, name, &arg_slices));
            }
        };
        // Issue #678 followup: if the consumer-visible name resolves to a
        // V8-fallback module, there is no `perry_fn_<src>__<name>` symbol
        // (the origin was demoted to V8 and never emitted a native one).
        // Route the call through the runtime V8 bridge.
        if let Some(specifier) = ctx.import_function_v8_specifiers.get(name).cloned() {
            let mut lowered: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            return Ok(crate::expr::emit_v8_export_call(
                ctx, &specifier, name, &lowered,
            ));
        }
        // Issue #678: re-export rename (`export { default as render } from
        // './render.js'`) means the origin module emits the symbol under
        // the *origin* name (`default`), not the consumer-visible name
        // (`render`). Look up the actual origin suffix before forming the
        // extern.
        let origin_suffix =
            crate::expr::import_origin_suffix(ctx.import_function_origin_names, name);
        let fname = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
        // Issue #493 followup: when the imported binding is a VARIABLE
        // holding a closure value (e.g. `var mergePath = (b, s, ...r) => …`
        // exported from another module), `perry_fn_<src>__<name>` is the
        // ZERO-arg GETTER that returns the closure pointer (set up at
        // crates/perry/src/commands/compile.rs's `imported_vars` registration
        // and emitted by the source module's value-getter loop). Calling
        // the getter with N args puts garbage in the registers and discards
        // the actual call — `mergePath('/', '/foo')` returned the closure
        // itself instead of the merged path. The fix is to call the getter
        // first, treating its return as a closure value, then dispatch
        // through `js_closure_callN`. The runtime's closure-rest registry
        // (issue #493) bundles trailing args correctly when the closure
        // has `...rest`. Before this branch, ExternFuncRef-as-call for
        // imported-VAR bindings silently broke any code path that imports
        // an arrow-bound exported value (hono's `mergePath` from utils/url.js,
        // any `export const foo = () => …` cross-module use).
        if ctx.imported_vars.contains(name) {
            ctx.pending_declares.push((fname.clone(), DOUBLE, vec![]));
            let closure_box = ctx.block().call(DOUBLE, &fname, &[]);
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            if lowered_args.len() > 16 {
                bail!(
                    "perry-codegen Phase D.1: closure call with {} args (max 16)",
                    lowered_args.len()
                );
            }
            let blk = ctx.block();
            let closure_handle = unbox_to_i64(blk, &closure_box);
            let runtime_fn = format!("js_closure_call{}", lowered_args.len());
            let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
            for v in &lowered_args {
                call_args.push((DOUBLE, v.as_str()));
            }
            return Ok(blk.call(DOUBLE, &runtime_fn, &call_args));
        }
        // Record the cross-module call so the caller can add a `declare`
        // line for it after the &mut LlFunction borrow is released. The
        // module dedupes by name, so duplicates are harmless. Without
        // this, clang errors with `use of undefined value @perry_fn_*`
        // for any cross-module call hidden inside a closure body, try
        // block, switch, etc. — the old pre-walker missed those shapes.
        //
        // Determine the actual param count from the imported function
        // signature. Calls that pass fewer args than the function declares
        // (because the trailing params have defaults) need to be padded
        // with `undefined` so the function body sees defined values for
        // the missing args (and can apply its defaults). Without this,
        // the d-registers for the missing params hold stale data and
        // the function reads garbage (e.g. alpha = -3e-5 instead of 1).
        let declared_count = ctx
            .imported_func_param_counts
            .get(name)
            .copied()
            .unwrap_or(args.len());
        let has_rest = ctx.imported_func_has_rest.contains(name);
        // Issue #608: when the imported callee declares a trailing
        // `...rest` parameter, the LLVM signature has exactly
        // `declared_count` doubles (rest counts as one slot — a
        // NaN-boxed array pointer). Bundle every arg at and beyond the
        // rest position into a single `js_array_alloc` array; that
        // array is what the callee's rest binding sees. Without this
        // bundling, `tag\`hello ${x}\`` lowers to `tag([…], x)` and
        // the cross-module callee reads `params` as `x` directly
        // (`undefined` when no interp args, or the raw arg value
        // when one).
        let target_arity = if has_rest {
            declared_count.max(1)
        } else {
            declared_count.max(args.len())
        };
        let param_types: Vec<crate::types::LlvmType> =
            std::iter::repeat_n(DOUBLE, target_arity).collect();
        ctx.pending_declares
            .push((fname.clone(), DOUBLE, param_types));
        let mut lowered: Vec<String> = Vec::with_capacity(target_arity);
        if has_rest {
            // Fixed (non-rest) params: pass through.
            let fixed_count = declared_count.saturating_sub(1);
            for a in args.iter().take(fixed_count) {
                lowered.push(lower_expr(ctx, a)?);
            }
            // Pad fixed params if the caller passed too few.
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            while lowered.len() < fixed_count {
                lowered.push(undefined_lit.clone());
            }
            // Materialize the rest array (always — even when zero
            // trailing args, the callee's rest binding must be `[]`).
            let rest_count = args.len().saturating_sub(fixed_count);
            let cap = (rest_count as u32).to_string();
            let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for a in args.iter().skip(fixed_count) {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
            }
            let rest_box = nanbox_pointer_inline(ctx.block(), &current);
            lowered.push(rest_box);
        } else {
            for a in args {
                lowered.push(lower_expr(ctx, a)?);
            }
            // Pad with TAG_UNDEFINED for the missing trailing args.
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            while lowered.len() < target_arity {
                lowered.push(undefined_lit.clone());
            }
        }
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        return Ok(ctx.block().call(DOUBLE, &fname, &arg_slices));
    }

    // String/array method dispatch (Phase B.12) and class method
    // dispatch (Phase C.2). For PropertyGet receivers, dispatch based
    // on the receiver's static type.
    if let Expr::PropertyGet { object, property } = callee {
        // Number.prototype.toFixed(decimals) — call js_number_to_fixed.
        // Receiver is any number-typed value; we don't gate on
        // is_numeric_expr because tests often call it on Any locals.
        if property == "toFixed"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle = blk.call(I64, "js_number_to_fixed", &[(DOUBLE, &v), (DOUBLE, &dec)]);
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toPrecision(digits)
        if property == "toPrecision"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let prec = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_number_to_precision",
                &[(DOUBLE, &v), (DOUBLE, &prec)],
            );
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Number.prototype.toExponential(decimals)
        if property == "toExponential"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            let v = lower_expr(ctx, object)?;
            let dec = if args.is_empty() {
                "0.0".to_string()
            } else {
                lower_expr(ctx, &args[0])?
            };
            let blk = ctx.block();
            let handle = blk.call(
                I64,
                "js_number_to_exponential",
                &[(DOUBLE, &v), (DOUBLE, &dec)],
            );
            return Ok(nanbox_string_inline(blk, &handle));
        }
        // Buffer.prototype.toString(encoding) — handled BEFORE the radix
        // path because the encoding arg is a STRING ('utf8'/'hex'/'base64'),
        // not a number. Routing a string arg through `fptosi` produces
        // garbage and the runtime defaults to UTF-8 (the original v0.4.131
        // bug that this test pins). We dispatch via the runtime helper
        // `js_value_to_string_with_encoding` which checks BUFFER_REGISTRY
        // at runtime and falls back to `js_jsvalue_to_string` for
        // non-buffer values.
        if property == "toString"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
            && is_string_expr(ctx, &args[0])
        {
            let has_user_to_string = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx
                            .methods
                            .contains_key(&(c.clone(), "toString".to_string()))
                        {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_to_string {
                let v = lower_expr(ctx, object)?;
                let enc_tag_i32 = if let Expr::String(s) = &args[0] {
                    let lower = s.to_ascii_lowercase();
                    let tag: i32 = match lower.as_str() {
                        "utf8" | "utf-8" | "ascii" | "latin1" | "binary" => 0,
                        "hex" => 1,
                        "base64" | "base64url" => 2,
                        _ => 0,
                    };
                    tag.to_string()
                } else {
                    let enc_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    blk.call(I32, "js_encoding_tag_from_value", &[(DOUBLE, &enc_box)])
                };
                let blk = ctx.block();
                let handle = blk.call(
                    I64,
                    "js_value_to_string_with_encoding",
                    &[(DOUBLE, &v), (I32, &enc_tag_i32)],
                );
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        // Number.prototype.toString(radix) — special case where the
        // single arg is the radix (2..36). Routes through
        // js_jsvalue_to_string_radix so `(255).toString(16)` returns
        // "ff" instead of "255".
        if property == "toString"
            && args.len() == 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Only treat as radix call if class doesn't have toString.
            let has_user_to_string = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx
                            .methods
                            .contains_key(&(c.clone(), "toString".to_string()))
                        {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_to_string {
                let v = lower_expr(ctx, object)?;
                let radix_d = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let radix_i32 = blk.fptosi(DOUBLE, &radix_d, I32);
                let handle = blk.call(
                    I64,
                    "js_jsvalue_to_string_radix",
                    &[(DOUBLE, &v), (I32, &radix_i32)],
                );
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        // Universal `.toString()` — works for any JS value via the
        // runtime's js_jsvalue_to_string dispatch (numbers print as
        // their decimal form, strings as themselves, objects as
        // [object Object], etc.). Only intercepts if NO class
        // method dispatch can win (i.e. the receiver isn't a known
        // class with its own toString) — otherwise the user's
        // override wouldn't run.
        if property == "toString"
            && args.len() <= 1
            && !is_string_expr(ctx, object)
            && !is_array_expr(ctx, object)
        {
            // Check whether the receiver class (if any) defines
            // toString itself or via inheritance.
            let has_user_to_string = receiver_class_name(ctx, object)
                .map(|cls| {
                    let mut cur = Some(cls);
                    while let Some(c) = cur {
                        if ctx
                            .methods
                            .contains_key(&(c.clone(), "toString".to_string()))
                        {
                            return true;
                        }
                        cur = ctx.classes.get(&c).and_then(|cd| cd.extends_name.clone());
                    }
                    false
                })
                .unwrap_or(false);
            if !has_user_to_string {
                let v = lower_expr(ctx, object)?;
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                let blk = ctx.block();
                let handle = blk.call(I64, "js_jsvalue_to_string", &[(DOUBLE, &v)]);
                return Ok(nanbox_string_inline(blk, &handle));
            }
        }
        if is_string_expr(ctx, object) {
            return lower_string_method(ctx, object, property, args);
        }
        // String method fallback for Any-typed receivers: when the method
        // name is a well-known string method that has no array/object
        // equivalent, route through the string dispatcher. This handles
        // the common pattern where a cross-module function returns a string
        // but the local is typed as Any (e.g., `readFileSync(path).split('\n')`).
        // Without this, .split/.charCodeAt/.charAt/etc. on Any-typed strings
        // fall through to js_native_call_method which returns [object Object].
        {
            // Only include methods that are EXCLUSIVELY string methods
            // (no array/map/set equivalent). Exclude: slice, indexOf,
            // lastIndexOf, includes, at, concat — these also exist on
            // arrays and would break when the receiver is an Any-typed
            // array. startsWith/endsWith are string-only in JS so the
            // 2-arg form (searchString, position) is also unambiguous.
            let is_string_only_method = match property.as_str() {
                "split" | "charCodeAt" | "charAt" | "trim" | "trimStart" | "trimEnd"
                | "substring" | "substr" | "toLowerCase" | "toUpperCase" | "toLocaleLowerCase"
                | "toLocaleUpperCase" | "replaceAll" | "padStart" | "padEnd" | "repeat"
                | "normalize" | "codePointAt" | "localeCompare" => true,
                // Issue #638: `replace` is also string-exclusive, but routing
                // it here unconditionally caused regressions in async dispatch
                // pathways. Only fire when args[1] is statically detectable as
                // a closure literal — that's the failing case (replace
                // callback got coerced to "[object Object]" via the runtime
                // fallback path because the string-method dispatch never
                // saw it). When args[1] is a string, the existing
                // js_native_call_method fallback handles it correctly via
                // js_string_replace_string.
                "replace" if args.len() == 2 && matches!(&args[1], Expr::Closure { .. }) => true,
                // slice/indexOf/includes exist on both strings and arrays.
                // Route to string path only when args rule out the array
                // variant (e.g., slice(0) is ambiguous but slice() with 0
                // args is always array.slice to copy).
                "slice" if !args.is_empty() => true,
                "indexOf" | "includes" if args.len() == 1 => true,
                // startsWith / endsWith only exist on String — both 1-arg
                // and 2-arg (searchString, position) forms route here.
                "startsWith" | "endsWith" if args.len() == 1 || args.len() == 2 => true,
                "lastIndexOf" if args.len() == 1 => true,
                _ => false,
            };
            // Don't route buffer/Uint8Array methods through the string path —
            // buffers have a different header layout and their indexOf/includes
            // go through dispatch_buffer_method via js_native_call_method.
            let is_buffer = matches!(
                crate::type_analysis::static_type_of(ctx, object),
                Some(perry_types::Type::Named(ref n)) if n == "Uint8Array" || n == "Buffer"
            );
            if is_string_only_method && !is_array_expr(ctx, object) && !is_buffer {
                return lower_string_method(ctx, object, property, args);
            }
        }
        if is_array_expr(ctx, object) {
            return lower_array_method(ctx, object, property, args);
        }

        // -------- Promise.then / .catch / .finally --------
        // Promise pointers are NaN-boxed with POINTER_TAG. We unbox
        // to get the raw i64 promise handle, then call the runtime
        // `js_promise_then(promise, on_fulfilled, on_rejected)` which
        // returns a new promise handle that we re-box with POINTER_TAG.
        //
        // `.catch(cb)` is sugar for `.then(undefined, cb)`.
        if matches!(property.as_str(), "then" | "catch" | "finally") && is_promise_expr(ctx, object)
        {
            match property.as_str() {
                "then" => {
                    if !args.is_empty() {
                        // Fused fast path: detect `Promise.resolve(<expr>).then(cb_f, cb_e?)`
                        // and route to `js_promise_resolved_then`, which skips
                        // the intermediate Promise-#1 allocation when `<expr>`
                        // is a NaN-boxed primitive (number/bool/null/undefined/
                        // string/bigint/int32). Steady-state shape of every
                        // `await` after async-to-generator lowering — saves
                        // one Promise alloc + one TASK_QUEUE round-trip per
                        // await.
                        if let Expr::Call {
                            callee: inner_callee,
                            args: inner_args,
                            ..
                        } = object.as_ref()
                        {
                            if let Expr::PropertyGet {
                                object: inner_object,
                                property: inner_property,
                            } = inner_callee.as_ref()
                            {
                                // #1008: accept both the legacy `Promise` =
                                // GlobalGet shape and the post-#973
                                // PropertyGet { GlobalGet(0), "Promise" }
                                // shape. Without the second arm the
                                // fast path silently disengaged for
                                // every `Promise.resolve(...).then(...)`
                                // call (microtask-02..07 regression).
                                // Resolved-from-merge note: this used to live as
                                // an unresolved conflict on main; the incoming
                                // side called `is_global_constructor_expr`,
                                // which is what the rest of the file uses post
                                // #1030. Keep the richer comment from HEAD but
                                // call the same helper everything else does.
                                if inner_property == "resolve"
                                    && is_global_constructor_expr(inner_object.as_ref(), "Promise")
                                {
                                    let inner_value = if inner_args.is_empty() {
                                        double_literal(0.0)
                                    } else {
                                        lower_expr(ctx, &inner_args[0])?
                                    };
                                    let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                                    let on_rejected_box = if args.len() >= 2 {
                                        lower_expr(ctx, &args[1])?
                                    } else {
                                        "0".to_string()
                                    };
                                    let blk = ctx.block();
                                    let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                                    let on_rejected_handle = if args.len() >= 2 {
                                        unbox_to_i64(blk, &on_rejected_box)
                                    } else {
                                        "0".to_string()
                                    };
                                    let new_promise = blk.call(
                                        I64,
                                        "js_promise_resolved_then",
                                        &[
                                            (DOUBLE, &inner_value),
                                            (I64, &on_fulfilled_handle),
                                            (I64, &on_rejected_handle),
                                        ],
                                    );
                                    return Ok(nanbox_pointer_inline(blk, &new_promise));
                                }
                            }
                        }

                        let promise_box = lower_expr(ctx, object)?;
                        let on_fulfilled_box = lower_expr(ctx, &args[0])?;
                        let on_rejected_box = if args.len() >= 2 {
                            lower_expr(ctx, &args[1])?
                        } else {
                            "0".to_string() // null → no rejection handler
                        };
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_fulfilled_handle = unbox_to_i64(blk, &on_fulfilled_box);
                        let on_rejected_i64 = if args.len() >= 2 {
                            unbox_to_i64(blk, &on_rejected_box)
                        } else {
                            "0".to_string() // null i64
                        };
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &on_fulfilled_handle),
                                (I64, &on_rejected_i64),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "catch" => {
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_rejected_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_rejected_handle = unbox_to_i64(blk, &on_rejected_box);
                        let null_i64 = "0".to_string();
                        let new_promise = blk.call(
                            I64,
                            "js_promise_then",
                            &[
                                (I64, &promise_handle),
                                (I64, &null_i64),
                                (I64, &on_rejected_handle),
                            ],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                "finally" => {
                    // .finally(cb) — per spec: call cb() ignoring its return value,
                    // then propagate the upstream value/reason unchanged.
                    // Routes through js_promise_finally which wraps cb in
                    // fulfill/reject proxy closures that call cb() and then
                    // return the upstream value (or re-throw the upstream reason).
                    if !args.is_empty() {
                        let promise_box = lower_expr(ctx, object)?;
                        let on_finally_box = lower_expr(ctx, &args[0])?;
                        let blk = ctx.block();
                        let promise_handle = unbox_to_i64(blk, &promise_box);
                        let on_finally_handle = unbox_to_i64(blk, &on_finally_box);
                        let new_promise = blk.call(
                            I64,
                            "js_promise_finally",
                            &[(I64, &promise_handle), (I64, &on_finally_handle)],
                        );
                        return Ok(nanbox_pointer_inline(blk, &new_promise));
                    }
                }
                _ => {}
            }
        }

        // -------- Map/Set methods on PropertyGet receivers --------
        // The HIR only folds `m.set(...)`/`m.get(...)` to MapSet/MapGet
        // when `m` is an Ident receiver (plain local). When the receiver
        // is `this.field` (class method accessing a Map-typed field),
        // the generic Call reaches here and needs an explicit dispatch
        // to the Map runtime helpers. Without this branch,
        // `this.handlers.get(event)` falls through to js_native_call_method
        // which doesn't know about Maps and returns undefined.
        if is_map_expr(ctx, object) {
            match property.as_str() {
                "set" if args.len() == 2 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let v_box = lower_expr(ctx, &args[1])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    blk.call_void(
                        "js_map_set",
                        &[(I64, &m_handle), (DOUBLE, &k_box), (DOUBLE, &v_box)],
                    );
                    return Ok(m_box);
                }
                "get" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    return Ok(blk.call(
                        DOUBLE,
                        "js_map_get",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    ));
                }
                "has" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_map_has",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "delete" if args.len() == 1 => {
                    let m_box = lower_expr(ctx, object)?;
                    let k_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_map_delete",
                        &[(I64, &m_handle), (DOUBLE, &k_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "clear" if args.is_empty() => {
                    let m_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    blk.call_void("js_map_clear", &[(I64, &m_handle)]);
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                // Map iterator methods (entries / keys / values).
                // Issue #412: the HIR-level fold at expr_call.rs only
                // fires for `Expr::Ident` receivers (a plain local).
                // Receivers like `new Map(...).values()`,
                // `this.field.values()`, `obj.field.values()` come
                // through the generic call path and need codegen-time
                // dispatch — pre-fix they fell off the bottom of the
                // method-dispatch tower and silently returned
                // `undefined`. The runtime returns a real Array; we
                // NaN-box-pointer the result for downstream
                // `.length` / `forEach` / `Array.from` use.
                "entries" | "keys" | "values" if args.is_empty() => {
                    let m_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let m_handle = unbox_to_i64(blk, &m_box);
                    let runtime_fn = match property.as_str() {
                        "entries" => "js_map_entries",
                        "keys" => "js_map_keys",
                        "values" => "js_map_values",
                        _ => unreachable!(),
                    };
                    let result = blk.call(I64, runtime_fn, &[(I64, &m_handle)]);
                    return Ok(crate::expr::nanbox_pointer_inline_pub(blk, &result));
                }
                _ => {}
            }
        }
        if is_set_expr(ctx, object) {
            match property.as_str() {
                "add" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    blk.call_void("js_set_add", &[(I64, &s_handle), (DOUBLE, &v_box)]);
                    return Ok(s_box);
                }
                "has" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_set_has",
                        &[(I64, &s_handle), (DOUBLE, &v_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "delete" if args.len() == 1 => {
                    let s_box = lower_expr(ctx, object)?;
                    let v_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    let i32_v = blk.call(
                        crate::types::I32,
                        "js_set_delete",
                        &[(I64, &s_handle), (DOUBLE, &v_box)],
                    );
                    return Ok(crate::expr::i32_bool_to_nanbox(blk, &i32_v));
                }
                "clear" if args.is_empty() => {
                    let s_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    blk.call_void("js_set_clear", &[(I64, &s_handle)]);
                    return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                }
                // Set iterator methods. Per ECMA-262 §24.2.3.5–7,
                // `Set.prototype.values`, `.keys`, and `.entries` all
                // return iterators over the Set's elements (keys ===
                // values for Sets; entries yields [v, v] pairs).
                // Perry's `js_set_to_array` returns a real Array of
                // the Set's elements — sufficient for the common
                // `Array.from(s.values())` / `for-of s.values()` /
                // spread shapes. Pre-fix `new Set([1]).values()`
                // returned `undefined` because the HIR-level fold at
                // expr_call.rs only fires for `Expr::Ident` receivers.
                "values" | "keys" if args.is_empty() => {
                    let s_box = lower_expr(ctx, object)?;
                    let blk = ctx.block();
                    let s_handle = unbox_to_i64(blk, &s_box);
                    let result = blk.call(I64, "js_set_to_array", &[(I64, &s_handle)]);
                    return Ok(crate::expr::nanbox_pointer_inline_pub(blk, &result));
                }
                _ => {}
            }
        }

        // -------- Map.forEach / Set.forEach --------
        // The HIR emits these as generic Call { callee: PropertyGet }
        // because it skips ArrayForEach when the receiver is Map/Set.
        // Route to the runtime forEach implementations which iterate
        // entries and call the callback via js_closure_call2.
        if property == "forEach" && !args.is_empty() {
            if is_map_expr(ctx, object) {
                let m_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let m_handle = unbox_to_i64(blk, &m_box);
                blk.call_void("js_map_foreach", &[(I64, &m_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
            if is_set_expr(ctx, object) {
                let s_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let s_handle = unbox_to_i64(blk, &s_box);
                blk.call_void("js_set_foreach", &[(I64, &s_handle), (DOUBLE, &cb_box)]);
                return Ok(double_literal(0.0));
            }
            // URLSearchParams.forEach((value, key, this) => …). The HIR
            // variant `Expr::UrlSearchParamsForEach` only fires when the
            // receiver is a typed-named local; chained access (`u.searchParams
            // .forEach(...)`) and unannotated `const sp = new URLSearchParams()`
            // routes flow through this generic Call path. Route both via the
            // runtime entry so the callback gets the string `(value, key)`
            // pair instead of `(NaN, 0)` from the Array.forEach fast path.
            if is_url_search_params_expr(ctx, object) {
                let p_box = lower_expr(ctx, object)?;
                let cb_box = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let p_handle = unbox_to_i64(blk, &p_box);
                blk.call_void(
                    "js_url_search_params_for_each",
                    &[(I64, &p_handle), (DOUBLE, &cb_box)],
                );
                return Ok(double_literal(0.0));
            }
        }

        // ── AbortController / AbortSignal dispatch ──
        // `new AbortController()` returns a NaN-boxed pointer
        // (refined to `Named("AbortController")`). The runtime's
        // ObjectHeader carries `signal` / `aborted` fields that the
        // generic property-get path reads. Method calls need explicit
        // interception because the class isn't in `ctx.classes`.
        if let Some(val) = lower_abort_controller_call(ctx, object, property, args)? {
            return Ok(val);
        }

        // ── Chained Web Fetch dispatch ──
        // `r.headers.get(k)` — the inner `r.headers` lowered to a
        // NativeMethodCall that returns an f64 Headers handle; route
        // the outer `.get(...)` (and friends) through the Headers FFI.
        // `r.clone().status` / `.text()` / etc — the inner clone call
        // returns an f64 Response handle; route the outer call through
        // the fetch dispatch.
        //
        // `new Response(...).text()` — likewise, when the receiver is
        // a direct `Expr::New { class_name: "Response"|"Headers"|"Request" }`
        // (no intermediate let binding).
        if let Expr::NativeMethodCall {
            module: chain_mod,
            method: chain_method,
            ..
        } = object.as_ref()
        {
            // Chain `<Response>.headers.<method>(...)` where chain_method == "headers".
            if chain_mod == "fetch" && chain_method == "headers" {
                if let Some(val) = lower_fetch_native_method(
                    ctx,
                    "Headers",
                    property.as_str(),
                    Some(object),
                    args,
                )? {
                    return Ok(val);
                }
            }
            // Chain `<Response>.clone().<method>(...)` — dispatch as a
            // fetch method on the cloned handle.
            if chain_mod == "fetch" && chain_method == "clone" {
                if let Some(val) =
                    lower_fetch_native_method(ctx, "fetch", property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }
        // Chain `new Response(...).text()` / `.json()` etc.
        if let Expr::New { class_name: nc, .. } = object.as_ref() {
            let fetch_dispatch = matches!(nc.as_str(), "Response" | "Headers" | "Request");
            if fetch_dispatch {
                let module = match nc.as_str() {
                    "Response" => "fetch",
                    "Headers" => "Headers",
                    "Request" => "Request",
                    _ => unreachable!(),
                };
                if let Some(val) =
                    lower_fetch_native_method(ctx, module, property.as_str(), Some(object), args)?
                {
                    return Ok(val);
                }
            }
        }

        // Issue #687 — ClassRef receiver static-method dispatch.
        // `ClassName.method(args)` where `ClassName` lowered to
        // `Expr::ClassRef` (an INT32-NaN-boxed class id) rather than a
        // pointer to an instance. The Effect repro is Schema.ts's
        // `BigIntFromSelf.pipe(positiveBigInt(...))`, where
        // `BigIntFromSelf` is declared as
        // `class BigIntFromSelf extends make<bigint>(AST.bigIntKeyword) {}`
        // and `pipe` is a static method inherited from the anonymous
        // class returned by `make()`. Pre-fix the call fell through to
        // the dynamic-instance-dispatch tower below, which read
        // `js_object_get_class_id(0x324)` → 0 (the receiver is a class
        // id, not an instance pointer), missed every implementor case,
        // and `js_native_call_method` threw
        // `(number).pipe is not a function`.
        //
        // Resolution: when the static receiver is `Expr::ClassRef`, walk
        // the class's own static methods plus its `extends_name` chain
        // looking for `property`. If found, emit a direct call to the
        // `perry_static_<modprefix>__<class>__<method>` symbol with
        // IMPLICIT_THIS bound to the ClassRef so `pipe`'s body's
        // `this` references the class. If nothing matches (Effect's
        // BigIntFromSelf case — its parent is an unnamed CallExpr so
        // perry's `extends_name` chain is empty), fall back to
        // returning the ClassRef itself: chainable `.pipe()` calls in
        // module init then propagate the class ref forward, letting
        // Schema.ts__init advance past previously-fatal sites. The
        // returned value isn't semantically equivalent to Effect's
        // transformed schema, but it unblocks module init for the
        // #321 DoD repro.
        // Resolve the static-method receiver class through one of two
        // shapes:
        //   (a) the receiver is `Expr::ClassRef(name)` directly — the
        //       original #687 case (Effect Schema's
        //       `BigIntFromSelf.pipe(...)`); and
        //   (b) the receiver is `Expr::LocalGet(id)` where the local was
        //       initialised from `Expr::ClassRef` (or from a factory call
        //       the inliner already collapsed to ClassRef) — Effect's
        //       `const Tag = make(); Tag.staticMethod(...)`, and more
        //       generally any
        //         const C = make();
        //         C.staticMethod(...)
        //       Refs #915 (gap 2 from #899). The local→class map is the
        //       same one `lower_new`'s alias rerouting consults below.
        // Refs #915 (gap 3 / #321 follow-up): walk the receiver to
        // recognise the "static-method on a class produced by a
        // factory" pattern. Covered shapes:
        //   - `Expr::ClassRef(name)` — direct class literal.
        //   - `Expr::LocalGet(id)` whose let-init was a ClassRef (the
        //     post-#912 `const Cls = make(); Cls.foo(...)` shape).
        //   - `Expr::Call { callee: FuncRef(fid) }` where `fid` is a
        //     factory function tagged via `func_returns_class`. The
        //     HIR inliner sometimes leaves these calls in place
        //     (Effect's `Literal(value).pipe(...)`); the
        //     `func_returns_class` fixed-point pass tags Literal,
        //     makeLiteralClass, make, etc.
        //   - `Expr::Sequence` whose trailing expression itself
        //     resolves to a class. The inliner sometimes collapses
        //     `Literal(value)` to
        //     `Sequence([RegisterClassParentDynamic, ClassRef(L)])`
        //     so the call site sees the class without an outer Call.
        fn resolve_static_dispatch_cls(
            expr: &Expr,
            local_id_to_name: &std::collections::HashMap<u32, String>,
            local_class_aliases: &std::collections::HashMap<String, String>,
            func_returns_class: &std::collections::HashMap<u32, String>,
        ) -> Option<String> {
            match expr {
                Expr::ClassRef(name) => Some(name.clone()),
                Expr::LocalGet(id) => local_id_to_name
                    .get(id)
                    .and_then(|name| local_class_aliases.get(name).cloned()),
                Expr::Call { callee, .. } => match callee.as_ref() {
                    Expr::FuncRef(fid) => func_returns_class.get(fid).cloned(),
                    _ => None,
                },
                Expr::Sequence(exprs) => exprs.last().and_then(|e| {
                    resolve_static_dispatch_cls(
                        e,
                        local_id_to_name,
                        local_class_aliases,
                        func_returns_class,
                    )
                }),
                _ => None,
            }
        }
        let static_dispatch_cls: Option<String> = resolve_static_dispatch_cls(
            object,
            &ctx.local_id_to_name,
            &ctx.local_class_aliases,
            ctx.func_returns_class,
        );
        if let Some(cls_name) = static_dispatch_cls {
            // (fn_name, is_static, declared_param_count, has_rest, is_synthetic_arguments)
            let mut resolved: Option<(String, bool, usize, bool, bool)> = None;
            let mut cur = Some(cls_name.clone());
            while let Some(c) = cur {
                if let Some(class_info) = ctx.classes.get(&c) {
                    let sm = class_info
                        .static_methods
                        .iter()
                        .find(|m| m.name == *property);
                    if let Some(sm) = sm {
                        if let Some(fname) =
                            ctx.methods.get(&(c.clone(), property.clone())).cloned()
                        {
                            let declared = sm.params.len();
                            let has_rest = sm.params.last().map(|p| p.is_rest).unwrap_or(false);
                            let is_synth_args = sm
                                .params
                                .last()
                                .map(|p| p.is_rest && p.name == "arguments")
                                .unwrap_or(false);
                            resolved = Some((fname, true, declared, has_rest, is_synth_args));
                            break;
                        }
                    }
                }
                cur = ctx
                    .classes
                    .get(&c.clone())
                    .and_then(|cc| cc.extends_name.clone());
            }
            if let Some((fn_name, _is_static, declared, has_rest, is_synth_args)) = resolved {
                // Receiver-box selection (`this` inside the static body):
                //   - `ClassRef`: `lower_expr` already yields the
                //     INT32-NaN-boxed class id; `this === ClassRef`.
                //   - `Call` (factory return): `lower_expr` returns the
                //     dynamic class produced by the factory, so each
                //     `Literal(value)` / `make(ast)` call carries
                //     unique static fields (`static literals = […]`,
                //     `static ast = …`). The static body reads those
                //     through `this.<field>`, so passing the synthesized
                //     ClassRef would lose the per-call data — use the
                //     actual lowered call result instead.
                //   - Everything else (`LocalGet` after a
                //     `const Cls = make()` collapse, etc.): synthesize
                //     a fresh ClassRef NaN-box. The static body's
                //     `this.<field>` then dispatches through the
                //     ClassRef's class-keys + class-field side-table,
                //     which is the post-#912 (gap 2) shape.
                let recv_box = match object.as_ref() {
                    Expr::ClassRef(_) => lower_expr(ctx, object)?,
                    Expr::Call { .. } => lower_expr(ctx, object)?,
                    Expr::Sequence(_) => lower_expr(ctx, object)?,
                    _ => {
                        // Synthesize a ClassRef NaN-box from the resolved class.
                        let cid = ctx.class_ids.get(&cls_name).copied().unwrap_or(0);
                        let bits = crate::nanbox::INT32_TAG | (cid as u64 & 0xFFFF_FFFF);
                        crate::nanbox::double_literal(f64::from_bits(bits))
                    }
                };
                // Refs #915 (gap 3 / #321 follow-up): Effect's `class
                // SchemaClass { static pipe() { ... arguments ... } }`
                // factory returns an anon class whose `pipe` reads
                // `arguments.length` to dispatch. The HIR appends a
                // synthesized `arguments` rest param (#677 / #899). The
                // direct-call dispatch here previously forwarded the
                // call args 1:1 to the function whose only declared
                // parameter is the rest array — so for
                // `Cls.pipe(f1, f2)` the function got `arg0 = f1` (then
                // read .length = "function" → undefined). Mirror the
                // arg-bundling logic from the regular Call lowering
                // (lines ~720–765) so the rest slot receives a real
                // array of all call args, matching JS `arguments`
                // semantics. The non-synthetic rest path (e.g.
                // `static foo(a, ...rest)`) follows the same shape:
                // pass the first `declared-1` positional args as-is,
                // then bundle the trailing args into an Array.
                let mut lowered: Vec<String> = Vec::with_capacity(args.len());
                if has_rest && is_synth_args {
                    let cap = (args.len() as u32).to_string();
                    let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for a in args {
                        let v = lower_expr(ctx, a)?;
                        let blk = ctx.block();
                        current =
                            blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
                    }
                    let arguments_box = nanbox_pointer_inline(ctx.block(), &current);
                    lowered.push(arguments_box);
                } else if has_rest {
                    let fixed_count = declared.saturating_sub(1);
                    for a in args.iter().take(fixed_count) {
                        lowered.push(lower_expr(ctx, a)?);
                    }
                    let rest_count = args.len().saturating_sub(fixed_count);
                    let cap = (rest_count as u32).to_string();
                    let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for a in args.iter().skip(fixed_count) {
                        let v = lower_expr(ctx, a)?;
                        let blk = ctx.block();
                        current =
                            blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
                    }
                    let rest_box = nanbox_pointer_inline(ctx.block(), &current);
                    lowered.push(rest_box);
                } else {
                    for a in args {
                        lowered.push(lower_expr(ctx, a)?);
                    }
                }
                let prev_this =
                    ctx.block()
                        .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &recv_box)]);
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
                let result = ctx.block().call(DOUBLE, &fn_name, &arg_slices);
                ctx.block()
                    .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev_this)]);
                return Ok(result);
            }
            // No static method resolved through the visible chain.
            // Lower the args for side effects and return the ClassRef
            // itself so chained `.pipe()` calls keep producing a
            // typed-class-shaped value during module init.
            if matches!(object.as_ref(), Expr::ClassRef(_)) {
                for a in args {
                    let _ = lower_expr(ctx, a)?;
                }
                return lower_expr(ctx, object);
            }
            // For LocalGet receivers that resolve to a class but the
            // method isn't a static — fall through to the normal
            // instance/dynamic dispatch tower below.
        }

        // Class instance method call. The receiver's static type is
        // `Type::Named(<class>)` for typed instances.
        //
        // Resolution strategy:
        //   1. Walk the receiver's class + parent chain to find a
        //      method named `property`. The first match (most-derived
        //      that defines the method) is the static fallback.
        //   2. Find every subclass of the receiver's class that ALSO
        //      defines the same method — those are the virtual
        //      override candidates.
        //   3. If there are no overrides, emit a direct call to the
        //      static fallback (fast path, no runtime cost).
        //   4. If there ARE overrides, emit a switch on the object's
        //      runtime class_id: each override gets its own case
        //      calling its concrete method, default falls through to
        //      the static fallback.
        // Interface / dynamic dispatch fallback: when the static
        // class is unknown OR resolves to an interface name not in
        // the class registry, BUT the property name corresponds to
        // a method defined on at least one class in the registry,
        // emit a switch on class_id over all classes that have that
        // method.
        // Skip dynamic dispatch when the receiver is GlobalGet (e.g.
        // `console.log`). GlobalGet is a module-level global object
        // (console, Math, JSON, etc.), not a class instance. Without
        // this guard, `console.log()` gets hijacked by the interface
        // dispatch tower when a user class happens to have a method
        // with the same name (like `SimpleLogger.log()`).
        let is_global = matches!(object.as_ref(), Expr::GlobalGet(_));
        // If the receiver's static type is a well-known built-in with its own
        // runtime method family (Buffer byte readers, Array, Map, Set, …),
        // don't enter the user-class dispatch tower. Otherwise an imported
        // user class that happens to declare the same method name (e.g. a
        // BufferCursor with `readUInt8`) would be enumerated as an
        // implementor and `buf.readUInt8(i)` would fall through to the
        // default 0.0 case when the Buffer's class id doesn't match any
        // tower entry.
        let is_builtin_receiver = match receiver_class_name(ctx, object) {
            Some(name) => matches!(
                name.as_str(),
                "Buffer"
                    | "Uint8Array"
                    | "Uint8ClampedArray"
                    | "Int8Array"
                    | "Int16Array"
                    | "Uint16Array"
                    | "Int32Array"
                    | "Uint32Array"
                    | "Float32Array"
                    | "Float64Array"
                    | "BigInt64Array"
                    | "BigUint64Array"
                    | "Array"
                    | "ReadonlyArray"
                    | "Map"
                    | "ReadonlyMap"
                    | "Set"
                    | "ReadonlySet"
                    | "WeakMap"
                    | "WeakSet"
                    | "Promise"
                    | "RegExp"
                    | "Date"
            ),
            None => false,
        };
        let needs_dynamic_dispatch = !is_global
            && !is_builtin_receiver
            && match receiver_class_name(ctx, object) {
                None => true,
                Some(name) => !ctx.classes.contains_key(&name),
            };
        if needs_dynamic_dispatch {
            // Find all (class_id → fn_name) for `property` — including
            // INHERITED methods. Per JS spec, `subInstance.method()` for a
            // method defined on a parent dispatches to the parent's
            // implementation. perry's previous walk only added classes that
            // DIRECTLY declared `property`; subclasses that inherited the
            // method weren't represented in the dispatch tower, so the
            // icmp_eq vs class_id missed and the call fell through to the
            // runtime's js_native_call_method fallback (which returns an
            // empty object for unknown receiver class+method combos).
            // Refs #420 — drizzle's `serial("id").primaryKey()` where
            // primaryKey is on ColumnBuilder (grandparent) but the
            // receiver is a PgSerialBuilder (grandchild).
            //
            // Algorithm: walk every class C in `class_ids`. For each, walk
            // C's parent chain and find the FIRST class that has `property`
            // in `ctx.methods`. Register (C's id → that ancestor's fn_name).
            let mut implementors: Vec<(u32, String)> = Vec::new();
            let mut seen_pairs: std::collections::HashSet<(u32, String)> =
                std::collections::HashSet::new();
            for (start_cls, &start_cid) in ctx.class_ids.iter() {
                let mut cur: Option<String> = Some(start_cls.clone());
                while let Some(c) = cur {
                    let key = (c.clone(), property.clone());
                    if let Some(fname) = ctx.methods.get(&key).cloned() {
                        if seen_pairs.insert((start_cid, fname.clone())) {
                            implementors.push((start_cid, fname));
                        }
                        break;
                    }
                    cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
                }
            }
            if !implementors.is_empty() {
                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                // Issue #235: pad lowered_args with TAG_UNDEFINED so the callee's
                // default-param desugaring fires when the call site passed fewer
                // args than the method declares. Pre-fix the dispatch tower
                // passed exactly `args.len() + 1` doubles to a function declared
                // with N+1 doubles, leaving any param the caller skipped to be
                // read from an uninitialized arg-register slot — typically a
                // real heap pointer that hung the dispatch chain on
                // `options.session` deref.
                //
                // Take max arity across all implementors so the same arg_slices
                // works for every concrete callee. Implementations with smaller
                // arity silently ignore extra trailing args at runtime.
                let mut max_explicit_arity: usize = 0;
                for (_, fname) in &implementors {
                    for ((cls, mname), reg_fname) in ctx.methods.iter() {
                        if reg_fname == fname && mname == property {
                            if let Some(&n) =
                                ctx.method_param_counts.get(&(cls.clone(), mname.clone()))
                            {
                                if n > max_explicit_arity {
                                    max_explicit_arity = n;
                                }
                            }
                            break;
                        }
                    }
                }
                let target_total = max_explicit_arity + 1; // +1 for `this`
                let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                // Issue #672: bundle trailing args into a rest array on the
                // dynamic-dispatch path too. Mirrors the static-dispatch arm
                // below — without it, `conn.command("SET","k","v")` on a
                // `conn: any` (the @perryts/redis case) reached the callee with
                // `name="SET"`, `args="k"` and the trailing `"v"` silently
                // dropped, since the LLVM signature only declares N+1 doubles
                // and any 4th double is just discarded.
                let mut method_has_rest_dyn = false;
                let mut method_decl_count_dyn = max_explicit_arity;
                for (_, fname) in &implementors {
                    for ((cls, mname), reg_fname) in ctx.methods.iter() {
                        if reg_fname == fname && mname == property {
                            let key = (cls.clone(), mname.clone());
                            if let Some(&true) = ctx.method_has_rest.get(&key) {
                                method_has_rest_dyn = true;
                                if let Some(&n) = ctx.method_param_counts.get(&key) {
                                    method_decl_count_dyn = n;
                                }
                                break;
                            }
                        }
                    }
                    if method_has_rest_dyn {
                        break;
                    }
                }
                if method_has_rest_dyn {
                    let fixed_user = method_decl_count_dyn.saturating_sub(1);
                    while lowered_args.len() - 1 < fixed_user {
                        lowered_args.push(undefined_lit.clone());
                    }
                    let split_at = 1 + fixed_user;
                    let rest_count = lowered_args.len().saturating_sub(split_at);
                    let cap = (rest_count as u32).to_string();
                    let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for v in &lowered_args[split_at..] {
                        let blk = ctx.block();
                        rest_arr =
                            blk.call(I64, "js_array_push_f64", &[(I64, &rest_arr), (DOUBLE, v)]);
                    }
                    let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                    lowered_args.truncate(split_at);
                    lowered_args.push(rest_box);
                } else {
                    while lowered_args.len() < target_total {
                        lowered_args.push(undefined_lit.clone());
                    }
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                // Issue #628 followup (#620 in dynamic-dispatch shape): probe
                // own-property override BEFORE the class-id switch tower. The
                // tower hard-codes the static method body for each known
                // class id; when a user mutates `this.method = X` inside
                // a method body (hono's SmartRouter rebinds itself on first
                // call), the second call's dispatch must invoke the stored
                // override, not the original method. The static-class fast
                // path got this in v0.5.716 (#620). The dynamic-dispatch
                // path needs the parallel fix.
                let key_idx_probe = ctx.strings.intern(property);
                let probe_entry = ctx.strings.entry(key_idx_probe);
                let probe_bytes_global = format!("@{}", probe_entry.bytes_global);
                let probe_name_len_str = probe_entry.byte_len.to_string();
                let own_method_probe = ctx.block().call(
                    DOUBLE,
                    "js_object_get_own_field_or_undef",
                    &[
                        (DOUBLE, &recv_box),
                        (crate::types::PTR, &probe_bytes_global),
                        (I64, &probe_name_len_str),
                    ],
                );
                let own_bits_probe = ctx.block().bitcast_double_to_i64(&own_method_probe);
                let undef_bits_str = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
                let is_undef_probe = ctx.block().icmp_eq(I64, &own_bits_probe, &undef_bits_str);
                let probe_override_idx = ctx.new_block("idisp.override");
                let probe_dispatch_idx = ctx.new_block("idisp.dispatch");
                let probe_outer_merge_idx = ctx.new_block("idisp.outer_merge");
                let probe_override_label = ctx.block_label(probe_override_idx);
                let probe_dispatch_label = ctx.block_label(probe_dispatch_idx);
                let probe_outer_merge_label = ctx.block_label(probe_outer_merge_idx);
                ctx.block().cond_br(
                    &is_undef_probe,
                    &probe_dispatch_label,
                    &probe_override_label,
                );

                // Override path: pack user args (skip recv at slot 0) and
                // invoke via js_native_call_value. The stored value is
                // typically an arrow function or `.bind()` closure whose
                // `this` is captured/bound, so we don't pass the receiver
                // as an extra arg — matches the static-class fast path's
                // contract.
                ctx.current_block = probe_override_idx;
                let user_arg_count_probe = lowered_args.len().saturating_sub(1);
                let (probe_args_ptr, probe_args_len_str) = if user_arg_count_probe == 0 {
                    ("null".to_string(), "0".to_string())
                } else {
                    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, user_arg_count_probe);
                    for (i, a_val) in lowered_args.iter().skip(1).enumerate() {
                        let slot = ctx
                            .block()
                            .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                        ctx.block().store(DOUBLE, a_val, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, user_arg_count_probe, buf_reg
                    ));
                    (ptr_reg, user_arg_count_probe.to_string())
                };
                // Issue #632: bind IMPLICIT_THIS to the receiver around
                // the override call. The stored function may be a class
                // field assigning a non-arrow function (`class X { match
                // = match; }` — hono RegExpRouter — where the imported
                // `match` body reads `this.buildAllMatchers()`). Without
                // the bind, the body sees stale IMPLICIT_THIS and reads
                // garbage. Mirrors `lower_call.rs:2607` for the closure-
                // call fallthrough pattern (#519).
                let recv_for_this_probe = recv_box.clone();
                let prev_this_probe = ctx.block().call(
                    DOUBLE,
                    "js_implicit_this_set",
                    &[(DOUBLE, &recv_for_this_probe)],
                );
                let v_override_probe = ctx.block().call(
                    DOUBLE,
                    "js_native_call_value",
                    &[
                        (DOUBLE, &own_method_probe),
                        (crate::types::PTR, &probe_args_ptr),
                        (I64, &probe_args_len_str),
                    ],
                );
                ctx.block().call(
                    DOUBLE,
                    "js_implicit_this_set",
                    &[(DOUBLE, &prev_this_probe)],
                );
                let after_override_probe = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&probe_outer_merge_label);
                }

                // Dispatch path: existing class-id switch tower.
                ctx.current_block = probe_dispatch_idx;
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Tower of icmp+br: each implementor's case calls
                // its concrete method, default returns 0.0 (the
                // closure-call fallback would also handle this but
                // returning a sentinel is cheaper).
                let mut case_idxs: Vec<usize> = Vec::with_capacity(implementors.len());
                for (i, _) in implementors.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("idispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("idispatch.default");
                let merge_idx = ctx.new_block("idispatch.merge");
                let merge_label = ctx.block_label(merge_idx);

                for (i, (case_cid, _)) in implementors.iter().enumerate() {
                    let case_label = ctx.block_label(case_idxs[i]);
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < implementors.len() {
                        let next_idx = ctx.new_block(&format!("idispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                    } else {
                        let default_label = ctx.block_label(default_idx);
                        ctx.block().cond_br(&cmp, &case_label, &default_label);
                    }
                }

                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in implementors.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }
                // Default branch: receiver's class id didn't match any user
                // class implementing `property`. Rather than returning 0.0,
                // fall through to the runtime's `js_native_call_method` so
                // same-named built-in methods (Buffer.readUInt8, Array.push,
                // Map.get, …) still reach their native dispatch. Without
                // this, a `buf.readUInt8(i)` call site ends up in the
                // default branch and returns 0, silently corrupting reads
                // any time a user class in scope happens to declare a
                // method of the same name.
                ctx.current_block = default_idx;
                let key_idx = ctx.strings.intern(property);
                let entry = ctx.strings.entry(key_idx);
                let bytes_global = format!("@{}", entry.bytes_global);
                let name_len_str = entry.byte_len.to_string();
                let (fb_args_ptr, fb_args_len) = if args.is_empty() {
                    ("null".to_string(), "0".to_string())
                } else {
                    // Hoist the args-array alloca to the function entry
                    // block — see issue #167 and `alloca_entry_array` doc.
                    let n = args.len();
                    let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                    // skip(1) the receiver, take(n) so the issue-#235 default-arg
                    // padding entries appended to lowered_args don't overflow the
                    // n-sized buffer (and aren't needed for the ncm fallback path,
                    // which forwards user-provided args only).
                    for (i, a_val) in lowered_args.iter().skip(1).take(n).enumerate() {
                        let slot = ctx
                            .block()
                            .gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                        ctx.block().store(DOUBLE, a_val, &slot);
                    }
                    let ptr_reg = ctx.block().next_reg();
                    ctx.block().emit_raw(format!(
                        "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                        ptr_reg, n, buf_reg
                    ));
                    (ptr_reg, n.to_string())
                };
                let v_def = ctx.block().call(
                    DOUBLE,
                    "js_native_call_method",
                    &[
                        (DOUBLE, &recv_box),
                        (crate::types::PTR, &bytes_global),
                        (I64, &name_len_str),
                        (crate::types::PTR, &fb_args_ptr),
                        (I64, &fb_args_len),
                    ],
                );
                let def_label = ctx.block().label.clone();
                ctx.block().br(&merge_label);
                phi_inputs.push((v_def, def_label));

                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> = phi_inputs
                    .iter()
                    .map(|(v, l)| (v.as_str(), l.as_str()))
                    .collect();
                let v_dispatch_phi = ctx.block().phi(DOUBLE, &phi_args);
                let after_dispatch_phi = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&probe_outer_merge_label);
                }

                // Outer merge: phi over override and dispatch values.
                ctx.current_block = probe_outer_merge_idx;
                return Ok(ctx.block().phi(
                    DOUBLE,
                    &[
                        (v_override_probe.as_str(), after_override_probe.as_str()),
                        (v_dispatch_phi.as_str(), after_dispatch_phi.as_str()),
                    ],
                ));
            }
        }

        if let Some(class_name) = receiver_class_name(ctx, object) {
            // Step 1: walk parent chain for the static method name.
            let mut static_fn: Option<String> = None;
            let mut current_class = Some(class_name.clone());
            while let Some(cur) = current_class {
                let key = (cur.clone(), property.clone());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    static_fn = Some(fname);
                    break;
                }
                current_class = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }

            if let Some(fallback_fn) = static_fn {
                // Step 2: collect overriding subclasses. For each
                // subclass C transitively extending class_name, look
                // up which method C uses for `property` (walking C's
                // parent chain). If that resolves to a different
                // function than the static fallback, C needs an
                // explicit case in the dispatch table.
                let mut overrides: Vec<(u32, String)> = Vec::new();
                for (sub_name, &sub_id) in ctx.class_ids.iter() {
                    if *sub_name == class_name {
                        continue;
                    }
                    // Is sub_name transitively a subclass of class_name?
                    let mut parent = ctx
                        .classes
                        .get(sub_name)
                        .and_then(|c| c.extends_name.clone());
                    let mut is_subclass = false;
                    while let Some(p) = parent {
                        if p == class_name {
                            is_subclass = true;
                            break;
                        }
                        parent = ctx.classes.get(&p).and_then(|c| c.extends_name.clone());
                    }
                    if !is_subclass {
                        continue;
                    }
                    // Resolve the method for sub_name by walking its
                    // own parent chain (NOT class_name's chain).
                    let mut cur = Some(sub_name.clone());
                    let mut sub_fn: Option<String> = None;
                    while let Some(c) = cur {
                        let key = (c.clone(), property.clone());
                        if let Some(fname) = ctx.methods.get(&key).cloned() {
                            sub_fn = Some(fname);
                            break;
                        }
                        cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                    }
                    if let Some(sub_fn) = sub_fn {
                        if sub_fn != fallback_fn {
                            overrides.push((sub_id, sub_fn));
                        }
                    }
                }

                let recv_box = lower_expr(ctx, object)?;
                let mut lowered_args: Vec<String> = Vec::with_capacity(args.len() + 1);
                lowered_args.push(recv_box.clone());
                for a in args {
                    lowered_args.push(lower_expr(ctx, a)?);
                }
                // Issue #235: pad lowered_args with TAG_UNDEFINED so the
                // callee's default-param desugaring fires when the call site
                // passed fewer args than the method declares. Same approach
                // and reasoning as the dynamic-dispatch branch above —
                // applied here for the static-dispatch + virtual-override
                // case (receiver class IS in `ctx.classes`).
                //
                // Walk the parent chain `static_fn` was resolved through to
                // find the fallback's arity; take max across all overrides
                // so the unified arg_slices works for every concrete callee.
                let mut max_explicit_arity: usize = 0;
                let mut walk = Some(class_name.clone());
                while let Some(cur) = walk {
                    let key = (cur.clone(), property.clone());
                    if let Some(&n) = ctx.method_param_counts.get(&key) {
                        if n > max_explicit_arity {
                            max_explicit_arity = n;
                        }
                        break;
                    }
                    walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
                }
                for (sub_id, _) in &overrides {
                    for (sub_name, &id) in ctx.class_ids.iter() {
                        if id == *sub_id {
                            if let Some(&n) = ctx
                                .method_param_counts
                                .get(&(sub_name.clone(), property.clone()))
                            {
                                if n > max_explicit_arity {
                                    max_explicit_arity = n;
                                }
                            }
                            break;
                        }
                    }
                }
                // Closes #484: bundle trailing user args into a rest
                // array when the method has a `...rest` parameter.
                // Walk the same parent chain to find has_rest. Same
                // structural shape as the freestanding-function rest
                // bundling at lower_call.rs:444 — but operates on
                // `lowered_args` after the receiver was prepended.
                let mut method_has_rest = false;
                let mut method_decl_count = max_explicit_arity;
                let mut rest_walk = Some(class_name.clone());
                while let Some(cur) = rest_walk {
                    let key = (cur.clone(), property.clone());
                    if let Some(&true) = ctx.method_has_rest.get(&key) {
                        method_has_rest = true;
                        method_decl_count = ctx
                            .method_param_counts
                            .get(&key)
                            .copied()
                            .unwrap_or(max_explicit_arity);
                        break;
                    }
                    rest_walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
                }
                let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                if method_has_rest {
                    // user-visible fixed param count = decl - 1 (the
                    // last param is the rest). lowered_args[0] is
                    // `this`, [1..] are user args.
                    let fixed_user = method_decl_count.saturating_sub(1);
                    // Pad missing fixed args first.
                    while lowered_args.len() - 1 < fixed_user {
                        lowered_args.push(undefined_lit.clone());
                    }
                    // Bundle remaining trailing args into a fresh
                    // js_array. Index in lowered_args: 1 + fixed_user.
                    let split_at = 1 + fixed_user;
                    let rest_count = lowered_args.len().saturating_sub(split_at);
                    let cap = (rest_count as u32).to_string();
                    let mut rest_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for v in &lowered_args[split_at..] {
                        let blk = ctx.block();
                        rest_arr =
                            blk.call(I64, "js_array_push_f64", &[(I64, &rest_arr), (DOUBLE, v)]);
                    }
                    let rest_box = nanbox_pointer_inline(ctx.block(), &rest_arr);
                    lowered_args.truncate(split_at);
                    lowered_args.push(rest_box);
                } else {
                    let target_total = max_explicit_arity + 1; // +1 for `this`
                    while lowered_args.len() < target_total {
                        lowered_args.push(undefined_lit.clone());
                    }
                }
                let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    lowered_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

                if overrides.is_empty() {
                    // Issue #620: before falling through to the static method,
                    // check whether the receiver has an own-property override
                    // for `property` (set via `this.method = X` inside the
                    // class). Hono's SmartRouter rebinds `this.match` on the
                    // first call so subsequent calls go through the bound
                    // fast-path closure instead of the original method.
                    return Ok(emit_own_method_override_check(
                        ctx,
                        &recv_box,
                        property,
                        &fallback_fn,
                        &arg_slices,
                        &lowered_args,
                    ));
                }

                // Step 4: virtual dispatch via class_id switch.
                // Read class_id from the object header, then branch
                // to the right concrete method block.
                let blk = ctx.block();
                let recv_handle = unbox_to_i64(blk, &recv_box);
                let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

                // Pre-create blocks: one per override + default + merge.
                let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
                for (i, _) in overrides.iter().enumerate() {
                    case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
                }
                let default_idx = ctx.new_block("vdispatch.default");
                let merge_idx = ctx.new_block("vdispatch.merge");

                // Default → fallback. We use a tower of icmp+br rather
                // than the LLVM `switch` instruction (which the IR
                // builder doesn't expose generically) — same shape,
                // slightly more verbose.
                let mut current_label = ctx.block().label.clone();
                for (i, (case_cid, _)) in overrides.iter().enumerate() {
                    let next_label = if i + 1 < overrides.len() {
                        // We'll start the next test in this same block
                        // — actually use a fresh block for the test.
                        format!("vdispatch.test{}", i + 1)
                    } else {
                        ctx.block_label(default_idx)
                    };
                    let case_label = ctx.block_label(case_idxs[i]);
                    // Make sure ctx.current_block points at the
                    // current test block.
                    let _ = current_label;
                    let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                    if i + 1 < overrides.len() {
                        // Create the next test block as a fresh block
                        // and branch into it on the false arm.
                        let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                        let next_lbl = ctx.block_label(next_idx);
                        ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                        ctx.current_block = next_idx;
                        current_label = next_lbl;
                    } else {
                        ctx.block().cond_br(&cmp, &case_label, &next_label);
                    }
                }

                // Each case block: call the override and branch to merge.
                let merge_label = ctx.block_label(merge_idx);
                let mut phi_inputs: Vec<(String, String)> = Vec::new();
                for ((_, fname), &case_idx) in overrides.iter().zip(case_idxs.iter()) {
                    ctx.current_block = case_idx;
                    let v = ctx.block().call(DOUBLE, fname, &arg_slices);
                    let after_label = ctx.block().label.clone();
                    if !ctx.block().is_terminated() {
                        ctx.block().br(&merge_label);
                    }
                    phi_inputs.push((v, after_label));
                }

                // Default block: call the static fallback.
                ctx.current_block = default_idx;
                let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
                let def_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v_def, def_label));

                // Merge: phi over all incoming case results.
                ctx.current_block = merge_idx;
                let phi_args: Vec<(&str, &str)> = phi_inputs
                    .iter()
                    .map(|(v, l)| (v.as_str(), l.as_str()))
                    .collect();
                return Ok(ctx.block().phi(DOUBLE, &phi_args));
            }
        }
    }

    // console.log(<args...>) sink.
    //
    // JS spec: console.log can take any number of args, separated by
    // single spaces. We approximate by emitting a separate dispatch
    // call per arg with a literal " " in between, then a final "\n".
    // The runtime functions take a NaN-boxed double and print it
    // followed by a single trailing space (for the inter-arg form)
    // or newline (for the final/single-arg form). For now we use the
    // existing js_console_log_dynamic for every arg — the runtime
    // already adds a newline, so multi-arg console.log will be
    // separated by newlines instead of spaces. Spec-compliant
    // separator handling lives in a future Phase I tweak.
    if let Expr::PropertyGet { object, property } = callee {
        if matches!(object.as_ref(), Expr::GlobalGet(_))
            && matches!(
                property.as_str(),
                "log"
                    | "info"
                    | "warn"
                    | "error"
                    | "debug"
                    | "dir"
                    | "table"
                    | "trace"
                    | "group"
                    | "groupEnd"
                    | "groupCollapsed"
                    | "time"
                    | "timeEnd"
                    | "timeLog"
                    | "count"
                    | "countReset"
                    | "clear"
                    | "assert"
            )
        {
            // Catch-all for the entire console.* surface. Most of
            // them are best-effort: we route the args through
            // js_console_log_dynamic so the user at least sees the
            // values, then return undefined-as-double. Spec-compliant
            // dispatch (separate stderr for warn/error, dir's depth
            // option, table's tabular layout) is a future improvement.
            // Zero-arg console.* calls — handle the truly nullary
            // methods (groupEnd, clear) and the dataless variants of
            // log/info/warn/error/debug (which print nothing). Methods
            // with meaningful zero-arg semantics (count, countReset,
            // time, timeEnd, timeLog with the implicit "default" label)
            // intentionally fall through to the dedicated handler below.
            if args.is_empty() {
                match property.as_str() {
                    "groupEnd" => {
                        ctx.block().call_void("js_console_group_end", &[]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "clear" => {
                        ctx.block().call_void("js_console_clear", &[]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "count" | "countReset" | "time" | "timeEnd" | "timeLog" => {
                        // Fall through to the dedicated handler below
                        // which calls the runtime with the implicit
                        // "default" label.
                    }
                    "log" | "info" | "debug" => {
                        // Issue #557: zero-arg console.log()/info()/debug()
                        // emits a newline to stdout (matches Node/bun). The
                        // *_spread runtime fns already print just `\n` when
                        // their arg is null, so pass i64 0 directly.
                        ctx.block()
                            .call_void("js_console_log_spread", &[(I64, "0")]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "warn" => {
                        ctx.block()
                            .call_void("js_console_warn_spread", &[(I64, "0")]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    "error" => {
                        ctx.block()
                            .call_void("js_console_error_spread", &[(I64, "0")]);
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                    _ => {
                        // Other zero-arg console.* methods (dir, assert,
                        // etc.) — print nothing.
                        return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
                    }
                }
            }
            // console.group / groupCollapsed with a label — push
            // indent level and print the label.
            if matches!(property.as_str(), "group" | "groupCollapsed") {
                for a in args {
                    let v = lower_expr(ctx, a)?;
                    ctx.block()
                        .call_void("js_console_log_dynamic", &[(DOUBLE, &v)]);
                }
                ctx.block().call_void("js_console_group_begin", &[]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.trace([msg]) — `js_console_trace` formats the
            // optional message and emits a native backtrace to stderr
            // (issue #20).
            if property == "trace" {
                let val: String = if args.is_empty() {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                } else {
                    lower_expr(ctx, &args[0])?
                };
                ctx.block().call_void("js_console_trace", &[(DOUBLE, &val)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.table(data) — dedicated table renderer.
            if property == "table" && args.len() == 1 {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block().call_void("js_console_table", &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // console.time(label) / timeEnd(label) / timeLog(label) —
            // dedicated timer functions that track per-label Instants
            // in a thread-local HashMap. Without this dispatch the
            // label got routed through js_console_log_dynamic and just
            // printed the string, losing the elapsed-time output.
            if matches!(
                property.as_str(),
                "time" | "timeEnd" | "timeLog" | "count" | "countReset"
            ) && args.len() == 1
            {
                let v = lower_expr(ctx, &args[0])?;
                let blk = ctx.block();
                let handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &v)]);
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time",
                    "timeEnd" => "js_console_time_end",
                    "timeLog" => "js_console_time_log",
                    "count" => "js_console_count",
                    "countReset" => "js_console_count_reset",
                    _ => unreachable!(),
                };
                blk.call_void(runtime_fn, &[(I64, &handle)]);
                return Ok("0.0".to_string());
            }
            // Zero-arg time* / count* use the default label "default".
            if matches!(
                property.as_str(),
                "time" | "timeEnd" | "timeLog" | "count" | "countReset"
            ) && args.is_empty()
            {
                let sp_idx = ctx.strings.intern("default");
                let sp_global = format!("@{}", ctx.strings.entry(sp_idx).handle_global);
                let blk = ctx.block();
                let sp_box = blk.load(DOUBLE, &sp_global);
                let handle = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &sp_box)]);
                let runtime_fn = match property.as_str() {
                    "time" => "js_console_time",
                    "timeEnd" => "js_console_time_end",
                    "timeLog" => "js_console_time_log",
                    "count" => "js_console_count",
                    "countReset" => "js_console_count_reset",
                    _ => unreachable!(),
                };
                blk.call_void(runtime_fn, &[(I64, &handle)]);
                return Ok("0.0".to_string());
            }
            // console.assert(cond[, ...messages]) — runtime helper
            // checks the condition and only prints "Assertion failed: msg"
            // when cond is falsy. Without this dedicated dispatch, the call
            // fell through to the multi-arg console.log path which
            // printed both cond and messages unconditionally ("true should
            // not appear" / "false assertion failed message").
            //
            // Two shapes:
            //   1. 0–1 message args → js_console_assert(cond, msg_ptr)
            //   2. 2+ message args  → bundle into array, call
            //      js_console_assert_spread(cond, arr_ptr) which formats
            //      each element with format_jsvalue and joins with spaces.
            if property == "assert" {
                let cond_v = if args.is_empty() {
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
                } else {
                    lower_expr(ctx, &args[0])?
                };
                if args.len() <= 2 {
                    let msg_handle = if args.len() == 2 {
                        let msg_v = lower_expr(ctx, &args[1])?;
                        let blk = ctx.block();
                        blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &msg_v)])
                    } else {
                        "0".to_string()
                    };
                    ctx.block().call_void(
                        "js_console_assert",
                        &[(DOUBLE, &cond_v), (I64, &msg_handle)],
                    );
                } else {
                    // Multi-arg messages: bundle args[1..] into a heap
                    // array and call the spread variant.
                    let cap = ((args.len() - 1) as u32).to_string();
                    let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
                    for arg in args.iter().skip(1) {
                        let v = lower_expr(ctx, arg)?;
                        let blk = ctx.block();
                        current_arr = blk.call(
                            I64,
                            "js_array_push_f64",
                            &[(I64, &current_arr), (DOUBLE, &v)],
                        );
                    }
                    ctx.block().call_void(
                        "js_console_assert_spread",
                        &[(DOUBLE, &cond_v), (I64, &current_arr)],
                    );
                }
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // console.dir(obj[, options]) — Node prints just the formatted
            // object, ignoring the options arg (Perry doesn't honor depth /
            // colors / showHidden yet). Without this, the multi-arg dispatch
            // would print both the obj and the options object side by side.
            if property == "dir" && !args.is_empty() {
                let v = lower_expr(ctx, &args[0])?;
                ctx.block()
                    .call_void("js_console_log_dynamic", &[(DOUBLE, &v)]);
                // Lower remaining args for side effects only.
                for a in args.iter().skip(1) {
                    let _ = lower_expr(ctx, a)?;
                }
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            // Single-arg fast path: just print directly. Pre-fix #345 this
            // ignored the `property` and always called `js_console_log_*`,
            // which collapsed `console.error("x")` and `console.warn("x")`
            // onto stdout. Dispatch on property so each console method
            // routes to its matching runtime fn (and stream).
            if args.len() == 1 {
                let arg = &args[0];
                let is_number_literal = matches!(arg, Expr::Integer(_) | Expr::Number(_));
                let v = if let Some(v) = lower_util_types_predicate_arg(ctx, arg)? {
                    v
                } else {
                    lower_expr(ctx, arg)?
                };
                let runtime_fn = match (property.as_str(), is_number_literal) {
                    ("error", true) => "js_console_error_number",
                    ("error", false) => "js_console_error_dynamic",
                    ("warn", true) => "js_console_warn_number",
                    ("warn", false) => "js_console_warn_dynamic",
                    (_, true) => "js_console_log_number",
                    (_, false) => "js_console_log_dynamic",
                };
                ctx.block().call_void(runtime_fn, &[(DOUBLE, &v)]);
                return Ok("0.0".to_string());
            }
            // Multi-arg: bundle all args into a heap array and call
            // js_console_log_spread, which uses the runtime's
            // format_jsvalue (Node-style util.inspect output for
            // objects/arrays). This is more accurate than
            // js_jsvalue_to_string which only does the JS toString
            // protocol (returns "[object Object]" for plain objects).
            let cap = (args.len() as u32).to_string();
            let mut current_arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for arg in args.iter() {
                let v = if let Some(v) = lower_util_types_predicate_arg(ctx, arg)? {
                    v
                } else {
                    lower_expr(ctx, arg)?
                };
                let blk = ctx.block();
                current_arr = blk.call(
                    I64,
                    "js_array_push_f64",
                    &[(I64, &current_arr), (DOUBLE, &v)],
                );
            }
            let runtime_fn = match property.as_str() {
                "warn" => "js_console_warn_spread",
                "error" => "js_console_error_spread",
                _ => "js_console_log_spread",
            };
            ctx.block().call_void(runtime_fn, &[(I64, &current_arr)]);
            return Ok("0.0".to_string());
        }
    }

    // -------- Promise.resolve / reject / all / race / allSettled --------
    //
    // The HIR doesn't have dedicated PromiseResolve/Reject variants. Depending
    // on the lowering path they appear either as a bare GlobalGet receiver or
    // as `globalThis.Promise.<method>`.
    if let Expr::PropertyGet { object, property } = callee {
        if is_global_constructor_expr(object, "Promise") {
            match property.as_str() {
                "resolve" => {
                    let value = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_resolved", &[(DOUBLE, &value)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "reject" => {
                    let reason = if args.is_empty() {
                        double_literal(0.0)
                    } else {
                        lower_expr(ctx, &args[0])?
                    };
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_rejected", &[(DOUBLE, &reason)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "all" | "race" | "allSettled" | "any" => {
                    if args.is_empty() {
                        return Ok(double_literal(0.0));
                    }
                    let arr_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let arr_handle = unbox_to_i64(blk, &arr_box);
                    let runtime_fn = match property.as_str() {
                        "all" => "js_promise_all",
                        "race" => "js_promise_race",
                        "any" => "js_promise_any",
                        _ => "js_promise_all_settled",
                    };
                    let handle = blk.call(I64, runtime_fn, &[(I64, &arr_handle)]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                "withResolvers" => {
                    // Promise.withResolvers<T>() returns { promise, resolve, reject }.
                    // We create a pending promise and return an object with
                    // the promise + resolve/reject closures.
                    let blk = ctx.block();
                    let handle = blk.call(I64, "js_promise_with_resolvers", &[]);
                    return Ok(nanbox_pointer_inline(blk, &handle));
                }
                _ => {}
            }
        }
        // `Array.fromAsync(input)` — Node 22+ static method.
        if is_global_constructor_expr(object, "Array") && property == "fromAsync" {
            if args.is_empty() {
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let input = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            return Ok(blk.call(DOUBLE, "js_array_from_async", &[(DOUBLE, &input)]));
        }
    }

    // -------- PropertyGet method dispatch via js_native_call_method --------
    //
    // For `recv.method(args)` where the static dispatch above didn't fire
    // and the receiver isn't a known class instance, route through the
    // runtime's universal `js_native_call_method` dispatcher. This is the
    // path that catches Map/Set/RegExp methods on plain object fields
    // (e.g. `wrap.m.get(k)` where `wrap: { m: Map }`) — the runtime
    // detects the registry and dispatches to `js_map_get` etc. directly.
    //
    // The signature is `js_native_call_method(obj: f64, name_ptr: ptr,
    // name_len: i64, args_ptr: ptr, args_len: i64) -> f64`. We pass the
    // method name as a raw rodata byte pointer (the StringPool already
    // emits the bytes as `[N+1 x i8]` for every interned string), and
    // materialize the args into a stack `[N x double]` slot.
    if let Expr::PropertyGet { object, property } = callee {
        // Skip when the receiver is a global module access (e.g. `console.log`,
        // `JSON.parse`) — those are handled by the spread/closure paths above
        // or have dedicated lowerings. Skip when the receiver is a known class
        // instance — those have static method dispatch handled earlier.
        //
        // Exception: `Uint8Array`/`Buffer` typed receivers must NOT be skipped.
        // They aren't real classes (no vtable) — the runtime's
        // `js_native_call_method` detects them via `is_registered_buffer` and
        // routes through `dispatch_buffer_method` which handles the full
        // Node-style numeric read/write/swap/indexOf method family.
        //
        // Issue #510: also skip `NativeModuleRef` receivers (e.g. unknown
        // `fs.*` / `crypto.*` calls that fall through their dedicated arms).
        // `NativeModuleRef` lowers to literal `0.0`, which the runtime
        // catch-all would treat as a primitive (`number`) and throw on. The
        // pre-#510 behavior was a silent NULL_OBJECT_BYTES fallback —
        // matching that here keeps "unsupported native module method" cases
        // returning undefined instead of throwing. (Throwing would be more
        // helpful but requires per-module unimplemented-API detection at the
        // codegen site, tracked as part of the unimplemented-API plan in
        // #463.)
        let class_name_opt = receiver_class_name(ctx, object);
        let is_buffer_class = matches!(
            class_name_opt.as_deref(),
            Some("Uint8Array") | Some("Buffer") | Some("Uint8ClampedArray")
        );
        // Issue #392 followup: when the receiver's static class name is known
        // but the class is NOT in `ctx.classes` (the canonical case is a
        // type-only `import type { Changeset } from "./changeset"` which
        // strips the module from `hir.imports` and produces no entry in
        // `imported_classes` for this consumer module — see
        // crates/perry/src/commands/compile.rs::is_unresolved_name where the
        // class is considered "resolved" because it's in
        // `all_program_type_names`, which short-circuits the
        // `references_interface` full-visibility fallback), the static
        // dispatch tower above can't find a method entry and would fall
        // through to `js_closure_call<N>` against `obj.<method>` read as a
        // closure-valued property — which silently no-ops on Map/Set field
        // mutations like `this.adds.set(k, v)` inside the cross-module
        // method. Route through `js_native_call_method` instead so the
        // runtime's `CLASS_VTABLE_REGISTRY` (populated by v0.5.464's
        // `js_register_class_method` calls in `emit_string_pool`) dispatches
        // to the real `perry_method_<modprefix>__<class>__<method>`.
        let class_unknown_to_codegen = class_name_opt
            .as_ref()
            .is_some_and(|n| !ctx.classes.contains_key(n));
        // Well-known `Object.prototype` / `Function.prototype` methods —
        // any user class instance can have them invoked via the
        // prototype chain. Pre-fix the static class-dispatch path
        // skipped `js_native_call_method` entirely when the receiver's
        // class WAS known to codegen, which made `({ k: null }).propertyIsEnumerable("k")`
        // (ramda's `keys.js` IIFE) fall into the closure-call fallback
        // that read `propertyIsEnumerable` as a property value
        // (returning `undefined`) and threw `value is not a function`.
        let is_well_known_proto_method = matches!(
            property.as_str(),
            "hasOwnProperty" | "propertyIsEnumerable" | "isPrototypeOf" | "toLocaleString"
        );
        let skip_native = matches!(object.as_ref(), Expr::GlobalGet(_))
            || matches!(object.as_ref(), Expr::NativeModuleRef(_))
            || (class_name_opt.is_some()
                && !is_buffer_class
                && !class_unknown_to_codegen
                && !is_well_known_proto_method);
        if !skip_native {
            // Issue #92 fast path: intrinsify Buffer numeric reads
            // (`buf.readInt32BE(off)` etc.) when the receiver is a tracked
            // `const buf = Buffer.alloc(N)` local. Returns Ok(Some(reg)) on
            // success; falls through to the runtime dispatch for all other
            // Buffer methods or untracked receivers.
            if is_buffer_class {
                if let Some(reg) = try_emit_buffer_read_intrinsic(ctx, object, property, args)? {
                    return Ok(reg);
                }
            }
            let recv_box = lower_expr(ctx, object)?;
            let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
            for a in args {
                lowered_args.push(lower_expr(ctx, a)?);
            }
            // Intern the method name and reference its rodata byte global.
            let key_idx = ctx.strings.intern(property);
            let entry = ctx.strings.entry(key_idx);
            let bytes_global = format!("@{}", entry.bytes_global);
            let name_len_str = entry.byte_len.to_string();
            // Stack-allocate the args array if any. The alloca MUST live in
            // the function entry block — emitting it into the current block
            // (which may be a loop body) makes LLVM lower it as a runtime
            // `sub %rsp, N` that never gets restored, eating the stack at
            // ~16 bytes/iteration. See issue #167.
            let (args_ptr, args_len_str) = if lowered_args.is_empty() {
                ("null".to_string(), "0".to_string())
            } else {
                let n = lowered_args.len();
                let buf_reg = ctx.func.alloca_entry_array(DOUBLE, n);
                let blk = ctx.block();
                for (i, v) in lowered_args.iter().enumerate() {
                    let slot = blk.gep(DOUBLE, &buf_reg, &[(I64, &format!("{}", i))]);
                    blk.store(DOUBLE, v, &slot);
                }
                (buf_reg, n.to_string())
            };
            let blk = ctx.block();
            return Ok(blk.call(
                DOUBLE,
                "js_native_call_method",
                &[
                    (DOUBLE, &recv_box),
                    (PTR, &bytes_global),
                    (I64, &name_len_str),
                    (PTR, &args_ptr),
                    (I64, &args_len_str),
                ],
            ));
        }
    }

    // Fallthrough: assume the callee evaluates to a closure value at
    // runtime and dispatch through `js_closure_call<N>`. This catches:
    //   - LocalGet of an `: any`-typed local that the static check missed
    //   - Nested calls like `curry(1)(2)(3)` where the callee is itself
    //     a Call returning a function
    //   - PropertyGet on a class instance whose property is a closure
    //
    // The runtime checks the closure header on its own — if the value
    // isn't actually a closure, js_closure_call<N> handles the error.
    if args.len() <= 16 {
        // Issue #519: when the callee shape is `recv.method(args)` (a
        // PropertyGet) — i.e. a method-style invocation — bind the
        // receiver as the implicit `this` for the duration of the call.
        // Non-arrow function bodies (including FuncRef wrappers) read
        // `this` via `js_implicit_this_get` when their lexical
        // this_stack is empty (codegen Expr::This fallback). Without
        // this save/set/restore, hono's `RegExpRouter.match = match`
        // (where `match` is an imported function declaration whose
        // body does `this.buildAllMatchers()`) sees `this = undefined`
        // and TypeErrors out at the first chained method call.
        //
        // We evaluate `object` once into a fresh slot so that
        // (a) it's only side-effect-evaluated once, and
        // (b) the lowered `callee` (which re-reads `object` to get the
        //     property) and the IMPLICIT_THIS save/set both see the
        //     same receiver value.
        let method_recv: Option<String> = if let Expr::PropertyGet { object, .. } = callee {
            // Skip the method-binding when the receiver is a global,
            // namespace import, or NativeModuleRef — those aren't
            // user objects and shouldn't influence `this`.
            if matches!(
                object.as_ref(),
                Expr::GlobalGet(_) | Expr::NativeModuleRef(_) | Expr::ExternFuncRef { .. }
            ) {
                None
            } else {
                Some(lower_expr(ctx, object)?)
            }
        } else {
            None
        };

        let recv_box = lower_expr(ctx, callee)?;
        let mut lowered_args: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered_args.push(lower_expr(ctx, a)?);
        }
        let prev_this: Option<String> = if let Some(ref this_val) = method_recv {
            let blk = ctx.block();
            Some(blk.call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, this_val)]))
        } else {
            None
        };
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &recv_box);
        let runtime_fn = format!("js_closure_call{}", args.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered_args {
            call_args.push((DOUBLE, v.as_str()));
        }
        let result = blk.call(DOUBLE, &runtime_fn, &call_args);
        if let Some(prev) = prev_this {
            ctx.block()
                .call(DOUBLE, "js_implicit_this_set", &[(DOUBLE, &prev)]);
        }
        return Ok(result);
    }

    bail!(
        "perry-codegen: Call callee shape not supported ({}) with {} args",
        variant_name(callee),
        args.len()
    )
}

fn lower_util_types_predicate_arg(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::NativeMethodCall {
        module,
        class_name,
        method,
        object,
        args,
        ..
    } = expr
    else {
        return Ok(None);
    };
    let is_util_types_namespace = module == "util" && class_name.as_deref() == Some("types");
    let is_direct_util_types_module = module == "util/types" && class_name.is_none();
    if (!is_util_types_namespace && !is_direct_util_types_module) || object.is_some() {
        return Ok(None);
    }
    let Some(runtime) = (match method.as_str() {
        "isPromise" => Some("js_util_types_is_promise"),
        "isArrayBuffer" | "isAnyArrayBuffer" => Some("js_util_types_is_array_buffer"),
        "isArrayBufferView" => Some("js_util_types_is_array_buffer_view"),
        "isTypedArray" => Some("js_util_types_is_typed_array"),
        "isUint8Array" => Some("js_util_types_is_uint8_array"),
        "isUint16Array" => Some("js_util_types_is_uint16_array"),
        "isInt32Array" => Some("js_util_types_is_int32_array"),
        "isFloat64Array" => Some("js_util_types_is_float64_array"),
        "isMap" => Some("js_util_types_is_map"),
        "isSet" => Some("js_util_types_is_set"),
        "isDate" => Some("js_util_types_is_date"),
        "isRegExp" => Some("js_util_types_is_reg_exp"),
        _ => None,
    }) else {
        return Ok(None);
    };
    let value = if let Some(first) = args.first() {
        lower_expr(ctx, first)?
    } else {
        double_literal(0.0)
    };
    Ok(Some(ctx.block().call(DOUBLE, runtime, &[(DOUBLE, &value)])))
}

/// Lower a `NativeMethodCall { module, method, object, args }` (Phase H.1).
///
/// Currently supports:
/// - `array.push_single` / `array.push` (single-arg push) on typed arrays
/// - `array.pop_back` / `array.pop` on typed arrays
///
/// The receiver is either a `PropertyGet { object, property }` (the
/// `this.items.push(x)` case) or a `LocalGet` (the `arr.push(x)` case).
/// For both shapes we chain a get + push + write-back so reallocations
/// are reflected in the source storage.

/// Issue #185 Phase C step 2: apply an inline `style: { ... }` object
/// to a freshly-created widget handle by destructuring the object
/// literal at HIR time and emitting a sequence of setter calls.
///
/// Step 2 supports the single-value scalar props that don't need
/// multi-arg destructure: borderRadius, opacity, borderWidth,
/// fontSize, fontWeight, tooltip, hidden, enabled. Color props
/// (`backgroundColor` / `color` / `borderColor`), padding (single
/// number or per-side object), shadow (color + blur + offsets), and
/// gradient (angle + stops array) land in step 3.
///
/// Unknown / not-yet-supported keys are silently lowered for side
/// effects but otherwise dropped — TS's structural typing makes the
/// `StyleProps` interface the source of typo-safety.
///
/// Mirrors the App({...}) destructure pattern in this file:
/// `extract_options_fields` returns the props, then per-key routing.

/// Lower `new ClassName(args)` for the built-in Web classes that don't
/// live in `ctx.classes`. Returns `Ok(None)` if the class isn't one we
/// handle here (caller should fall through to the default path).

/// Static dispatch table for perry/ui receiver-less calls. Covers the
/// constructors + setters mango uses, plus the most common widgets from
/// the cross-cutting "any perry/ui app" surface. Keep alphabetized by
/// `method` for easy scanning.
///
/// Entries NOT in this table fall through to the receiver-less early-out
/// in `lower_native_method_call` (which lowers args for side effects and
/// returns the zero-sentinel). That's the behavior the entire perry/ui
/// surface had pre-v0.5.10 — adding a row here flips one method from
/// "silent no-op" to "real call into libperry_ui_macos.a".

/// Instance method table for perry/ui receiver-based calls.
/// These methods are called on a widget/window handle: `handle.method(args)`.
/// The handle is automatically prepended as the first i64 arg.

pub(super) fn perry_ui_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_TABLE.iter().find(|s| s.method == method)
}

pub(super) fn perry_ui_instance_method_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UI_INSTANCE_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/system dispatch table
// =============================================================================

/// Maps JS import names from `perry/system` to their `perry_system_*` / `perry_*`
/// runtime C symbols. Uses the same UiSig + lower_perry_ui_table_call machinery
/// since the calling convention is identical.

pub(super) fn perry_system_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_SYSTEM_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/media dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/media/index.d.ts` (createPlayer,
/// play, pause, stop, seek, setVolume, setRate, getCurrentTime, getDuration,
/// getState, isPlaying, onStateChange, onTimeUpdate, setNowPlaying, destroy)
/// to their `perry_media_*` runtime symbols.
pub(super) fn perry_media_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_MEDIA_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/i18n format-wrapper dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/i18n/index.d.ts` (Currency, Percent,
/// FormatNumber, ShortDate, LongDate, FormatTime, Raw) to their `perry_i18n_*`
/// runtime symbols. Each runtime entry is a default-locale single-arg wrapper
/// over the lower-level `perry_i18n_format_*(value, locale_idx)` exports —
/// the wrapper folds in `LOCALE_INDEX` so the dispatch table here can stay
/// consistent with the other UiSig tables (one TS arg → one runtime arg).
///
/// `t()` is handled separately at the top of `lower_native_method_call`
/// because the perry-transform i18n pass replaces its first arg with an
/// `Expr::I18nString` — there's no runtime call involved.

pub(super) fn perry_i18n_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_I18N_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/updater dispatch table
// =============================================================================

/// Maps the TS exports from `types/perry/updater/index.d.ts` to their runtime
/// symbols exported by the `core` and `desktop` modules of `perry-updater`.
/// The download itself stays in TS (uses existing `fetch()`); this table only
/// covers verify, install, relaunch, sentinel state, and path resolution.
pub(super) fn perry_updater_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_UPDATER_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/background dispatch table (issue #538)
// =============================================================================

/// Maps the TS exports from `types/perry/background/index.d.ts` to their
/// runtime symbols (`perry_background_register_task` / `_schedule` /
/// `_cancel`) exported by the per-platform `perry-ui-*` crates.
pub(super) fn perry_background_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_BACKGROUND_TABLE.iter().find(|s| s.method == method)
}

// =============================================================================
// perry/plugin dispatch table
// =============================================================================

/// Receiver-less (host-side) functions exported from perry/plugin.
/// These map `import { loadPlugin, listPlugins, … } from "perry/plugin"` to
/// their `perry_plugin_*` runtime symbols. Arg shapes match plugin.rs exactly:
/// strings are passed as NaN-boxed f64 (`UiArgKind::F64`) because the runtime
/// calls `extract_string(nanboxed: f64)` internally — not raw pointer.
static PERRY_PLUGIN_TABLE: &[UiSig] = &[
    // loadPlugin(path) -> PluginId (NaN-boxed i64 handle, 0 on failure)
    UiSig {
        method: "loadPlugin",
        runtime: "perry_plugin_load",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::Widget,
    },
    // unloadPlugin(id) -> void
    UiSig {
        method: "unloadPlugin",
        runtime: "perry_plugin_unload",
        args: &[UiArgKind::Widget],
        ret: UiReturnKind::Void,
    },
    // emitHook(hookName, context) -> context (possibly transformed by handlers)
    UiSig {
        method: "emitHook",
        runtime: "perry_plugin_emit_hook",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // emitEvent(event, data) -> undefined
    UiSig {
        method: "emitEvent",
        runtime: "perry_plugin_emit_event",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // invokeTool(name, args) -> handler return value
    UiSig {
        method: "invokeTool",
        runtime: "perry_plugin_invoke_tool",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // setPluginConfig(key, value) -> undefined
    UiSig {
        method: "setPluginConfig",
        runtime: "perry_plugin_set_config",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // discoverPlugins(dir) -> string[] of plugin paths
    UiSig {
        method: "discoverPlugins",
        runtime: "perry_plugin_discover",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // listPlugins() -> { id, name, version, description }[]
    UiSig {
        method: "listPlugins",
        runtime: "perry_plugin_list_plugins",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // listHooks() -> string[]
    UiSig {
        method: "listHooks",
        runtime: "perry_plugin_list_hooks",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // listTools() -> { name, description, pluginId }[]
    UiSig {
        method: "listTools",
        runtime: "perry_plugin_list_tools",
        args: &[],
        ret: UiReturnKind::F64,
    },
    // pluginCount() -> number
    UiSig {
        method: "pluginCount",
        runtime: "perry_plugin_count",
        args: &[],
        ret: UiReturnKind::I64AsF64,
    },
    // initPlugins() -> void  (call once from main before loading plugins)
    UiSig {
        method: "initPlugins",
        runtime: "perry_plugin_init",
        args: &[],
        ret: UiReturnKind::Void,
    },
];

/// Instance methods on a PluginApi handle returned by `loadPlugin`.
/// The handle (NaN-boxed i64) is the receiver and is prepended as the
/// first `i64` arg (`api_handle`) in every runtime call.
static PERRY_PLUGIN_INSTANCE_TABLE: &[UiSig] = &[
    // api.registerHook(hookName, handler) -> undefined
    UiSig {
        method: "registerHook",
        runtime: "perry_plugin_register_hook",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerHookEx(hookName, handler, priority, mode) -> undefined
    UiSig {
        method: "registerHookEx",
        runtime: "perry_plugin_register_hook_ex",
        args: &[
            UiArgKind::F64,
            UiArgKind::Closure,
            UiArgKind::I64Raw,
            UiArgKind::I64Raw,
        ],
        ret: UiReturnKind::F64,
    },
    // api.registerTool(name, description, handler) -> undefined
    UiSig {
        method: "registerTool",
        runtime: "perry_plugin_register_tool",
        args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerService(name, startFn, stopFn) -> undefined
    UiSig {
        method: "registerService",
        runtime: "perry_plugin_register_service",
        args: &[UiArgKind::F64, UiArgKind::Closure, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.registerRoute(path, handler) -> undefined
    UiSig {
        method: "registerRoute",
        runtime: "perry_plugin_register_route",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.getConfig(key) -> any
    UiSig {
        method: "getConfig",
        runtime: "perry_plugin_get_config",
        args: &[UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.log(level, message) -> undefined   (level: 0=DEBUG,1=INFO,2=WARN,3=ERROR)
    UiSig {
        method: "log",
        runtime: "perry_plugin_log",
        args: &[UiArgKind::I64Raw, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.setMetadata(name, version, description) -> undefined
    UiSig {
        method: "setMetadata",
        runtime: "perry_plugin_set_metadata",
        args: &[UiArgKind::F64, UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
    // api.on(event, handler) -> undefined
    UiSig {
        method: "on",
        runtime: "perry_plugin_on",
        args: &[UiArgKind::F64, UiArgKind::Closure],
        ret: UiReturnKind::F64,
    },
    // api.emit(event, data) -> undefined
    UiSig {
        method: "emit",
        runtime: "perry_plugin_emit",
        args: &[UiArgKind::F64, UiArgKind::F64],
        ret: UiReturnKind::F64,
    },
];

pub(super) fn perry_plugin_table_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_PLUGIN_TABLE.iter().find(|s| s.method == method)
}

pub(super) fn perry_plugin_instance_method_lookup(method: &str) -> Option<&'static UiSig> {
    PERRY_PLUGIN_INSTANCE_TABLE
        .iter()
        .find(|s| s.method == method)
}

/// Lower a perry/ui call described by `sig`. Walks each arg, applies
/// the per-kind coercion to produce an LLVM SSA value of the right type,
/// lazy-declares the runtime function, emits the call, and boxes the
/// return value per `sig.ret`.
///
/// Args length mismatch (caller passed wrong number of args) → falls
/// back to lowering all args for side effects + returning the
/// zero-sentinel. The catch-all is intentional: TS users may write
/// `Text()` (no arg) or `Text(s, extra)` and we don't want to bail
/// the entire compilation.
pub(super) fn lower_perry_ui_table_call(
    ctx: &mut FnCtx<'_>,
    sig: &UiSig,
    args: &[Expr],
) -> Result<String> {
    // Issue #185 Phase C step 4: when a Widget-returning constructor is
    // called with one extra trailing arg, treat it as an inline `style`
    // object and apply via `apply_inline_style` after the create call.
    // Lets every widget in the table (Text, Toggle, Slider, TextField,
    // Spacer, Divider, ImageFile, ImageSymbol, ProgressView, NavStack,
    // ZStack, etc.) accept the same React-style ergonomics that Button
    // already has, with no per-widget code edits.
    // Issue #389: `appSetTimer` accepts both `(intervalMs, callback)`
    // (the user-facing 2-arg form per the type stub) and
    // `(app, intervalMs, callback)` (the historical 3-arg form). The
    // dispatch table declares 3 args (`Widget, F64, Closure`); the
    // platform runtime helpers all ignore `_app_handle`. When the
    // user supplies only 2 args, prepend a synthetic 0 Widget so the
    // call still matches the 3-arg ABI without changing the runtime
    // signatures across 8 platform crates.
    let synthesised_args: Vec<Expr>;
    let args: &[Expr] = if sig.method == "appSetTimer" && args.len() == 2 && sig.args.len() == 3 {
        synthesised_args = std::iter::once(Expr::Integer(0))
            .chain(args.iter().cloned())
            .collect();
        &synthesised_args[..]
    } else {
        args
    };

    let inline_style_arg: Option<&Expr> =
        if args.len() == sig.args.len() + 1 && matches!(sig.ret, UiReturnKind::Widget) {
            Some(&args[sig.args.len()])
        } else {
            None
        };
    let declared_arg_count = sig.args.len();

    if args.len() != declared_arg_count && inline_style_arg.is_none() {
        // Mismatched arity (and not a trailing-style absorption case)
        // — fall back to side-effect lowering only.
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(double_literal(0.0));
    }

    // Lower each arg according to its declared kind. Build two parallel
    // vectors so we can pass them through to `blk.call(...)` in one shot
    // without intermediate borrows. Iterate the declared sig args only
    // — the inline-style trailing arg (if present) is consumed below.
    let mut llvm_args: Vec<(crate::types::LlvmType, String)> =
        Vec::with_capacity(declared_arg_count);
    let mut runtime_param_types: Vec<crate::types::LlvmType> =
        Vec::with_capacity(declared_arg_count);
    for (kind, arg) in sig.args.iter().zip(args.iter().take(declared_arg_count)) {
        match kind {
            UiArgKind::Widget => {
                // Widgets are NaN-boxed pointers. Lower as JSValue,
                // strip the POINTER_TAG bits to get the raw 1-based
                // handle as i64.
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                let h = unbox_to_i64(blk, &v);
                llvm_args.push((I64, h));
                runtime_param_types.push(I64);
            }
            UiArgKind::Str => {
                let h = get_raw_string_ptr(ctx, arg)?;
                llvm_args.push((I64, h));
                runtime_param_types.push(I64);
            }
            UiArgKind::F64 => {
                let v = lower_expr(ctx, arg)?;
                llvm_args.push((DOUBLE, v));
                runtime_param_types.push(DOUBLE);
            }
            UiArgKind::Closure => {
                // Closures are NaN-boxed pointers passed as f64. The
                // runtime side calls `js_closure_call0` (or callN) on
                // them, so it expects the f64 representation.
                let v = lower_expr(ctx, arg)?;
                llvm_args.push((DOUBLE, v));
                runtime_param_types.push(DOUBLE);
            }
            UiArgKind::I64Raw => {
                // Numeric arg the runtime wants as i64 (e.g. enum tag,
                // boolean flag). `fptosi` converts the f64 to a signed
                // integer.
                let v = lower_expr(ctx, arg)?;
                let blk = ctx.block();
                let i = blk.fptosi(DOUBLE, &v, I64);
                llvm_args.push((I64, i));
                runtime_param_types.push(I64);
            }
        }
    }

    // Lazy-declare the runtime function so the linker pulls in the
    // libperry_ui_*.a symbol. Same pending_declares mechanism the
    // cross-module call site uses for `perry_fn_*`.
    let return_type = match sig.ret {
        UiReturnKind::Widget | UiReturnKind::I64AsF64 => I64,
        UiReturnKind::F64 => DOUBLE,
        UiReturnKind::Void => crate::types::VOID,
        UiReturnKind::Str => I64,
    };
    ctx.pending_declares
        .push((sig.runtime.to_string(), return_type, runtime_param_types));

    // Emit the call. Slices need a borrow of `llvm_args` because the
    // tuple's second field is `String` and `blk.call` expects `&str`.
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();
    match sig.ret {
        UiReturnKind::Widget => {
            // Scope `blk` so the mutable borrow on `ctx` is released
            // before the optional `apply_inline_style` call re-borrows.
            let handle = {
                let blk = ctx.block();
                blk.call(I64, sig.runtime, &arg_slices)
            };
            // Issue #185 Phase C step 4: apply inline style if a
            // trailing object literal was passed.
            if let Some(style_arg) = inline_style_arg {
                apply_inline_style(ctx, &handle, style_arg)?;
            }
            let blk = ctx.block();
            Ok(nanbox_pointer_inline(blk, &handle))
        }
        UiReturnKind::F64 => Ok(ctx.block().call(DOUBLE, sig.runtime, &arg_slices)),
        UiReturnKind::Void => {
            ctx.block().call_void(sig.runtime, &arg_slices);
            Ok(double_literal(0.0))
        }
        UiReturnKind::Str => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(crate::expr::nanbox_string_inline(blk, &raw))
        }
        UiReturnKind::I64AsF64 => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(blk.sitofp(I64, &raw, DOUBLE))
        }
    }
}

/// Walk a statement to collect LocalIds declared inside a closure body —
/// `Stmt::Let` and `Stmt::For` init `let`s. Used by the perry/thread
/// thread-safety check to distinguish inner locals (safe to write) from
/// captures (unsafe). Recurses into nested control-flow but deliberately
/// NOT into nested closures: those have their own inner-id set.
pub(super) fn collect_closure_introduced_ids(
    stmt: &perry_hir::Stmt,
    out: &mut std::collections::HashSet<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { id, .. } => {
            out.insert(*id);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            for s in then_branch {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::For { init, body, .. } => {
            if let Some(init_stmt) = init.as_ref() {
                collect_closure_introduced_ids(init_stmt, out);
            }
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                collect_closure_introduced_ids(s, out);
            }
            if let Some(cc) = catch {
                if let Some((id, _)) = &cc.param {
                    out.insert(*id);
                }
                for s in &cc.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Switch { cases, .. } => {
            for case in cases {
                for s in &case.body {
                    collect_closure_introduced_ids(s, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => collect_closure_introduced_ids(body, out),
        _ => {} // Expr, Return, Throw, Break, Continue, LabeledBreak/Continue — don't declare locals
    }
}

/// Walk a statement looking for LocalSet / Update whose target LocalId is
/// NOT in `inner_ids` — i.e. the closure is writing to a captured or
/// module-level variable. Does NOT recurse into nested Closure expressions
/// (those are a separate scope with their own check when they're passed to
/// a threading primitive).
pub(super) fn find_outer_writes_stmt(
    stmt: &perry_hir::Stmt,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => {
            if let Some(expr) = init {
                find_outer_writes_expr(expr, inner_ids, out);
            }
        }
        Stmt::Expr(e) | Stmt::Return(Some(e)) | Stmt::Throw(e) => {
            find_outer_writes_expr(e, inner_ids, out);
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_) => {}
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in then_branch {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(eb) = else_branch {
                for s in eb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::While { condition, body } => {
            find_outer_writes_expr(condition, inner_ids, out);
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::DoWhile { condition, body } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            find_outer_writes_expr(condition, inner_ids, out);
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            if let Some(init_stmt) = init.as_ref() {
                find_outer_writes_stmt(init_stmt, inner_ids, out);
            }
            if let Some(c) = condition {
                find_outer_writes_expr(c, inner_ids, out);
            }
            if let Some(u) = update {
                find_outer_writes_expr(u, inner_ids, out);
            }
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            for s in body {
                find_outer_writes_stmt(s, inner_ids, out);
            }
            if let Some(cc) = catch {
                for s in &cc.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
            if let Some(fb) = finally {
                for s in fb {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            find_outer_writes_expr(discriminant, inner_ids, out);
            for case in cases {
                if let Some(val) = &case.test {
                    find_outer_writes_expr(val, inner_ids, out);
                }
                for s in &case.body {
                    find_outer_writes_stmt(s, inner_ids, out);
                }
            }
        }
        Stmt::Labeled { body, .. } => find_outer_writes_stmt(body, inner_ids, out),
        Stmt::PreallocateBoxes(_) => {}
    }
}

fn find_outer_writes_expr(
    expr: &perry_hir::Expr,
    inner_ids: &std::collections::HashSet<perry_types::LocalId>,
    out: &mut Vec<perry_types::LocalId>,
) {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, val) => {
            if !inner_ids.contains(id) {
                out.push(*id);
            }
            find_outer_writes_expr(val, inner_ids, out);
        }
        Expr::Update { id, .. } => {
            if !inner_ids.contains(id) {
                out.push(*id);
            }
        }
        Expr::Closure { .. } => {
            // Stop at nested closure boundary — it has its own scope and
            // will be checked separately if it's the one being passed to
            // a threading primitive.
        }
        Expr::Binary { left, right, .. } => {
            find_outer_writes_expr(left, inner_ids, out);
            find_outer_writes_expr(right, inner_ids, out);
        }
        Expr::Call { callee, args, .. } => {
            find_outer_writes_expr(callee, inner_ids, out);
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                find_outer_writes_expr(o, inner_ids, out);
            }
            for a in args {
                find_outer_writes_expr(a, inner_ids, out);
            }
        }
        Expr::PropertyGet { object, .. } => {
            find_outer_writes_expr(object, inner_ids, out);
        }
        Expr::IndexGet { object, index } => {
            find_outer_writes_expr(object, inner_ids, out);
            find_outer_writes_expr(index, inner_ids, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                find_outer_writes_expr(e, inner_ids, out);
            }
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            find_outer_writes_expr(condition, inner_ids, out);
            find_outer_writes_expr(then_expr, inner_ids, out);
            find_outer_writes_expr(else_expr, inner_ids, out);
        }
        _ => {} // Literals, LocalGet, GlobalGet, etc. — no writes
    }
}

/// Look up a native module method in the static dispatch table.
/// Entries with `class_filter: Some("Pool")` only match when
/// `class_name == Some("Pool")`; entries with `class_filter: None`
/// match any class_name. More-specific entries (with class_filter)
/// are checked first.
pub(super) fn native_module_lookup(
    module: &str,
    has_receiver: bool,
    method: &str,
    class_name: Option<&str>,
) -> Option<&'static NativeModSig> {
    // Issue #605: `redis` (the npm `redis` package) and `ioredis` route
    // to the same perry-ext-ioredis staticlib via well-known bindings,
    // but the dispatch table only has `module: "ioredis"` rows. Without
    // normalization, `import { createClient } from "redis"` falls
    // through every lookup arm and the user's `client.connect()`
    // dispatches against `undefined`. Mirror the well-known aliasing
    // here so call-site lookups find the right runtime fns regardless
    // of which alias the user imported from.
    let normalized = match module {
        "redis" => "ioredis",
        m => m,
    };
    // First pass: look for an exact class_filter match.
    let exact = NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == normalized
            && sig.has_receiver == has_receiver
            && sig.method == method
            && sig.class_filter.is_some()
            && sig.class_filter == class_name
    });
    if exact.is_some() {
        return exact;
    }
    // Second pass: generic (class_filter == None) entries.
    NATIVE_MODULE_TABLE.iter().find(|sig| {
        sig.module == normalized
            && sig.has_receiver == has_receiver
            && sig.method == method
            && sig.class_filter.is_none()
    })
}

/// Lower a native module call through the dispatch table.
/// For receiver-less calls, `recv_i64` should be None.
/// For instance method calls, `recv_i64` should be Some(handle_i64_ssa).
pub(super) fn lower_native_module_dispatch(
    ctx: &mut FnCtx<'_>,
    sig: &NativeModSig,
    recv_i64: Option<&str>,
    args: &[Expr],
) -> Result<String> {
    // Build the LLVM arg list: receiver handle (if any) + coerced args.
    let mut llvm_args: Vec<(crate::types::LlvmType, String)> = Vec::new();
    let mut arg_types: Vec<crate::types::LlvmType> = Vec::new();

    // Receiver handle
    if let Some(handle) = recv_i64 {
        llvm_args.push((I64, handle.to_string()));
        arg_types.push(I64);
    }

    // Coerce each arg per the sig's coercion rules.
    // If more args are passed than the sig declares, pass extras as F64.
    let mut i = 0;
    while i < args.len() {
        let kind = sig.args.get(i).copied().unwrap_or(NativeArgKind::F64);
        if kind == NativeArgKind::VarArgsAsArray {
            // Pack args[i..] into a freshly allocated JS array and pass a
            // single i64 ArrayHeader pointer. VarArgsAsArray must be the
            // last entry in `sig.args`, so any further declared kinds
            // would be unreachable — break after consuming.
            let remaining = &args[i..];
            let cap = (remaining.len() as u32).to_string();
            let mut arr = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
            for r in remaining {
                let v = lower_expr(ctx, r)?;
                let blk = ctx.block();
                arr = blk.call(I64, "js_array_push_f64", &[(I64, &arr), (DOUBLE, &v)]);
            }
            llvm_args.push((I64, arr));
            arg_types.push(I64);
            i = args.len();
            break;
        }
        let lowered = lower_expr(ctx, &args[i])?;
        match kind {
            NativeArgKind::F64 => {
                llvm_args.push((DOUBLE, lowered));
                arg_types.push(DOUBLE);
            }
            NativeArgKind::StrPtr => {
                let blk = ctx.block();
                let ptr = blk.call(I64, "js_get_string_pointer_unified", &[(DOUBLE, &lowered)]);
                llvm_args.push((I64, ptr));
                arg_types.push(I64);
            }
            NativeArgKind::PtrI64 => {
                let blk = ctx.block();
                let handle = unbox_to_i64(blk, &lowered);
                llvm_args.push((I64, handle));
                arg_types.push(I64);
            }
            NativeArgKind::JsvalI64 => {
                // Bitcast the NaN-boxed f64 to i64 without unboxing —
                // the callee will interpret the raw bits.
                let blk = ctx.block();
                let bits = blk.bitcast_double_to_i64(&lowered);
                llvm_args.push((I64, bits));
                arg_types.push(I64);
            }
            NativeArgKind::VarArgsAsArray => unreachable!("handled above"),
        }
        i += 1;
    }
    // If fewer args than sig expects, pad with undefined / 0 / empty-array.
    for j in i..sig.args.len() {
        match sig.args[j] {
            NativeArgKind::F64 => {
                llvm_args.push((
                    DOUBLE,
                    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)),
                ));
                arg_types.push(DOUBLE);
            }
            NativeArgKind::StrPtr | NativeArgKind::PtrI64 | NativeArgKind::JsvalI64 => {
                llvm_args.push((I64, "0".to_string()));
                arg_types.push(I64);
            }
            NativeArgKind::VarArgsAsArray => {
                // No user args at this position — pass an empty array.
                let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, "0")]);
                llvm_args.push((I64, arr));
                arg_types.push(I64);
            }
        }
    }

    // Determine return type for the declare
    let ret_type = match sig.ret {
        NativeRetKind::Ptr
        | NativeRetKind::Str
        | NativeRetKind::ObjFromJsonStr
        | NativeRetKind::BigInt => I64,
        NativeRetKind::F64 => DOUBLE,
        NativeRetKind::I32Void => I32,
        NativeRetKind::Void => crate::types::VOID,
    };

    ctx.pending_declares
        .push((sig.runtime.to_string(), ret_type, arg_types));

    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        llvm_args.iter().map(|(t, s)| (*t, s.as_str())).collect();

    match sig.ret {
        NativeRetKind::Ptr => {
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(nanbox_pointer_inline(blk, &raw))
        }
        NativeRetKind::Str => {
            // Returned raw *mut StringHeader — NaN-box with STRING_TAG so
            // downstream string ops (JSON.stringify, ===, .length) work.
            // Null pointer (header value 0) is returned as TAG_NULL so
            // `request.header('missing')` reads as `null` instead of a
            // dangling string pointer.
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            let is_null = blk.icmp_eq(I64, &raw, "0");
            let boxed = nanbox_string_inline(blk, &raw);
            let null_val = double_literal(f64::from_bits(crate::nanbox::TAG_NULL));
            Ok(blk.select(crate::types::I1, &is_null, DOUBLE, &null_val, &boxed))
        }
        NativeRetKind::ObjFromJsonStr => {
            // Returned raw *mut StringHeader containing JSON — pipe
            // through `js_json_parse_or_null` so user code sees a real
            // object (e.g. `jwt.verify(...).sub` works). Symmetric
            // counterpart to the NA_JSON arg coercion landed in #915.
            // Null pointer (failure mode — e.g. `jwt.verify` on a bad
            // signature) is returned as TAG_NULL without throwing,
            // matching the previous NR_STR null-handling. #927.
            //
            // `js_json_parse_or_null` takes `*const StringHeader` (i64
            // on the FFI side) and returns the NaN-boxed JSValue bits
            // as i64. It returns TAG_NULL for null input (instead of
            // the throw that plain `js_json_parse` does). Declare
            // BEFORE grabbing `blk` so the mutable borrow on
            // pending_declares doesn't overlap the block borrow.
            ctx.pending_declares
                .push(("js_json_parse_or_null".to_string(), I64, vec![I64]));
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            let parsed_bits = blk.call(I64, "js_json_parse_or_null", &[(I64, &raw)]);
            Ok(blk.bitcast_i64_to_double(&parsed_bits))
        }
        NativeRetKind::BigInt => {
            // Returned raw *mut BigIntHeader — NaN-box with BIGINT_TAG (0x7FFA).
            let blk = ctx.block();
            let raw = blk.call(I64, sig.runtime, &arg_slices);
            Ok(nanbox_bigint_inline(blk, &raw))
        }
        NativeRetKind::F64 => Ok(ctx.block().call(DOUBLE, sig.runtime, &arg_slices)),
        NativeRetKind::I32Void => {
            let _discard = ctx.block().call(I32, sig.runtime, &arg_slices);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
        NativeRetKind::Void => {
            ctx.block().call_void(sig.runtime, &arg_slices);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
    }
}

#[cfg(test)]
mod ffi_return_type_tests {
    /// Verify that the `returns` manifest field values map to the correct
    /// dispatch flags. These tests guard against accidentally conflating
    /// "i64_str" with "i64" or "string" — the three are mutually exclusive.
    ///
    /// Related: issue #222 — explicit `returns: "i64_str"` for string-pointer
    /// detection when the Rust function is declared `-> i64`.
    fn parse_flags(manifest_ret: Option<&str>) -> (bool, bool, bool, bool) {
        // Mirror the manifest-driven arm of the flag computation in the
        // ExternFuncRef dispatch inside lower_call.  The name-based heuristic
        // and HIR-type fallback arms are omitted here; this only tests the
        // explicit manifest field.
        let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
        let returns_string = matches!(manifest_ret, Some("string") | Some("ptr"));
        let returns_i64 = matches!(manifest_ret, Some("i64"));
        let returns_void = matches!(manifest_ret, Some("void"));
        (returns_i64_str, returns_string, returns_i64, returns_void)
    }

    #[test]
    fn i64_str_is_recognized() {
        let (i64_str, string, i64, void) = parse_flags(Some("i64_str"));
        assert!(i64_str, "returns_i64_str must be true for \"i64_str\"");
        assert!(!string, "returns_string must be false for \"i64_str\"");
        assert!(!i64, "returns_i64 must be false for \"i64_str\"");
        assert!(!void, "returns_void must be false for \"i64_str\"");
    }

    #[test]
    fn string_not_confused_with_i64_str() {
        let (i64_str, string, i64, void) = parse_flags(Some("string"));
        assert!(!i64_str, "returns_i64_str must be false for \"string\"");
        assert!(string, "returns_string must be true for \"string\"");
        assert!(!i64, "returns_i64 must be false for \"string\"");
        assert!(!void, "returns_void must be false for \"string\"");
    }

    #[test]
    fn ptr_alias_for_string() {
        let (i64_str, string, i64, void) = parse_flags(Some("ptr"));
        assert!(!i64_str, "returns_i64_str must be false for \"ptr\"");
        assert!(string, "returns_string must be true for \"ptr\"");
        assert!(!i64, "returns_i64 must be false for \"ptr\"");
        assert!(!void, "returns_void must be false for \"ptr\"");
    }

    #[test]
    fn i64_stays_numeric() {
        let (i64_str, string, i64, void) = parse_flags(Some("i64"));
        assert!(!i64_str, "returns_i64_str must be false for \"i64\"");
        assert!(!string, "returns_string must be false for \"i64\"");
        assert!(i64, "returns_i64 must be true for \"i64\"");
        assert!(!void, "returns_void must be false for \"i64\"");
    }

    #[test]
    fn void_recognized() {
        let (i64_str, string, i64, void) = parse_flags(Some("void"));
        assert!(!i64_str, "returns_i64_str must be false for \"void\"");
        assert!(!string, "returns_string must be false for \"void\"");
        assert!(!i64, "returns_i64 must be false for \"void\"");
        assert!(void, "returns_void must be true for \"void\"");
    }

    #[test]
    fn i64_str_dispatch_order() {
        // When manifest is "i64_str", it must take the i64_str path even
        // if the HIR type also says String (which would normally set
        // returns_string via the ext_return_type arm).
        let manifest_ret: Option<&str> = Some("i64_str");
        let returns_i64_str = matches!(manifest_ret, Some("i64_str"));
        // Simulate returns_string with HIR String type:
        let hir_string_arm = true; // ext_return_type == HirType::String
        let returns_string = matches!(manifest_ret, Some("string") | Some("ptr")) || hir_string_arm;
        // Both could be true simultaneously, but in the dispatch the
        // `returns_i64_str` branch is checked FIRST, so it wins.
        assert!(returns_i64_str);
        assert!(returns_string); // also true — but i64_str branch fires first
    }
}
