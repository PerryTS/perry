//! The whole-program representation of locals (representation RFC §10,
//! phase P1): a `let` the facts cover takes the facts' representation
//! (`ElisionFacts::local_reps`, wpfacts `BindingFacts::rep`), and codegen
//! does not choose one for it.
//!
//! For the canonical-i32 slot that means: a local the facts give `I32` takes
//! the slot whenever the storage allows it (no profitability refusal: the
//! facts' int demand already answered "should we", whole-program); a local
//! the facts give `F64` or `Boxed` never takes it, whatever the per-function
//! collectors would have proved. A local the facts do not cover keeps the
//! collectors' decision.
//!
//! `PERRY_WP_REP_P1=0` restores the collectors' decision everywhere.
//!
//! Phase P3 (§10.5): a Number is a canonical double, its one encoding. No
//! reader tests for another.
//!
//! Phase P2 (§10.4), first slice: a parameter is a local like any other, so
//! one the facts give `I32` takes the canonical-i32 slot at entry
//! (`bind_params`). Its arguments are already a plain double there, and the I32 interval is
//! exact, so the slot's value is one `fptosi`. The calling convention does
//! not change yet. A method call whose every callee returns only Numbers
//! (wpfacts `ModuleBindings::number_calls`, the call's return
//! representation) is a Number (`number_call`). `PERRY_WP_REP_P2=0` keeps
//! parameters in their double slots and call results unproven.

use std::collections::HashSet;

use super::FactsRep;
use crate::types::{DOUBLE, I32};

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WP_REP_P1").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The facts' representation of local `id`, when they cover it.
pub(crate) fn local(ctx: &crate::expr::FnCtx<'_>, id: u32) -> Option<FactsRep> {
    if !enabled() || ctx.wpfacts_off {
        return None;
    }
    ctx.wpfacts.local_reps.get(&id).copied()
}

/// Phase P2 (module docs).
pub(crate) fn typed_params() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WP_REP_P2").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The method call whose callee is `callee` is a Number by the facts: a
/// canonical double (module docs).
pub(crate) fn number_call(ctx: &crate::expr::FnCtx<'_>, callee: &perry_hir::Expr) -> bool {
    let Some(byte_offset) = claimed_call(ctx, callee) else {
        return false;
    };
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::NumericResult,
        || serde_json::json!({ "call": byte_offset }),
    );
    crate::facts_consumed::used();
    true
}

/// The site offset of the method call through `callee` when the facts
/// prove it a Number call and this body may use that proof.
fn claimed_call(ctx: &crate::expr::FnCtx<'_>, callee: &perry_hir::Expr) -> Option<u32> {
    let perry_hir::Expr::PropertyGet { byte_offset, .. } = callee else {
        return None;
    };
    let usable = typed_params()
        && !ctx.wpfacts_off
        && !ctx.wpfacts.interp_guard
        && *byte_offset != 0
        && ctx.wpfacts.number_calls.contains(byte_offset);
    usable.then_some(*byte_offset)
}

/// Check mode: `value`, the result of the call through `callee` that
/// `number_call` proves a Number, is a plain double, not a tagged value. A
/// miss reports the false proof and exits 86.
pub(crate) fn witness_number_call(
    ctx: &mut crate::expr::FnCtx<'_>,
    callee: &perry_hir::Expr,
    value: &str,
) {
    if !crate::verify_facts::enabled() {
        return;
    }
    let Some(byte_offset) = claimed_call(ctx, callee) else {
        return;
    };
    crate::verify_facts::covered(
        crate::verify_facts::Fact::NumericResult,
        || serde_json::json!({ "call": byte_offset }),
    );
    let plain = super::typed_entry::emit_plain_number_test(ctx.block(), value);
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("number.call.verify_miss");
    let next = ctx.new_block("number.call.admitted");
    let miss_l = ctx.block_label(miss);
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(&plain, &next_l, &miss_l);
    ctx.current_block = miss;
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("number call @{byte_offset}"),
        crate::verify_facts::Fact::NumericResult,
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

/// Give each parameter in `number_params` (proven Number on every entry,
/// never rebound, not boxed: `facts_number_locals::facts_number_params`)
/// that the facts give `I32` its canonical-i32 slot, seeded from the entry
/// value. Reads and writes then take that slot
/// (`expr::canonical_local_i32_slot`). A parameter another rule already
/// holds in an i32 slot, or a closure reads, keeps its storage, and so does
/// every parameter of a body whose context refuses canonical-i32 locals.
pub(crate) fn bind_params(ctx: &mut crate::expr::FnCtx<'_>, number_params: &HashSet<u32>) {
    if !typed_params() || !ctx.repsel_context_allows_canonical_i32 {
        return;
    }
    let mut ids: Vec<u32> = number_params.iter().copied().collect();
    ids.sort_unstable();
    for id in ids {
        if local(ctx, id) != Some(FactsRep::I32)
            || ctx.local_slot_reps.contains_key(&id)
            || ctx.i32_counter_slots.contains_key(&id)
            || ctx.repsel_closure_ref_locals.contains(&id)
            || ctx.boxed_vars.contains(&id)
            || ctx.module_globals.contains_key(&id)
        {
            continue;
        }
        let Some(slot) = ctx.locals.get(&id).cloned() else {
            continue;
        };
        let value = ctx.block().load(DOUBLE, &slot);
        super::facts_int32_locals::witness_exact(ctx, &value, id);
        let raw = ctx.block().fptosi(DOUBLE, &value, I32);
        let i32_slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, &raw, &i32_slot);
        ctx.i32_counter_slots.insert(id, i32_slot);
        ctx.local_slot_reps.insert(id, crate::expr::SlotRep::I32);
    }
}

#[cfg(test)]
#[path = "facts_rep_tests.rs"]
mod tests;
