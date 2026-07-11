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
mod buffer_intrinsic;
mod builtin;
mod capture_writeback;
mod closure_analysis;
mod console_promise;
mod early_branches;
mod event_target;
mod extern_func;
mod field_init;
mod func_ref;
mod jsx;
mod method_override;
mod namespace_call;
mod native;
mod native_module_dispatch;
mod native_table;
mod new;
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
use event_target::lower_event_target_call;
use jsx::try_rewrite_perry_tui_jsx_intrinsic;
use method_override::{emit_guarded_direct_method_call, emit_own_method_override_check};
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
pub(crate) use new::{
    bind_inline_constructor_params, emit_class_capture_writeback, lower_new,
    lower_new_member_captured, restore_inline_constructor_scope, CaptureFill,
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
// `extract_options_fields` is consumed by `expr.rs` as
// `crate::lower_call::extract_options_fields` — keep that path stable.
pub(crate) use options::extract_options_fields;
// `iter_native_module_table` is consumed by `lib.rs`'s public manifest
// API as `lower_call::iter_native_module_table` — keep that path stable.
pub(crate) use native_table::iter_native_module_table;

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
