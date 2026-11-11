//! N6: a read of a module global the facts prove one Number is that Number.
//!
//! wpfacts exports the module `const`s whose every value is one integer
//! Number (`ElisionFacts::const_numbers`: a NUM-only binding with a point
//! interval, e.g. `const EP = 1 << 17`). A read of such a binding yields
//! that Number or is never reached: a read that can run before the
//! declarator is preceded by its own TDZ check (`tdz_module_check`, which
//! tests the cell itself and is not this read). So the read is the constant
//! instead of a load of `@perry_global_*`, and everything downstream (a
//! ToInt32 range test, a compare) folds on the immediate.
//!
//! Only a binding whose storage is a module global is folded: its cell holds
//! the value itself, and its dead zone is the separate check. A local's
//! boxed cell can hold a dead-zone sentinel that the read itself tests.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`): the read still loads the cell and
//! compares its bits with the constant's; a difference is a false proof
//! (exit 86, `Fact::ConstNumber`).
//!
//! `PERRY_FACTS_CONST_NUMBERS=0` withholds the facts (wpfacts exports none).

use super::FnCtx;
use crate::types::{DOUBLE, I32, I64};

/// The constant the facts give module global `id`.
fn of(ctx: &FnCtx<'_>, id: u32) -> Option<i64> {
    if ctx.wpfacts_off || ctx.wpfacts.interp_guard || !ctx.module_globals.contains_key(&id) {
        return None;
    }
    ctx.wpfacts.const_numbers.get(&id).copied()
}

/// The value of `LocalGet(id)` when the facts prove it a constant Number.
pub(crate) fn read(ctx: &mut FnCtx<'_>, id: u32) -> Option<String> {
    let value = of(ctx, id)?;
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::ConstNumber,
        || serde_json::json!({ "local": id, "value": value }),
    );
    crate::facts_consumed::used();
    let constant = value as f64;
    if crate::verify_facts::enabled() {
        witness(ctx, id, value, constant);
    }
    Some(crate::nanbox::double_literal(constant))
}

/// Check mode: the cell of module global `id` holds exactly `constant`.
fn witness(ctx: &mut FnCtx<'_>, id: u32, value: i64, constant: f64) {
    let Some(global_name) = ctx.module_globals.get(&id).cloned() else {
        return;
    };
    crate::verify_facts::covered(
        crate::verify_facts::Fact::ConstNumber,
        || serde_json::json!({ "local": id, "value": value }),
    );
    let held = crate::codegen::global_transfer::load_module_global(ctx, id, &global_name);
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(&held);
    let same = blk.icmp_eq(I64, &bits, &(constant.to_bits() as i64).to_string());
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("const_number.verify_miss");
    let next = ctx.new_block("const_number.admitted");
    let (miss_l, next_l) = (ctx.block_label(miss), ctx.block_label(next));
    ctx.block().cond_br(&same, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("constant number {value} local {id}"),
        crate::verify_facts::Fact::ConstNumber,
        &[],
    );
    let _ = crate::verify_facts::check(
        ctx,
        I32,
        "perry_wpfacts_check_miss",
        &[(DOUBLE, &held)],
        &site,
    );
    ctx.block().unreachable();
    ctx.current_block = next;
}
