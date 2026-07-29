//! Precise rooting for expression temporaries (#6951).
//!
//! The shadow stack roots named locals. It has no slot for the values that
//! only exist between two instructions, and an LLVM SSA register is not a GC
//! root — so an accumulator array, or an already-evaluated operand waiting for
//! its sibling, dies if the sibling's evaluation collects. Conservative native
//! stack scanning hid that (see `perry-runtime/src/gc/roots/temp_roots.rs`);
//! with `PERRY_CONSERVATIVE_STACK_SCAN=off` it is a live use-after-free.
//!
//! The emission contract, in the order it must appear:
//!
//! ```text
//! %idx = call i32 @js_gc_temp_root_push(i64 <bits>)   ; before the collection point
//! ...                                                  ; anything that may collect
//! %v   = call i64 @js_gc_temp_root_get(i32 %idx)       ; ALWAYS re-read
//!        call void @js_gc_temp_root_truncate(i32 %idx) ; after the last use
//! ```
//!
//! Re-reading is mandatory, not defensive: the slot is a *mutable* root, so an
//! evacuating cycle rewrites it and the register pushed beforehand is stale.
//! That is also why this is preferable to widening conservative scanning —
//! conservative roots have to pin, precise ones can move.

use perry_hir::Expr;

use crate::types::{DOUBLE, I32, I64};

use super::FnCtx;

/// Push `value_i64` (a bare heap pointer or NaN-boxed bits) and return the
/// slot-index register.
pub(crate) fn temp_root_push_i64(ctx: &mut FnCtx<'_>, value_i64: &str) -> String {
    ctx.block()
        .call(I32, "js_gc_temp_root_push", &[(I64, value_i64)])
}

/// Push a NaN-boxed `double` temporary and return the slot-index register.
pub(crate) fn temp_root_push_double(ctx: &mut FnCtx<'_>, value: &str) -> String {
    let bits = ctx.block().bitcast_double_to_i64(value);
    temp_root_push_i64(ctx, &bits)
}

/// Re-read slot `idx` as a raw `i64`.
pub(crate) fn temp_root_get_i64(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    ctx.block().call(I64, "js_gc_temp_root_get", &[(I32, idx)])
}

/// Re-read slot `idx` as a NaN-boxed `double`.
pub(crate) fn temp_root_get_double(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    let bits = temp_root_get_i64(ctx, idx);
    ctx.block().bitcast_i64_to_double(&bits)
}

/// Drop slot `idx` and everything pushed above it.
pub(crate) fn temp_root_truncate(ctx: &mut FnCtx<'_>, idx: &str) {
    ctx.block()
        .call_void("js_gc_temp_root_truncate", &[(I32, idx)]);
}

/// Push `value` onto the array held in temp-root slot `idx`, writing the
/// possibly-reallocated array pointer back into the slot.
pub(crate) fn temp_rooted_array_push(ctx: &mut FnCtx<'_>, idx: &str, value: &str) {
    ctx.block().call_void(
        "js_array_push_f64_temp_rooted",
        &[(I32, idx), (DOUBLE, value)],
    );
}

/// Allocate an argument-accumulator array and root it, returning the
/// temp-root slot index.
///
/// This is the shape behind every variadic / spread / rest argument list:
/// `js_array_alloc(n)`, then one `js_array_push_f64` per argument, with the
/// accumulator threaded through in an SSA register. That register held the
/// only reference to everything pushed so far — including argument 0 — across
/// the evaluation of argument 1, which is exactly the #6951 repro
/// (`console.log("label", allocatingCall())`).
///
/// Pair with [`temp_rooted_array_push`] per argument, then
/// [`rooted_array_read`] and [`temp_root_truncate`] — in that order, so the
/// array stays rooted across the call that consumes it.
pub(crate) fn rooted_array_begin(ctx: &mut FnCtx<'_>, cap: &str) -> String {
    let arr = ctx.block().call(I64, "js_array_alloc", &[(I32, cap)]);
    temp_root_push_i64(ctx, &arr)
}

/// Read the accumulator back out of its temp-root slot. Does NOT truncate:
/// callers truncate after the consuming call, so the array is still rooted
/// while the consumer runs (formatting an argument list allocates).
pub(crate) fn rooted_array_read(ctx: &mut FnCtx<'_>, idx: &str) -> String {
    temp_root_get_i64(ctx, idx)
}

/// Can lowering `expr` reach a collection point?
///
/// Deliberately one-sided: `false` must mean "provably allocates nothing", and
/// everything unrecognized answers `true`. A wrong `false` is a
/// use-after-free; a wrong `true` costs two runtime calls on a cold path.
pub(crate) fn expr_may_trigger_gc(expr: &Expr) -> bool {
    match expr {
        // Immediates and plain slot reads. `LocalGet` reads an alloca,
        // `GlobalGet` a module global — neither allocates.
        Expr::Undefined
        | Expr::Null
        | Expr::Bool(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_) => false,
        // A string literal is materialized once into a module-global handle by
        // `__perry_init_strings_*` and registered as a GC root there; the use
        // site is a load.
        Expr::String(_) => false,
        Expr::Unary { operand, .. } => expr_may_trigger_gc(operand),
        Expr::Compare { left, right, .. } => {
            expr_may_trigger_gc(left) || expr_may_trigger_gc(right)
        }
        // `+` on unknown operands can be string concatenation, which allocates;
        // every other binary operator is numeric or bitwise.
        Expr::Binary {
            op, left, right, ..
        } => {
            matches!(op, perry_hir::BinaryOp::Add)
                || expr_may_trigger_gc(left)
                || expr_may_trigger_gc(right)
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            expr_may_trigger_gc(condition)
                || expr_may_trigger_gc(then_expr)
                || expr_may_trigger_gc(else_expr)
        }
        Expr::Sequence(exprs) => exprs.iter().any(expr_may_trigger_gc),
        _ => true,
    }
}

/// Does any expression after index `i` reach a collection point?
///
/// This is the gate for protecting argument `i`: a value that nothing
/// allocating follows cannot be collected before it is consumed, so the
/// rooting calls would be pure overhead. `"a" + i`, `f(x, y)` on plain locals
/// and `[1, 2, 3]` therefore emit exactly the IR they emitted before #6951.
pub(crate) fn any_later_arg_may_trigger_gc(args: &[Expr], i: usize) -> bool {
    args.iter().skip(i + 1).any(expr_may_trigger_gc)
}

fn any_later_ref_may_trigger_gc(exprs: &[&Expr], i: usize) -> bool {
    exprs.iter().skip(i + 1).any(|e| expr_may_trigger_gc(e))
}

/// Lower `exprs` left to right, keeping each already-evaluated value precisely
/// rooted across the evaluation of the ones that follow (#6951).
///
/// Returns the lowered values — **re-read from their roots**, so they are
/// valid after an evacuating cycle — and the guard index the caller must pass
/// to [`temp_root_release`] once the consuming call has run. `None` means
/// nothing needed protecting and no runtime calls were emitted.
pub(crate) fn lower_exprs_rooted(
    ctx: &mut FnCtx<'_>,
    exprs: &[&Expr],
) -> anyhow::Result<(Vec<String>, Option<String>)> {
    let mut values = Vec::with_capacity(exprs.len());
    let mut slots: Vec<Option<String>> = Vec::with_capacity(exprs.len());
    let mut guard: Option<String> = None;
    for (i, expr) in exprs.iter().enumerate() {
        let value = super::lower_expr(ctx, expr)?;
        if any_later_ref_may_trigger_gc(exprs, i) {
            let idx = temp_root_push_double(ctx, &value);
            // The FIRST slot pushed is the guard: truncating it drops every
            // slot above it too, so one call releases the whole group.
            if guard.is_none() {
                guard = Some(idx.clone());
            }
            slots.push(Some(idx));
        } else {
            slots.push(None);
        }
        values.push(value);
    }
    for (value, slot) in values.iter_mut().zip(slots.iter()) {
        if let Some(idx) = slot {
            *value = temp_root_get_double(ctx, idx);
        }
    }
    Ok((values, guard))
}

/// Lower a `left`/`right` operand pair with the same contract as
/// [`lower_exprs_rooted`].
pub(crate) fn lower_operand_pair_rooted(
    ctx: &mut FnCtx<'_>,
    left: &Expr,
    right: &Expr,
) -> anyhow::Result<(String, String, Option<String>)> {
    let (mut values, guard) = lower_exprs_rooted(ctx, &[left, right])?;
    let right_value = values.pop().expect("pair lowering yields two values");
    let left_value = values.pop().expect("pair lowering yields two values");
    Ok((left_value, right_value, guard))
}

/// Release a guard returned by [`lower_exprs_rooted`]. Call it *after* the
/// consuming call, not before: the consumer allocates while reading these
/// values.
pub(crate) fn temp_root_release(ctx: &mut FnCtx<'_>, guard: Option<String>) {
    if let Some(idx) = guard {
        temp_root_truncate(ctx, &idx);
    }
}

/// A freshly allocated container handle (object, array, …) that generated code
/// keeps writing into while it lowers the initializer expressions.
///
/// The handle is a raw `i64` in an SSA register, and every initializer that
/// allocates is a chance for the half-built container to be swept out from
/// under it — the object-literal form of the #6951 accumulator bug. Re-read
/// the handle through [`rooted_handle_get`] before every use.
pub(crate) struct RootedHandle {
    slot: Option<String>,
    value: String,
}

/// Root `handle` when `protect` says an upcoming initializer can collect.
/// `protect == false` emits nothing and [`rooted_handle_get`] hands the
/// original register straight back, so unprotected sites keep their old IR.
pub(crate) fn rooted_handle_begin(
    ctx: &mut FnCtx<'_>,
    handle_i64: &str,
    protect: bool,
) -> RootedHandle {
    let slot = protect.then(|| temp_root_push_i64(ctx, handle_i64));
    RootedHandle {
        slot,
        value: handle_i64.to_string(),
    }
}

pub(crate) fn rooted_handle_get(ctx: &mut FnCtx<'_>, handle: &RootedHandle) -> String {
    match &handle.slot {
        Some(idx) => {
            let idx = idx.clone();
            temp_root_get_i64(ctx, &idx)
        }
        None => handle.value.clone(),
    }
}

pub(crate) fn rooted_handle_release(ctx: &mut FnCtx<'_>, handle: RootedHandle) {
    if let Some(idx) = handle.slot {
        temp_root_truncate(ctx, &idx);
    }
}

/// Do any of an object literal's / call's initializer expressions collect?
pub(crate) fn any_may_trigger_gc<'a>(exprs: impl IntoIterator<Item = &'a Expr>) -> bool {
    exprs.into_iter().any(expr_may_trigger_gc)
}
