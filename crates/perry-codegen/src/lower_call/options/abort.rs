//! `AbortController` / `AbortSignal` method-call lowering.
//!
//! Extracted from `lower_call.rs` (#1099, part of #1097) — pure move,
//! no behavior change. `is_abort_controller_expr` is the static-type
//! probe; `lower_abort_controller_call` handles `controller.abort(...)`,
//! `controller.signal.addEventListener("abort", cb)`, and
//! `AbortSignal.timeout(ms)`.

use anyhow::Result;
use perry_hir::Expr;
use perry_types::Type as HirType;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I64};

/// Returns `true` if the expression statically resolves to an
/// `AbortController`-typed value (either a local whose declared type
/// is `Named("AbortController")` or a `new AbortController()` call).
pub(in crate::lower_call) fn is_abort_controller_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    match e {
        Expr::New { class_name, .. } => class_name == "AbortController",
        Expr::LocalGet(id) => matches!(
            ctx.local_types.get(id),
            Some(HirType::Named(n)) if n == "AbortController"
        ),
        _ => false,
    }
}

/// Returns `true` if the expression statically resolves to an
/// `AbortSignal`-typed value: a local declared `Named("AbortSignal")`.
pub(in crate::lower_call) fn is_abort_signal_typed_expr(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    matches!(
        e,
        Expr::LocalGet(id)
            if matches!(ctx.local_types.get(id), Some(HirType::Named(n)) if n == "AbortSignal")
    )
}

/// Lower AbortController / AbortSignal method calls:
/// - `controller.abort(reason?)`
/// - `controller.signal.addEventListener("abort", cb)`
/// - `controller.signal.throwIfAborted()` / `signal.throwIfAborted()`
/// - `AbortSignal.timeout(ms)` (static)
///
/// Returns `None` if the call shape doesn't match one of the handled
/// patterns — caller falls through to the generic dispatch.
pub(in crate::lower_call) fn lower_abort_controller_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
) -> Result<Option<String>> {
    // ── controller.signal.throwIfAborted() / signal.throwIfAborted() ──
    if property == "throwIfAborted" {
        // Case 1: `<controller>.signal.throwIfAborted()` — derive the signal
        // pointer from the controller.
        if let Expr::PropertyGet {
            object: inner_obj,
            property: inner_prop,
            ..
        } = object
        {
            if inner_prop == "signal" && is_abort_controller_expr(ctx, inner_obj) {
                let ctrl_box = lower_expr(ctx, inner_obj)?;
                let blk = ctx.block();
                let ctrl_handle = unbox_to_i64(blk, &ctrl_box);
                let signal_handle =
                    blk.call(I64, "js_abort_controller_signal", &[(I64, &ctrl_handle)]);
                blk.call_void("js_abort_signal_throw_if_aborted", &[(I64, &signal_handle)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
        }
        // Case 2: receiver is itself an AbortSignal value (a local typed
        // `AbortSignal`, or the result of `AbortSignal.abort/timeout/any`).
        if is_abort_signal_typed_expr(ctx, object) {
            let sig_box = lower_expr(ctx, object)?;
            let blk = ctx.block();
            let sig_handle = unbox_to_i64(blk, &sig_box);
            blk.call_void("js_abort_signal_throw_if_aborted", &[(I64, &sig_handle)]);
            return Ok(Some(double_literal(f64::from_bits(
                crate::nanbox::TAG_UNDEFINED,
            ))));
        }
    }
    // ── AbortSignal.timeout(ms) static ──
    if property == "timeout" {
        if let Expr::GlobalGet(_) = object {
            // Can't distinguish AbortSignal.timeout from other globals
            // without more context — skip.
        }
    }
    // Static `AbortSignal.timeout(ms)` — matched via a PropertyGet on a
    // GlobalGet-shaped object isn't quite right because GlobalGet has
    // no name; best we can do is detect by property name "timeout" and
    // the local-isn't-a-known-thing. Skip for now.

    // ── controller.abort(reason?) ──
    if property == "abort" && is_abort_controller_expr(ctx, object) {
        let recv_box = lower_expr(ctx, object)?;
        let blk = ctx.block();
        let ctrl_handle = unbox_to_i64(blk, &recv_box);
        if args.is_empty() {
            blk.call_void("js_abort_controller_abort", &[(I64, &ctrl_handle)]);
        } else {
            let reason = lower_expr(ctx, &args[0])?;
            let blk = ctx.block();
            blk.call_void(
                "js_abort_controller_abort_reason",
                &[(I64, &ctrl_handle), (DOUBLE, &reason)],
            );
        }
        return Ok(Some(double_literal(f64::from_bits(
            crate::nanbox::TAG_UNDEFINED,
        ))));
    }

    // ── controller.signal.addEventListener("abort", cb) ──
    if property == "addEventListener" && args.len() >= 2 {
        if let Expr::PropertyGet {
            object: inner_obj,
            property: inner_prop,
            ..
        } = object
        {
            if inner_prop == "signal" && is_abort_controller_expr(ctx, inner_obj) {
                let ctrl_box = lower_expr(ctx, inner_obj)?;
                let blk = ctx.block();
                let ctrl_handle = unbox_to_i64(blk, &ctrl_box);
                // Get the signal pointer.
                let signal_handle =
                    blk.call(I64, "js_abort_controller_signal", &[(I64, &ctrl_handle)]);
                let evt = lower_expr(ctx, &args[0])?;
                let listener = lower_expr(ctx, &args[1])?;
                let blk = ctx.block();
                blk.call_void(
                    "js_abort_signal_add_listener",
                    &[(I64, &signal_handle), (DOUBLE, &evt), (DOUBLE, &listener)],
                );
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
        }
    }

    Ok(None)
}
