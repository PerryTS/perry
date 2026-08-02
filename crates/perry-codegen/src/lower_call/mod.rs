//! Call, new, and native method call lowering.
//!
//! Contains `lower_call`, `lower_new`, and `lower_native_method_call`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{variant_name, FnCtx};

// Tier 1.3 (v0.5.332): the perry/ui, perry/ui-instance, perry/system,
// perry/i18n dispatch tables moved to `perry_dispatch` so the JS and
// WASM backends can derive their (TS-name → runtime-symbol) mapping
// from the same source of truth. Local aliases below preserve the
// pre-refactor type names used throughout this file.

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
// - `extern_func.rs` / `func_ref.rs` / `namespace_call.rs` /
//   `property_get.rs` / `console_promise.rs` / `early_branches.rs`
//   / `ui_tables.rs` / `closure_analysis.rs` /
//   `native_module_dispatch.rs` (#1105 followup): per-branch
//   extraction of the original `lower_call.rs`'s 4.3k-LOC body so
//   every file in this directory stays under 2000 lines.
mod atomics;
pub(crate) mod buffer_intrinsic;
mod builtin;
mod builtin_table_gate;
mod capture_writeback;
mod closure_analysis;
mod console_promise;
mod dataview_intrinsic;
mod early_branches;
mod event_target;
mod extern_func;
mod extern_timers;
mod field_init;
mod func_ref;
mod jsx;
mod method_override;
mod namespace_call;
mod native;
mod native_module_dispatch;
mod native_table;
mod new;
mod new_ctor_args;
mod new_helpers;
mod omitted_native_params;
mod options;
mod property_get;
mod scalar_method;
mod ui_styling;
mod ui_tables;
mod web_storage;

use buffer_intrinsic::try_emit_buffer_read_intrinsic;
use builtin::lower_builtin_new;
use jsx::try_rewrite_perry_tui_jsx_intrinsic;
// `options/` (#1099): the options-object-literal lowering family,
// split by native-API surface (notification / abort / fetch) under
// `options/`. Bring the per-surface entry points + shared helpers
// into this module's scope so the existing `super::<name>` call
// sites in sibling submodules (builtin/native/ui_styling) keep
// resolving unchanged after the split.
use options::{
    build_headers_from_object, get_raw_string_ptr, lower_fetch_native_method,
    lower_notification_schedule,
};
// `native_table.rs` (#1099): the ~5k-row `NATIVE_MODULE_TABLE` data +
// arg/ret kind types. The dispatch consumers below
// (`native_module_lookup`, `lower_native_module_dispatch`) live in
// `native_module_dispatch.rs` now and pull these in via `super::`.
use native_table::{NativeArgKind, NativeModSig, NativeRetKind, NATIVE_MODULE_TABLE};
use ui_styling::apply_inline_style;

// Re-export `ui_tables.rs` items under their pre-split `super::<name>`
// names so siblings (`native.rs`, `extern_func.rs`) keep resolving the
// table-lookup family and `lower_perry_ui_table_call` unchanged.
pub(super) use ui_tables::{
    lower_perry_ui_table_call, perry_audio_table_lookup, perry_background_table_lookup,
    perry_i18n_table_lookup, perry_media_table_lookup, perry_plugin_instance_method_lookup,
    perry_plugin_table_lookup, perry_system_table_lookup, perry_ui_instance_method_lookup,
    perry_ui_table_lookup, perry_updater_table_lookup,
};
// Same for `native_module_dispatch.rs` — `native.rs` consumes both
// `native_module_lookup` and `lower_native_module_dispatch` via
// `super::`.
pub(super) use native_module_dispatch::{lower_native_module_dispatch, native_module_lookup};
// And the closure-analysis helpers — `native.rs` uses them via
// `super::` for the perry/thread thread-safety check.
pub(super) use closure_analysis::{
    collect_closure_introduced_ids, find_outer_writes_stmt, find_thread_hazard_in_body,
    hazardous_module_global_ids, ThreadClosureHazard,
};

// Re-export pub(crate) so callers outside this module (e.g.
// `crate::expr::use crate::lower_call::lower_native_method_call;`)
// keep resolving — `pub(super)` on the native fn would shadow them.
pub(crate) use native::lower_native_method_call;
// Re-export pub(crate) `new.rs` items consumed outside this module
// (codegen.rs / expr.rs / stmt.rs) so `crate::lower_call::lower_new`
// etc. keep resolving after the split.
pub(crate) use field_init::{apply_field_initializers_recursive, FieldInitMode};
pub(crate) use new::{emit_class_capture_writeback, lower_new, lower_new_member_captured};
pub(crate) use new_ctor_args::{
    bind_inline_constructor_params, restore_inline_constructor_scope, CaptureFill,
};
// The derived-ctor no-super static-throw predicates (shared with the
// standalone-ctor-symbol path in `codegen/method.rs`, which is the default
// `new` lowering when `force_ctor_call` redirects construction to the shared
// symbol instead of inlining). Refs class/subclass/builtin-objects/*/
// super-must-be-called.
pub(crate) use new_helpers::{
    ctor_body_calls_super, ctor_body_closure_calls_super, ctor_body_has_value_return,
    ctor_body_uses_this,
};
// #6325 / #6326: the class-chain walk to a native base whose surface perry
// stamps onto the instance, plus its init emitter. Shared by the implicit
// (no-own-ctor) `new` path in `new.rs` and the explicit-`super()` arm in
// `expr/this_super_call.rs`, which are the two places a derived constructor can
// reach the base.
pub(crate) use new_helpers::{emit_native_instance_base_init, native_instance_base_in_chain};
// `extract_options_fields` is consumed by `expr.rs` as
// `crate::lower_call::extract_options_fields` — keep that path stable.
pub(crate) use options::extract_options_fields;
// `iter_native_module_table` is consumed by `lib.rs`'s public manifest
// API as `lower_call::iter_native_module_table` — keep that path stable.
pub(crate) use native_table::iter_native_module_table;

/// #7154: lower a direct call's argument list with each already-evaluated
/// argument protected across the evaluation of the ones that follow it.
///
/// An argument list is evaluated left to right, and every value produced so
/// far lives in a bare SSA register while the later ones are lowered. The
/// cross-module `perry_fn_<src>__<name>` path lowered the whole list in a
/// plain `for a in args` loop with no protection at all, so
/// `f(URL, "GET", {…}, Schema.array(), body => …)` — the `sfw-registry`
/// reproducer's `defineApiCall(…)` shape — leaves arguments 1 and 2 naming
/// pre-collection addresses the moment an evacuating minor lands in argument
/// 3, 4 or 5. It does: argument 3 is an object literal (allocates), argument 4
/// runs user code with its own loop back-edge polls, argument 5 allocates a
/// closure.
///
/// The two string literals are the case that faulted, and they are the *cheap*
/// case to fix. A literal lowers to a load of a `__perry_init_strings_*` handle
/// global, which IS a registered root — so the string is never swept — but an
/// evacuating cycle REWRITES that global while the register keeps the pre-move
/// address. [`OperandProtection::Reload`] re-emits the load below the collection
/// point and costs no runtime call at all. Measured at the fault: the handle
/// global held the post-move address, the register held the retired from-space
/// one.
///
/// [`temp_root::lower_exprs_rooted`] gates each argument on
/// `any_later_ref_may_trigger_gc`, so an argument list nothing allocating
/// follows emits exactly the IR it emitted before.
///
/// Returns the values to pass and the guard for [`emit_rooted_call`].
///
/// [`OperandProtection::Reload`]: crate::expr::temp_root
/// [`temp_root::lower_exprs_rooted`]: crate::expr::temp_root::lower_exprs_rooted
pub(crate) fn lower_call_args_rooted(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
) -> Result<(Vec<String>, Option<String>)> {
    let refs: Vec<&Expr> = args.iter().collect();
    crate::expr::temp_root::lower_exprs_rooted(ctx, &refs)
}

/// Emit a direct call over an already-lowered argument list, then release the
/// [`lower_call_args_rooted`] guard.
///
/// The release has to sit BELOW the call, not above it: the callee allocates
/// while reading these arguments, so the slots have to outlive the call itself.
pub(crate) fn emit_rooted_call(
    ctx: &mut FnCtx<'_>,
    fname: &str,
    lowered: &[String],
    guard: Option<String>,
) -> String {
    let arg_slices: Vec<(crate::types::LlvmType, &str)> = lowered
        .iter()
        .map(|s| (crate::types::DOUBLE, s.as_str()))
        .collect();
    let result = ctx.block().call(crate::types::DOUBLE, fname, &arg_slices);
    crate::expr::temp_root::temp_root_release(ctx, guard);
    result
}

/// One array a rest/`arguments` call has to materialize from its argument
/// list: every argument from `from` onwards, optionally flagged as an
/// `arguments` object.
///
/// A call needs one of these for a real `...rest` binding, one for a synthetic
/// `arguments` param — and #1816's shape needs BOTH, from different offsets
/// over the same already-lowered arguments.
pub(crate) struct RestBundle {
    /// First argument index to bundle. `0` for a synthetic `arguments`
    /// (which must reflect ALL passed args); `fixed_count` for a real rest.
    pub from: usize,
    /// Emit `js_array_mark_arguments_object` over the finished array.
    pub mark_arguments_object: bool,
}

/// #7154: lower a rest/`arguments` call's argument list with every value
/// protected across the array construction that follows it.
///
/// This is [`lower_call_args_rooted`]'s twin for the rest path, and it is a
/// separate function because the hazard is strictly larger. #7240 fixed only
/// the non-rest arm; the rest arm has **two** unprotected registers, not one:
///
///  1. **The fixed parameters**, exactly as in the non-rest arm — except their
///     window does not end when the last argument is lowered. The rest array
///     is materialized *afterwards*, and materializing it runs
///     `js_array_alloc` plus one `js_array_push_f64` per trailing argument,
///     every one of which allocates. So `lower_exprs_rooted` is the wrong tool
///     here: it re-reads immediately, and the collection point it must re-read
///     below is a step it never sees. [`RootedOperands`] exists for precisely
///     that, and the caller picking the re-read point is the whole difference.
///
///  2. **The accumulator itself** — and this is the one with no analogue in
///     the non-rest arm. `current` is a RAW `*mut ArrayHeader` in a bare SSA
///     register, threaded through the push loop, holding the ONLY reference to
///     every argument pushed so far while the NEXT argument's expression is
///     lowered — arbitrary user code. Nothing roots it, so a minor in that
///     window does not merely move the array, it is free to sweep it.
///     [`temp_root::rooted_array_begin`]'s doc names this exact shape as "the
///     shape behind every variadic / spread / rest argument list"; the helper
///     has existed since #6951 and this path never adopted it.
///
/// `collects` is unconditionally true for the fixed parameters, and that is a
/// statement about the code rather than a conservative shrug: the rest array
/// is materialized on every path (a callee's rest binding must be `[]` even
/// when nothing trailing was passed), so `js_array_alloc` is always between a
/// fixed parameter and the call that consumes it. Scalar arguments still cost
/// nothing — [`temp_root::operand_protection`] routes anything
/// `expr_is_known_non_pointer_shadow_value` proves is not a heap reference to
/// `Reuse`, so `f(1, 2, ...rest)` emits the IR it emitted before.
///
/// Returns the values to pass — fixed parameters first, re-read from their
/// roots, then one boxed array per [`RestBundle`] — and the guard for
/// [`emit_rooted_call`].
///
/// [`RootedOperands`]: crate::expr::temp_root::RootedOperands
/// [`temp_root::rooted_array_begin`]: crate::expr::temp_root::rooted_array_begin
/// [`temp_root::operand_protection`]: crate::expr::temp_root::operand_protection
pub(crate) fn lower_rest_call_args_rooted(
    ctx: &mut FnCtx<'_>,
    args: &[Expr],
    fixed_count: usize,
    bundles: &[RestBundle],
) -> Result<(Vec<String>, Option<String>)> {
    use crate::expr::temp_root;
    use crate::types::I64;

    let refs: Vec<&Expr> = args.iter().collect();
    // Incrementally, one argument at a time: root each BEFORE the next is
    // lowered. Lowering the whole list and rooting it afterwards is not merely
    // late, it is worse than doing nothing — by then an earlier value may
    // already have been swept and the push publishes a dangling pointer into a
    // slot the collector scans. See `root_operands_begin`.
    let mut rooted = temp_root::root_operands_begin(refs.len());
    for expr in &refs {
        let value = crate::expr::lower_expr(ctx, expr)?;
        rooted.push(ctx, expr, &value, true);
    }

    let mut lowered: Vec<String> = Vec::with_capacity(fixed_count + bundles.len());

    // Build every array FIRST and leave each one in its temp-root slot, then
    // read them all back at the end. Building array 2 allocates, so array 1's
    // pointer must not be sitting in a bare register while it happens — #1816's
    // shape wants both a `...rest` and an `arguments` bundle over the same
    // list, and that second `js_array_alloc` is a collection point for the
    // first array exactly as the push loop is for its elements.
    let mut accs: Vec<String> = Vec::with_capacity(bundles.len());
    for bundle in bundles {
        let cap = (args.len().saturating_sub(bundle.from) as u32).to_string();
        let acc = temp_root::rooted_array_begin(ctx, &cap);
        for i in bundle.from..refs.len() {
            // Re-read per element: the previous push allocated, so the register
            // this argument was lowered into is already stale.
            let value = rooted.reread_one(ctx, &refs, i)?;
            temp_root::temp_rooted_array_push(ctx, &acc, &value);
        }
        accs.push(acc);
    }

    // Below every allocation now. `js_array_mark_arguments_object` only sets a
    // flag bit and hands the same pointer back (`array/header.rs:1046`), so it
    // is safe between the slot read and the box.
    let mut boxed_bundles: Vec<String> = Vec::with_capacity(bundles.len());
    for (bundle, acc) in bundles.iter().zip(accs.iter()) {
        let mut current = temp_root::rooted_array_read(ctx, acc);
        if bundle.mark_arguments_object {
            current = ctx
                .block()
                .call(I64, "js_array_mark_arguments_object", &[(I64, &current)]);
        }
        boxed_bundles.push(crate::expr::nanbox_pointer_inline(ctx.block(), &current));
    }

    let undefined_lit =
        crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    for i in 0..fixed_count {
        lowered.push(if i < refs.len() {
            rooted.reread_one(ctx, &refs, i)?
        } else {
            undefined_lit.clone()
        });
    }
    lowered.extend(boxed_bundles);

    // The operand group was pushed BEFORE the accumulators, so its guard is the
    // lower index and one truncate at it drops both. When nothing needed a real
    // root the first accumulator is the lowest slot and becomes the guard.
    let guard = rooted.guard().or_else(|| accs.first().cloned());
    Ok((lowered, guard))
}

/// Lower a `Call` expression. Two shapes are supported:
/// 1. `FuncRef(id)(args...)` — direct call to a user function by HIR id.
/// 2. `console.log(expr)` where `expr` lowers to a double — emits a
///    `js_console_log_number` call and returns `0.0` as the statement value.
pub(crate) fn lower_call(ctx: &mut FnCtx<'_>, callee: &Expr, args: &[Expr]) -> Result<String> {
    // #5253: localize the `ReferenceError: X is not defined` thrown by the
    // unresolved-identifier runtime helper. A bare unresolved identifier
    // (`module`, winston's stray globals, …) lowers to
    // `Call { callee: ExternFuncRef("js_global_get_or_throw_unresolved"),
    // args: [name], byte_offset }` (perry-hir lower_expr). That helper throws a
    // ReferenceError through `js_referenceerror_new` → `make_stack`, which reads
    // `CURRENT_CALL_LOCATION`. Emit a `js_set_call_location` from the call's
    // recorded byte offset (set on `pending_call_offset` by the `Expr::Call`
    // dispatcher) before lowering the call, so the throw carries `at file:line`.
    // No-op in the default build; offset 0 (synthesized refs) resolves to none.
    if let Expr::ExternFuncRef { name, .. } = callee {
        if name == "js_global_get_or_throw_unresolved" {
            let off = ctx.strings.pending_call_offset();
            crate::expr::calls::emit_call_location_at(ctx, off);
        }
    }

    // #3656: `p.call(thisArg, …)` / `p.apply(thisArg, argsArray)` on a Proxy
    // routes through the proxy's `[[Call]]` (apply trap) rather than reading
    // `.call`/`.apply` off the forwarded target.
    if let Some(v) = crate::expr::proxy_reflect::try_lower_proxy_fn_call_apply(ctx, callee, args)? {
        return Ok(v);
    }

    // #5196: `proxy.method(args)` (e.g. `proxyArray.map(fn)`) — the fused
    // member-call form. Route through `js_native_call_method` so `this` binds
    // to the proxy and array methods iterate it through its `get` trap.
    if let Some(v) = crate::expr::proxy_reflect::try_lower_proxy_method_call(ctx, callee, args)? {
        return Ok(v);
    }

    // Early-firing branches (#1113 chained native method call, computed
    // `obj[str](...)`, CurrentStepClosure, closure-typed local).
    if let Some(v) = early_branches::try_lower_native_chain_method_call(ctx, callee, args)? {
        return Ok(v);
    }
    if let Some(v) = early_branches::try_lower_index_get_call(ctx, callee, args)? {
        return Ok(v);
    }
    if let Some(v) = early_branches::try_lower_current_step_closure_call(ctx, callee, args)? {
        return Ok(v);
    }
    if let Some(v) = early_branches::try_lower_closure_typed_local_call(ctx, callee, args)? {
        return Ok(v);
    }

    // Namespace member call (#636) — `ns.foo(...)` where `ns` is an
    // ExternFuncRef namespace import.
    if let Some(v) = namespace_call::try_lower_namespace_member_call(ctx, callee, args)? {
        return Ok(v);
    }

    // `Atomics.load(...)` / `Atomics.add(...)` and related namespace statics.
    if let Some(v) = atomics::try_lower_atomics_static_call(ctx, callee, args)? {
        return Ok(v);
    }

    // User function call via FuncRef.
    if let Some(v) = func_ref::try_lower_func_ref_call(ctx, callee, args)? {
        return Ok(v);
    }

    // Cross-module function call via ExternFuncRef.
    if let Some(v) = extern_func::try_lower_extern_func_call(ctx, callee, args)? {
        return Ok(v);
    }

    // Scalar-replaced exact receiver method summaries, e.g.
    // `let p = new Point(x, y); p.sum()` where `sum` is proven to only read
    // numeric `this` fields. Must run before generic property-get method
    // dispatch, which requires a heap receiver.
    if let Some(v) = scalar_method::try_lower_scalar_replaced_method_call(ctx, callee, args)? {
        return Ok(v);
    }

    // String / array / class / Map / Set / Promise / fetch / static /
    // instance method dispatch — the big PropertyGet branch.
    if let Some(v) = property_get::try_lower_property_get_method_call(ctx, callee, args)? {
        return Ok(v);
    }

    // console.log / console.warn / console.error / …
    if let Some(v) = console_promise::try_lower_console_call(ctx, callee, args)? {
        return Ok(v);
    }

    // Promise.resolve / .reject / .all / .race / .allSettled +
    // Array.fromAsync.
    if let Some(v) = console_promise::try_lower_promise_static_call(ctx, callee, args)? {
        return Ok(v);
    }

    // `recv.method(args)` via `js_native_call_method` — catches
    // Map/Set/RegExp/Buffer methods on plain object fields.
    if let Some(v) = console_promise::try_lower_native_method_str_dispatch(ctx, callee, args)? {
        return Ok(v);
    }

    // Final fallthrough: closure-call via `js_closure_call<N>`.
    if let Some(v) = console_promise::try_lower_closure_call_fallthrough(ctx, callee, args)? {
        return Ok(v);
    }

    bail!(
        "perry-codegen: Call callee shape not supported ({}) with {} args",
        variant_name(callee),
        args.len()
    )
}
