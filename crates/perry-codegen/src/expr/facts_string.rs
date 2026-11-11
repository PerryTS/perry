//! Strings proven by the whole-program facts: the kinds of a call's result
//! (wpfacts `ModuleBindings::call_kinds`).
//!
//! Codegen types a call of a program function as "any value", so
//! `squareName(a) + squareName(b)` took `js_dynamic_string_or_number_add`
//! (about 65 instructions) although every body the call reaches returns a
//! string. The facts say what a call returns: a call whose result holds only
//! strings at every copy is a string operand, and `f(x) ?? <string>` is one
//! when `f` returns only strings, `null` and `undefined`. A `+` of two such
//! operands is the pairwise concatenation (`lower_string_concat`): inline for
//! two short inline strings (`expr/short_string.rs`), `js_string_concat_box`
//! otherwise.
//!
//! The fact selects a lowering, never an answer: every arm of the pairwise
//! concatenation is chosen by the operands' tags at run time, and a pair that
//! is no pair of strings reaches `js_dynamic_string_or_number_add` inside
//! `js_string_concat_box`. Check mode (`PERRY_VERIFY_FACTS=1`) still tests
//! each operand the facts alone call a string (`Fact::CallKinds`, exit 86).
//! Negative control: `PERRY_FACTS_SABOTAGE=stringcall`.
//!
//! The empty literal of `f(x) ?? ""` under such a `+` is lowered as the
//! empty inline string, so the pair `"e2e4" + ""` is joined at the site.
//!
//! Probe (`PERRY_SITE_STATS=1`): `PERRY_SITE facts-string concat`.
//!
//! `PERRY_FACTS_CALL_KINDS=0` (`off`, `false`): the facts export no call
//! kinds, and no `+` is lowered here. A compile-cache input.

use anyhow::Result;
use perry_hir::{BinaryOp, Expr, LogicalOp};

use super::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// wpfacts `model::prim` bits.
const K_STR: u16 = 1 << 1;
const K_NULL: u16 = 1 << 3;
const K_UNDEF: u16 = 1 << 4;

/// The kinds the facts give the result of call `e`, when this body may use
/// them.
fn call_kinds(ctx: &FnCtx<'_>, e: &Expr) -> Option<u16> {
    let Expr::Call { byte_offset, .. } = e else {
        return None;
    };
    if ctx.wpfacts_off || ctx.wpfacts.interp_guard || *byte_offset == 0 {
        return None;
    }
    ctx.wpfacts.call_kinds.get(byte_offset).copied()
}

/// `e` is a string by the facts: a call that returns only strings, such a
/// call (or one that may also return `null` / `undefined`) defaulted with
/// `??` to a string, a `+` with a string operand, a conditional of two
/// strings. Operands the declarations already call a string count too.
fn is_string(ctx: &FnCtx<'_>, e: &Expr, depth: u8) -> bool {
    if crate::type_analysis::is_declared_string_expr(ctx, e) {
        return true;
    }
    if depth == 0 {
        return false;
    }
    match e {
        Expr::Call { .. } => call_kinds(ctx, e) == Some(K_STR),
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        } => {
            is_string(ctx, right, depth - 1)
                && (is_string(ctx, left, depth - 1)
                    || call_kinds(ctx, left)
                        .is_some_and(|k| k & K_STR != 0 && k & !(K_STR | K_NULL | K_UNDEF) == 0))
        }
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => is_string(ctx, left, depth - 1) || is_string(ctx, right, depth - 1),
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => is_string(ctx, then_expr, depth - 1) && is_string(ctx, else_expr, depth - 1),
        _ => false,
    }
}

/// How deep [`is_string`] follows `??`, `+` and `?:`.
const DEPTH: u8 = 6;

/// `left + right` is a concatenation of two strings, and the facts are what
/// says so for at least one of them.
pub(crate) fn both_strings(ctx: &FnCtx<'_>, left: &Expr, right: &Expr) -> bool {
    !ctx.wpfacts.call_kinds.is_empty()
        && is_string(ctx, left, DEPTH)
        && is_string(ctx, right, DEPTH)
}

/// The empty literal that defaults a `??` operand of the `+`.
fn defaulted_empty(e: &Expr) -> Option<&Expr> {
    match e {
        Expr::Logical {
            op: LogicalOp::Coalesce,
            right,
            ..
        } => matches!(right.as_ref(), Expr::String(s) if s.is_empty()).then_some(right.as_ref()),
        _ => None,
    }
}

/// `left + right` for [`both_strings`]: the pairwise concatenation, with
/// check mode's test on each operand only the facts call a string.
pub(crate) fn lower_concat(ctx: &mut FnCtx<'_>, left: &Expr, right: &Expr) -> Result<String> {
    super::note_site("facts-string concat");
    crate::facts_consumed::used();
    let by_facts = [left, right].map(|e| !crate::type_analysis::is_declared_string_expr(ctx, e));
    for (e, facts) in [left, right].into_iter().zip(by_facts) {
        if facts {
            crate::verify_facts::consumed(
                crate::verify_facts::Fact::CallKinds,
                || serde_json::json!({ "operand": operand_offset(e) }),
            );
        }
    }
    let empties: Vec<&Expr> = [left, right]
        .into_iter()
        .filter_map(defaulted_empty)
        .collect();
    super::short_string::with_inline_literals(&empties, || {
        super::short_string::with_char_operands(&[left, right], || {
            crate::rooting::with_operands_rooted(ctx, &[left, right], |ctx, values| {
                for ((e, facts), value) in [left, right].into_iter().zip(by_facts).zip(values) {
                    if facts {
                        witness_string(ctx, e, value);
                    }
                }
                Ok(super::short_string::emit_concat_pair(
                    ctx, &values[0], &values[1],
                ))
            })
        })
    })
}

/// The source offset that names operand `e` in a report: its call's, or the
/// call's under a `??` / the first one under a `+`.
fn operand_offset(e: &Expr) -> u32 {
    match e {
        Expr::Call { byte_offset, .. } => *byte_offset,
        Expr::Logical { left, .. } | Expr::Binary { left, .. } => operand_offset(left),
        Expr::Conditional { then_expr, .. } => operand_offset(then_expr),
        _ => 0,
    }
}

/// Check mode: `value`, operand `e` the facts call a string, is one (a heap
/// or an inline string). A miss reports the false proof and exits 86.
fn witness_string(ctx: &mut FnCtx<'_>, e: &Expr, value: &str) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let offset = operand_offset(e);
    crate::verify_facts::covered(
        crate::verify_facts::Fact::CallKinds,
        || serde_json::json!({ "operand": offset }),
    );
    let is_string = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        let tag = blk.lshr(I64, &bits, "48");
        let heap = blk.icmp_eq(I64, &tag, crate::nanbox::STRING_TAG_TOP16_I64);
        let inline = blk.icmp_eq(I64, &tag, crate::nanbox::SHORT_STRING_TAG_TOP16_I64);
        blk.or(crate::types::I1, &heap, &inline)
    };
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("string.call.verify_miss");
    let next = ctx.new_block("string.call.admitted");
    let miss_l = ctx.block_label(miss);
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(&is_string, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("string operand @{offset}"),
        crate::verify_facts::Fact::CallKinds,
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
