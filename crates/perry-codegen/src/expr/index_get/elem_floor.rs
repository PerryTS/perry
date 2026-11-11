//! Check mode for wpfacts' `ElemUndef` (DESIGN-p4l1 §2.8, PR-C1).
//!
//! The solve leaves the past-the-end `undefined` out of a read of a program
//! Array when the key is an integer in `[0, lo)` of the length interval
//! `[lo, hi]`: every value downstream (a Number element read, a local's
//! representation) is proven without it. wpfacts exports such a read's
//! least `lo` (`ElisionFacts::element_floors`), and under
//! `PERRY_VERIFY_FACTS=1` the read first re-tests its key: an integer in
//! `[0, floor)`, or the facts are false (exit 86, `Fact::ElementFloor`).
//! The length half (`length >= floor`) is the length interval's own claim,
//! which `proven_array`'s witness tests where it is consumed.
//!
//! The key is evaluated again for the test, so only a key whose evaluation
//! has no effect is tested: a local.

use anyhow::Result;
use perry_hir::Expr;

use super::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// Emit the witness for `expr` (an `IndexGet`) when check mode is on and
/// the facts give the read a floor.
pub(crate) fn witness(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<()> {
    if !crate::verify_facts::enabled() || ctx.wpfacts_off {
        return Ok(());
    }
    let Expr::IndexGet {
        index,
        element_offset,
        ..
    } = expr
    else {
        return Ok(());
    };
    if *element_offset == 0 || !matches!(index.as_ref(), Expr::LocalGet(_)) {
        return Ok(());
    }
    let Some(&floor) = ctx.wpfacts.element_floors.get(element_offset) else {
        return Ok(());
    };
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ElementFloor,
        || serde_json::json!({ "floor": floor }),
    );
    let key = super::lower_expr(ctx, index)?;
    let integral = ctx.new_block("elem_floor.integral");
    let miss = ctx.new_block("elem_floor.verify_miss");
    let next = ctx.new_block("elem_floor.admitted");
    let (integral_l, miss_l, next_l) = (
        ctx.block_label(integral),
        ctx.block_label(miss),
        ctx.block_label(next),
    );
    let bound = format!("{:.1}", f64::from(floor));
    let blk = ctx.block();
    // Ordered compares: NaN and any non-Number word fail both.
    let low = blk.fcmp("oge", &key, "0.0");
    let high = blk.fcmp("olt", &key, &bound);
    let inside = blk.and(crate::types::I1, &low, &high);
    blk.cond_br(&inside, &integral_l, &miss_l);
    ctx.current_block = integral;
    let blk = ctx.block();
    let whole = blk.fptosi(DOUBLE, &key, I64);
    let back = blk.sitofp(I64, &whole, DOUBLE);
    let exact = blk.fcmp("oeq", &back, &key);
    crate::verify_facts::tick(ctx);
    ctx.block().cond_br(&exact, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("element read below floor {floor}"),
        crate::verify_facts::Fact::ElementFloor,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &key)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
    Ok(())
}
