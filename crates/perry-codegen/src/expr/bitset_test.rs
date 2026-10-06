//! Guarded native lowering for the canonical 32-bit bitset membership idiom.
//!
//! ECS implementations commonly store component masks in a `Uint32Array` and
//! test one component with
//!
//! ```text
//! mask[~~(index / 32)] & (1 << (index % 32))
//! ```
//!
//! When `index` is an erased method parameter, lowering the expression one
//! node at a time loses the relationship between its parts: division and
//! remainder round-trip through doubles, the unknown receiver emits the full
//! eight-kind typed-array dispatch, and the final `&` calls the BigInt-capable
//! dynamic helper.  A non-negative native-i32 proof makes the index arithmetic
//! exact.  This module recognizes only that exact, side-effect-free tree and
//! emits one monomorphic Uint32Array guard plus a canonical slow path.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, UnaryOp};

use crate::types::{DOUBLE, I32, I64};

use super::{lower_expr, FnCtx};

/// Return the local ids `(mask, index)` when `expr` is exactly
/// `mask[~~(index / 32)] & (1 << (index % 32))`.
fn match_u32_bitset_test(expr: &Expr) -> Option<(u32, u32)> {
    let Expr::Binary {
        op: BinaryOp::BitAnd,
        left,
        right,
    } = expr
    else {
        return None;
    };
    let Expr::IndexGet { object, index } = left.as_ref() else {
        return None;
    };
    let Expr::LocalGet(mask_id) = object.as_ref() else {
        return None;
    };
    let Expr::Unary {
        op: UnaryOp::BitNot,
        operand: outer_not,
    } = index.as_ref()
    else {
        return None;
    };
    let Expr::Unary {
        op: UnaryOp::BitNot,
        operand: inner_not,
    } = outer_not.as_ref()
    else {
        return None;
    };
    let Expr::Binary {
        op: BinaryOp::Div,
        left: div_left,
        right: div_right,
    } = inner_not.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(index_id) = div_left.as_ref() else {
        return None;
    };
    if !matches!(div_right.as_ref(), Expr::Integer(32)) {
        return None;
    }

    let Expr::Binary {
        op: BinaryOp::Shl,
        left: shift_left,
        right: shift_right,
    } = right.as_ref()
    else {
        return None;
    };
    if !matches!(shift_left.as_ref(), Expr::Integer(1)) {
        return None;
    }
    let Expr::Binary {
        op: BinaryOp::Mod,
        left: mod_left,
        right: mod_right,
    } = shift_right.as_ref()
    else {
        return None;
    };
    if !matches!(mod_left.as_ref(), Expr::LocalGet(id) if id == index_id)
        || !matches!(mod_right.as_ref(), Expr::Integer(32))
    {
        return None;
    }

    Some((*mask_id, *index_id))
}

/// Whether `expr` is the exact Number-producing bitset test recognized by
/// [`try_lower_u32_bitset_test`].
///
/// This deliberately exposes only the structural fact, not the matched local
/// ids. On every normal JavaScript exit the expression is a Number for every
/// index value: `/`, `%`, and `<<` either produce Numbers or throw, and the
/// Number shift operand means a BigInt loaded from `mask` throws at `&`
/// instead of producing a BigInt result. The non-negative proof is required
/// only by the native indexing peephole, not by this result-kind fact.
pub(crate) fn is_u32_bitset_test(expr: &Expr) -> bool {
    match_u32_bitset_test(expr).is_some()
}

/// Try the native Uint32Array bitset path.
///
/// The index proof is deliberately two-part: the local must have a native i32
/// slot *and* be known non-negative in this body.  The `$idx_u31` method clone
/// supplies both.  Without the lower-bound proof, `lshr index, 5` would differ
/// from JavaScript's truncation-toward-zero `~~(index / 32)` for negatives.
pub(super) fn try_lower_u32_bitset_test(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
) -> Result<Option<String>> {
    let Some((mask_id, index_id)) = match_u32_bitset_test(expr) else {
        return Ok(None);
    };
    let Some(index_slot) = ctx.i32_counter_slots.get(&index_id).cloned() else {
        return Ok(None);
    };
    if !ctx.nonnegative_integer_locals.contains(&index_id) {
        return Ok(None);
    }

    // Both source operands are pure local/arithmetic trees. Evaluate the
    // receiver once in source order, then keep all index work in native i32.
    let mask_box = lower_expr(ctx, &Expr::LocalGet(mask_id))?;
    let index_i32 = ctx.block().load(I32, &index_slot);
    let word_i32 = ctx.block().lshr(I32, &index_i32, "5");
    let shift_i32 = ctx.block().and(I32, &index_i32, "31");
    let bit_i32 = ctx.block().shl(I32, "1", &shift_i32);

    let header_idx = ctx.new_block("u32bitset.header");
    let fast_idx = ctx.new_block("u32bitset.fast");
    let slow_idx = ctx.new_block("u32bitset.slow");
    let merge_idx = ctx.new_block("u32bitset.merge");
    let header_label = ctx.block_label(header_idx);
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);

    // The process-global cache uses `(receiver_address << 8) | kind`. An
    // exact address hit is also what makes the later header load safe; no
    // receiver-derived address is dereferenced before this branch.
    let access = super::byte_cell::resolve(
        ctx,
        &mask_box,
        &[super::byte_cell::brand_for_kind(5)],
        &slow_label,
    );
    let raw = access.raw;
    ctx.block().br(&header_label);

    // The cache hit above proves `raw` is the live Uint32Array header. Its
    // first word is the u32 length; only an in-bounds access may bypass
    // `[[Get]]` because OOB access can observe prototype semantics.
    ctx.current_block = header_idx;
    let length = access.len.clone();
    let in_bounds = ctx.block().icmp_ult(I32, &word_i32, &length);
    ctx.block().cond_br(&in_bounds, &fast_label, &slow_label);

    // Owning Uint32Array data begins 16 bytes after its header. The bitwise
    // result is interpreted as signed i32 by JavaScript's `&` operator.
    ctx.current_block = fast_idx;
    let word_i64 = ctx.block().zext(I32, &word_i32, I64);
    let byte_offset = ctx.block().shl(I64, &word_i64, "2");
    let data_addr = ctx.block().add(I64, &raw, "16");
    let elem_addr = ctx.block().add(I64, &data_addr, &byte_offset);
    let elem_ptr = ctx.block().inttoptr(I64, &elem_addr);
    let lane = ctx.block().load(I32, &elem_ptr);
    let native_result = ctx.block().and(I32, &lane, &bit_i32);
    let fast_value = ctx.block().sitofp(I32, &native_result, DOUBLE);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    // Preserve the original expression on every miss. `js_dyn_index_get`
    // implements RequireObjectCoercible, arbitrary property keys, proxies,
    // typed-array views/kinds, OOB behavior, and accessors. The dynamic `&`
    // retains BigInt and mixed-BigInt TypeError semantics. There is no
    // allocation between the first return and the second call; the latter
    // roots both arguments before ToNumeric can allocate.
    ctx.current_block = slow_idx;
    let word_double = ctx.block().uitofp(I32, &word_i32, DOUBLE);
    let bit_double = ctx.block().sitofp(I32, &bit_i32, DOUBLE);
    let loaded = ctx.block().call(
        DOUBLE,
        "js_dyn_index_get",
        &[(DOUBLE, &mask_box), (DOUBLE, &word_double)],
    );
    let slow_value = ctx.block().call(
        DOUBLE,
        "js_dynamic_bitand",
        &[(DOUBLE, &loaded), (DOUBLE, &bit_double)],
    );
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Ok(Some(ctx.block().phi(
        DOUBLE,
        &[(&fast_value, &fast_end), (&slow_value, &slow_end)],
    )))
}
