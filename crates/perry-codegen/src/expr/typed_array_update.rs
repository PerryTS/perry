//! `ta[i]++` / `ta[i]--` / `++ta[i]` / `--ta[i]` on a typed-array receiver.
//!
//! The generic `Expr::IndexUpdate` lowering (`member_update.rs`) is four
//! out-of-line calls per update: `js_dyn_index_get`, `js_to_numeric`,
//! `js_numeric_step` and `js_put_value_set`. That is right for an arbitrary
//! receiver, but a typed-array element is a Number by construction, and the
//! explicit `ta[i] = ta[i] - 1` already takes the guarded inline element read
//! and store. On fannkuch's `count[r]--` the difference was 21.5% of the
//! whole program's instructions.
//!
//! This lowering keeps the update's semantics and evaluates the receiver and
//! the index exactly once:
//!
//! ```text
//! old = <guarded inline element read>(obj, idx)   ; exit: complete [[Get]]
//! if old is a Number:                             ; every typed-array hit
//!     new = old + 1 | old - 1                     ; Number::add / subtract
//!     <guarded inline element store>(obj, idx, new)  ; exit: complete [[Set]]
//! else:                                           ; BigInt, a lying annotation
//!     old = ToNumeric(old); new = step(old)       ; the generic steps
//!     js_dyn_index_set_strict(obj, idx, new)      ; complete [[Set]]
//! result = prefix ? new : old
//! ```
//!
//! Both halves of the fast arm are the ones the explicit form uses, so the
//! element conversion (Int32 wraps, Uint8Clamped clamps, Float64 stores the
//! double) is the store's own: the inline store only admits the truncating
//! integer kinds and the float kinds, and everything else — clamped, Float16,
//! BigInt, a view, out of bounds, a receiver that is not a typed array at all
//! — leaves through the runtime `[[Set]]`. The expression's value is the
//! Number `new` (prefix) or `old` (postfix), not the converted element, which
//! is what the language specifies.
//!
//! `old` is tested with `JSValue::is_number`'s range. A Number needs no
//! `ToNumeric`, and `fadd old, 1.0` / `fsub old, 1.0` are exactly
//! `Number::add(old, 1)` / `Number::subtract(old, 1)`. A non-Number (an
//! out-of-bounds `undefined`, a BigInt from a lying annotation, an object from
//! a receiver that is not a typed array) runs the generic `ToNumeric` and
//! numeric step, so a BigInt stays a BigInt and a `valueOf` runs once.
//!
//! # Rooting
//!
//! Receiver and index are rooted in one group and re-read at each use: the
//! read's exit can run a getter, `ToNumeric` a `valueOf`. The slow arm owns a
//! nested group for its own `ToNumeric` / step results, which can be heap
//! BigInts live across the `[[Set]]` (#7628's result half); the group is
//! released inside the arm, so no slot outlives the diamond. The fast arm holds
//! only Numbers.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr};

use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I32, I64};

use super::FnCtx;

/// Whether `object[index]` is an element of a receiver this lowering serves:
/// a declared/inferred numeric typed array (or `Uint8Array`) indexed by a
/// Number. Non-numeric keys (a Symbol, a property name) keep the generic
/// lowering, mirroring the typed-array `IndexSet` arm.
fn eligible(ctx: &FnCtx<'_>, object: &Expr, index: &Expr) -> bool {
    (super::index_set::is_width_tracked_typed_array_receiver(ctx, object)
        || super::index_set::is_uint8array_receiver(ctx, object))
        && crate::type_analysis::is_numeric_expr(ctx, index)
}

/// Lower a typed-array element update, or return `None` for the generic path.
#[allow(clippy::too_many_arguments)]
pub(super) fn try_lower(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    op: BinaryOp,
    prefix: bool,
    strict: bool,
    result_may_be_heap: bool,
) -> Result<Option<String>> {
    if !eligible(ctx, object, index) {
        return Ok(None);
    }
    let strict_i32 = if strict { "1" } else { "0" };
    let step_arg = match op {
        BinaryOp::Sub => "0",
        _ => "1",
    };
    rooting::with_rooted_group(ctx, 2, |ctx, group| {
        let obj = group.lower(ctx, object, true)?;
        let idx = group.lower(ctx, index, true)?;
        let old = {
            let (obj_box, idx_box) = (group.reread(ctx, obj)?, group.reread(ctx, idx)?);
            super::index_get::lower_inline_dyn_typed_array_get(ctx, &obj_box, &idx_box, false)
        };
        if ctx.block().is_terminated() {
            return Ok(old);
        }
        let fast_idx = ctx.new_block("ta_update.number");
        let slow_idx = ctx.new_block("ta_update.generic");
        let merge_idx = ctx.new_block("ta_update.merge");
        let fast_label = ctx.block_label(fast_idx);
        let slow_label = ctx.block_label(slow_idx);
        let merge_label = ctx.block_label(merge_idx);
        {
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&old);
            let is_number = blk.icmp_slt(
                I64,
                &bits,
                &crate::nanbox::i64_literal(crate::nanbox::SHORT_STRING_TAG),
            );
            blk.cond_br(&is_number, &fast_label, &slow_label);
        }

        // ---- a Number: one IEEE add, then the explicit form's store ----
        ctx.current_block = fast_idx;
        let fast_new = match op {
            BinaryOp::Sub => ctx.block().fsub(&old, "1.0"),
            _ => ctx.block().fadd(&old, "1.0"),
        };
        {
            let (obj_box, idx_box) = (group.reread(ctx, obj)?, group.reread(ctx, idx)?);
            super::index_set_typed_array::lower_inline_dyn_typed_array_set(
                ctx, &obj_box, &idx_box, &fast_new, strict, None,
            )?;
        }
        let fast_result = if prefix { fast_new } else { old.clone() };
        let fast_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        // ---- anything else: ToNumeric + numeric step, complete [[Set]] ----
        ctx.current_block = slow_idx;
        let slow_result = rooting::with_rooted_group(ctx, 2, |ctx, inner| {
            let old_num = ctx.block().call(DOUBLE, "js_to_numeric", &[(DOUBLE, &old)]);
            let old_root =
                inner.adopt_emitted(ctx, Repr::Boxed, &old_num, result_may_be_heap && !prefix);
            let new = ctx.block().call(
                DOUBLE,
                "js_numeric_step",
                &[(DOUBLE, &old_num), (I32, step_arg)],
            );
            let new_root =
                inner.adopt_emitted(ctx, Repr::Boxed, &new, result_may_be_heap && prefix);
            let (obj_box, idx_box) = (group.reread(ctx, obj)?, group.reread(ctx, idx)?);
            let new_arg = inner.reread_emitted(ctx, new_root);
            ctx.block().call(
                DOUBLE,
                "js_dyn_index_set_strict",
                &[
                    (DOUBLE, &obj_box),
                    (DOUBLE, &idx_box),
                    (DOUBLE, &new_arg),
                    (I32, strict_i32),
                ],
            );
            Ok(if prefix {
                inner.reread_emitted(ctx, new_root)
            } else {
                inner.reread_emitted(ctx, old_root)
            })
        })?;
        let slow_pred = ctx.block().label.clone();
        ctx.block().br(&merge_label);

        ctx.current_block = merge_idx;
        Ok(ctx.block().phi(
            DOUBLE,
            &[(&fast_result, &fast_pred), (&slow_result, &slow_pred)],
        ))
    })
    .map(Some)
}
