//! Global identifiers and global-object property reads use ordinary shape sites.

use super::*;

use anyhow::Result;

/// Emit the runtime installer a global VALUE read needs (binary size).
///
/// The `process` / `console` namespaces exist from realm bootstrap, but their
/// dynamic-dispatch buckets (and `process`'s stdio stream objects) are
/// installed only on demand, so a program that uses them purely through
/// member intrinsics (`console.log(…)`, `process.argv`) links none of that
/// surface. Every site that can hand one of those objects to user code as a
/// value — the bare global, a method read as a value, or the global object
/// itself (`globalThis` / `global` / `self` / `globalThis[k]`) — runs the
/// matching installer first. Idempotent and cheap (a few relaxed stores).
pub(crate) fn emit_global_value_installs(ctx: &mut FnCtx<'_>, name: &str) {
    let sym = match name {
        "process" => "js_nm_install_process",
        "console" => "js_nm_install_console",
        // `performance` is the `perf_hooks` namespace object: `const p = performance;
        // p.now()` dispatches through the perf bucket, and a call on a module whose
        // bucket is not installed is an unsupported call, not a silent `undefined`.
        "performance" => "js_nm_install_perf",
        "globalThis" | "global" | "self" | "window" | "frames" => {
            "js_install_global_value_surfaces"
        }
        _ => return,
    };
    ctx.block().call_void(sym, &[]);
}

/// A builtin global identifier read as a VALUE (`Object`, `Array`, `Date`
/// ...): a static read site of the global object for the name's pooled key.
///
/// The global object is an ordinary object whose own keys are the builtins,
/// so the identifier is `globalThis[name]` with a compile-time key: the
/// generic read site's per-site cache compares the global object's ShapeId
/// and loads the slot, with the key's atom (its pool handle) for the miss.
/// It used to call `js_get_global_this_builtin_value(bytes, len)`, which
/// validated the name's UTF-8, interned it and ran a by-name `[[Get]]` on
/// every evaluation. Reassigning the global (`globalThis.Object = X`) is a
/// store to that object, so the next read sees it: the slot is loaded on
/// every hit, and a key added or deleted changes the ShapeId.
fn global_object_expr() -> Expr {
    Expr::Call {
        callee: Box::new(Expr::ExternFuncRef {
            name: "js_get_global_this".to_string(),
            param_types: Vec::new(),
            return_type: perry_hir::types::Type::Any,
        }),
        args: Vec::new(),
        type_args: Vec::new(),
        byte_offset: 0,
    }
}

pub(crate) fn lower_global_builtin_read(ctx: &mut FnCtx<'_>, name: &str) -> Result<String> {
    super::generic_dispatch::lower_global_property_get(ctx, &global_object_expr(), name, true)
}

/// `typeof` of a global reference permits an absent binding.
pub(crate) fn lower_optional_global_read(ctx: &mut FnCtx<'_>, name: &str) -> Result<String> {
    super::generic_dispatch::lower_global_property_get(ctx, &global_object_expr(), name, false)
}

pub(crate) fn lower_globalget_property(ctx: &mut FnCtx<'_>, property: &str) -> Result<String> {
    emit_global_value_installs(ctx, property);
    // GlobalGet is the global object. A builtin member value has a nested
    // receiver read (globalThis.Object, globalThis.Buffer, etc.) and uses the
    // ordinary property path on that value. Never select a builtin here by
    // the member's name: globalThis.isBuffer is an ordinary missing property.
    if is_global_this_builtin_name(property) {
        lower_global_builtin_read(ctx, property)
    } else {
        lower_optional_global_read(ctx, property)
    }
}
