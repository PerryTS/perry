//! A bitwise operator whose operands the facts prove int32 Numbers stays in
//! i32 words end to end.
//!
//! `this.hash ^= table[k] ?? 0` on main reads the I32 field lane as a double,
//! reads the element as a double, range-tests both (`|v| < 2^63`, the
//! guarded bitwise arm of `binary::lower_guarded_numeric_arith`), truncates
//! both, and converts the result back for the lane store. Neither operand
//! needs a test:
//! - an **I32 lane read** every receiver has (`rest_fields::is_int32_read`)
//!   is the lane's word;
//! - a **present at-rest element** (`rest_elems::number_read`) is its word;
//! - an **int32 element read** (`ElisionFacts::int32_element_reads`: every
//!   Number the read yields is an int32), alone when it is never
//!   `undefined`, or under `?? <int32 literal>` (the literal replaces
//!   exactly the `undefined`);
//! - anything `i32_fast_path::is_known_i32_range` proves, and an int32
//!   literal.
//!
//! [`try_lower_bitwise`] takes `&`, `|`, `^`, `<<` and `>>` when one operand
//! is of the first three kinds and the other is proven too: each operand is
//! lowered to its i32 ([`lower`]: the lane's word, or one truncation of the
//! double), the operator is the i32 instruction, and the result is its
//! `sitofp` (which a lane store folds with its own truncation). An int32
//! Number is never a BigInt, so no helper arm exists.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): an operand taken as a double is
//! tested "an exact int32 double, or `-0`" before it is truncated (exit 86,
//! `Fact::Int32Input`). A lane's word is PR C's claim (its store witness).
//! Negative control: `PERRY_FACTS_SABOTAGE=int32read` (wpfacts claims every
//! Number element read an int32 read).
//!
//! `PERRY_INT32_OPERANDS=0` (`off`, `false`): main's lowering.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, LogicalOp};

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_INT32_OPERANDS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

fn int32_literal(e: &Expr) -> bool {
    matches!(e, Expr::Integer(n) if i32::try_from(*n).is_ok())
}

/// `e` is an element read every Number of which is an int32, and which is
/// never `undefined` unless `undef_ok`.
fn int32_element(ctx: &FnCtx<'_>, e: &Expr, undef_ok: bool) -> bool {
    let Expr::IndexGet { element_offset, .. } = e else {
        return false;
    };
    ctx.wpfacts.int32_element_reads.contains(element_offset)
        && match super::facts_canonical::element(ctx, *element_offset) {
            Some(super::facts_canonical::Num::Number) => true,
            Some(super::facts_canonical::Num::NumberOrUndefined) => undef_ok,
            None => false,
        }
}

/// `e` is one of the kinds this module adds (module docs): a value the
/// facts hold as, or prove, an int32.
fn at_rest_or_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    if super::rest_fields::is_int32_read(ctx, e) || super::rest_elems::number_read(ctx, e) {
        return true;
    }
    match e {
        Expr::IndexGet { .. } => int32_element(ctx, e, false),
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        } => int32_literal(right) && int32_element(ctx, left, true),
        _ => false,
    }
}

/// `e` is a word the facts hold (an I32 lane read, an at-rest element, an
/// int32 element read), with this module on: `int_product`'s leaf.
pub(super) fn word_read(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    enabled() && !ctx.wpfacts_off && !ctx.wpfacts.interp_guard && at_rest_or_read(ctx, e)
}

/// The i32 of a [`word_read`] operand.
pub(super) fn lower_word(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    lower(ctx, e)
}

/// `e` is an operand this module takes as an i32: an int32 Number at every
/// evaluation, a local whose ToInt32 has a shadow (`int32_shadow`), or a
/// bitwise operator this module lowers itself.
fn proven(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    int32_literal(e)
        || at_rest_or_read(ctx, e)
        || super::int32_shadow::is_shadowed(ctx, e)
        || matches!(e, Expr::Binary { op, left, right } if admitted(ctx, *op, left, right))
        || super::is_known_i32_range(ctx, e)
}

/// `e` is a word the facts hold, or a shadow: a kind this module adds.
fn word(ctx: &FnCtx<'_>, e: &Expr) -> bool {
    at_rest_or_read(ctx, e)
        || super::int32_shadow::is_shadowed(ctx, e)
        || matches!(e, Expr::Binary { op, left, right } if admitted(ctx, *op, left, right))
}

/// `left <op> right` is lowered here: a bitwise `op`, both operands
/// [`proven`], one of them a [`word`].
fn admitted(ctx: &FnCtx<'_>, op: BinaryOp, left: &Expr, right: &Expr) -> bool {
    matches!(
        op,
        BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor | BinaryOp::Shl | BinaryOp::Shr
    ) && enabled()
        && !ctx.wpfacts_off
        && !ctx.wpfacts.interp_guard
        && (word(ctx, left) || word(ctx, right))
        && proven(ctx, left)
        && proven(ctx, right)
}

/// Check mode: the double `value` of a proven operand is an exact int32
/// double or `-0`.
fn witness(ctx: &mut FnCtx<'_>, value: &str) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let bits = ctx.block().bitcast_double_to_i64(value);
    let exact = crate::codegen::emit_typed_i32_plain_guard_and_raw(ctx.block(), value, &bits).0;
    let nz = ctx.block().icmp_eq(I64, &bits, "-9223372036854775808");
    let ok = ctx.block().or(I1, &exact, &nz);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("int32.operand.verify_miss");
    let next = ctx.new_block("int32.operand.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        "int32 operand",
        crate::verify_facts::Fact::Int32Input,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}

/// The i32 of the proven operand `e`.
fn lower(ctx: &mut FnCtx<'_>, e: &Expr) -> Result<String> {
    if let Expr::Integer(n) = e {
        return Ok((*n as i32).to_string());
    }
    if super::rest_fields::is_int32_read(ctx, e) {
        if let Some(word) = super::rest_fields::try_lower_i32(ctx, e)? {
            return Ok(word);
        }
    }
    if let Some(word) = super::int32_shadow::load(ctx, e) {
        return Ok(word);
    }
    if let Expr::Binary { op, left, right } = e {
        if admitted(ctx, *op, left, right) {
            return lower_bitwise(ctx, *op, left, right);
        }
    }
    let value = super::lower_expr(ctx, e)?;
    witness(ctx, &value);
    Ok(ctx.block().toint32_fast(&value))
}

/// The i32 of `left <op> right` ([`admitted`]).
fn lower_bitwise(ctx: &mut FnCtx<'_>, op: BinaryOp, left: &Expr, right: &Expr) -> Result<String> {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::Int32Input,
        || serde_json::json!({ "operand": "bitwise" }),
    );
    crate::facts_consumed::used();
    // Left to right: each operand is an i32 once lowered, so nothing of the
    // left is live as a pointer while the right runs.
    let l = lower(ctx, left)?;
    let r = lower(ctx, right)?;
    let blk = ctx.block();
    Ok(match op {
        BinaryOp::BitAnd => blk.and(I32, &l, &r),
        BinaryOp::BitOr => blk.or(I32, &l, &r),
        BinaryOp::BitXor => blk.xor(I32, &l, &r),
        BinaryOp::Shl => {
            let shift = blk.and(I32, &r, "31");
            blk.shl(I32, &l, &shift)
        }
        _ => {
            let shift = blk.and(I32, &r, "31");
            blk.ashr(I32, &l, &shift)
        }
    })
}

/// `left <op> right` for a bitwise `op` whose operands are both taken as
/// i32s, one of them a kind this module adds: the double of the i32
/// result. `None` for every other expression.
pub(crate) fn try_lower_bitwise(
    ctx: &mut FnCtx<'_>,
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Result<Option<String>> {
    if !admitted(ctx, op, left, right) {
        return Ok(None);
    }
    let v = lower_bitwise(ctx, op, left, right)?;
    Ok(Some(ctx.block().sitofp(I32, &v, DOUBLE)))
}
