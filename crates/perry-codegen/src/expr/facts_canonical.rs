//! P3 B1 (DESIGN-p3-canonical-numbers rev 3.1 §2): consumers that drop an
//! INT32 test because the facts prove a value a plain Number.
//!
//! The fact is wpfacts' NUM declaration (REVIEW-p3 R2): an element read whose
//! value node holds only Number at every copy
//! (`ElisionFacts::number_element_reads`), possibly with `undefined` for a
//! hole or an index past the end. Every Number a node can hold is a program
//! Number (codegen lowers literals and arithmetic to plain doubles) or comes
//! from a builtin whose NUM seed is audited against its runtime function
//! (`perry-wpfacts/NUM_SEEDS.md`). So such a value is a plain IEEE double and
//! never an INT32-boxed word.
//!
//! The consumers:
//! - C1 (`expr/compare.rs`): a compare whose operands are both proven lowers
//!   to `fcmp`, with no INT32 prefix. Two Number-or-undefined operands keep
//!   an identity arm for `undefined === undefined` (REVIEW-p3 D1).
//! - C2 (`expr/binary.rs`): a guarded arithmetic leaf proven a Number needs
//!   no "is a Number" test.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): every operand a consumer trusts is
//! tested "is a plain double" (`emit_plain_number_test`: outside the tag band
//! and not HANDLE_TAG; REVIEW-p3 R7), or is exactly `undefined` when the
//! fact allows it. A miss exits 86 (`Fact::CanonicalNumber`).
//!
//! `PERRY_FACTS_CANONICAL_NUMBERS=0` turns these consumers off (wpfacts then
//! exports no element values either).

use anyhow::Result;
use perry_hir::{Expr, LogicalOp};

use super::FnCtx;
use crate::types::{DOUBLE, I1, I32, I64};

/// What the facts prove about a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Num {
    /// A plain double.
    Number,
    /// A plain double or `undefined`.
    NumberOrUndefined,
}

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_CANONICAL_NUMBERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The facts' answer for `e`, when they cover it.
pub(crate) fn of(ctx: &FnCtx<'_>, e: &Expr) -> Option<Num> {
    if !enabled() || ctx.wpfacts_off || ctx.wpfacts.interp_guard {
        return None;
    }
    match e {
        // I32 at rest B2: a present at-rest slot holds an int32 Number.
        Expr::IndexGet { .. } if crate::expr::rest_elems::number_read(ctx, e) => Some(Num::Number),
        Expr::IndexGet { element_offset, .. } => element(ctx, *element_offset),
        // `x ?? d`: `d` replaces exactly the `undefined` (and `null`, which a
        // Number-or-undefined value never is).
        Expr::Logical {
            op: LogicalOp::Coalesce,
            left,
            right,
        } => {
            of(ctx, left)?;
            let d = matches!(right.as_ref(), Expr::Integer(_) | Expr::Number(_))
                || of(ctx, right) == Some(Num::Number);
            d.then_some(Num::Number)
        }
        _ => None,
    }
}

/// The facts' answer for the element read at `element_offset`.
pub(crate) fn element(ctx: &FnCtx<'_>, element_offset: u32) -> Option<Num> {
    if !enabled() || ctx.wpfacts_off || ctx.wpfacts.interp_guard || element_offset == 0 {
        return None;
    }
    ctx.wpfacts
        .number_element_reads
        .get(&element_offset)
        .map(|&undef| {
            if undef {
                Num::NumberOrUndefined
            } else {
                Num::Number
            }
        })
}

/// Check mode: the trusted `value` (the lowering of an operand `of` proved
/// `fact`) is a plain double, or `undefined` where the fact allows it.
pub(crate) fn witness(ctx: &mut FnCtx<'_>, value: &str, fact: Num, what: &str) -> Result<()> {
    if !crate::verify_facts::enabled() {
        return Ok(());
    }
    crate::verify_facts::covered(
        crate::verify_facts::Fact::CanonicalNumber,
        || serde_json::json!({ "site": what }),
    );
    let blk = ctx.block();
    let plain = crate::codegen::emit_plain_number_test(blk, value);
    let ok = if fact == Num::NumberOrUndefined {
        let bits = blk.bitcast_double_to_i64(value);
        let undef = blk.icmp_eq(I64, &bits, crate::nanbox::TAG_UNDEFINED_I64);
        blk.or(I1, &plain, &undef)
    } else {
        plain
    };
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("canonical_number.verify_miss");
    let next = ctx.new_block("canonical_number.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&ok, &next_l, &miss_l);
    ctx.current_block = miss;
    let site_arg = crate::verify_facts::site_arg(
        ctx,
        &format!("canonical number {what}"),
        crate::verify_facts::Fact::CanonicalNumber,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, value)],
        &site_arg,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}
