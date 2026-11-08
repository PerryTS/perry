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
//! Phase P3 (§10.5): a proven Number is a canonical double. Proven code never
//! creates an INT32 box, and an INT32 box from native code reaches only a ⊤
//! node, never a proven one, so a proven Number's reader does not test the
//! INT32 tag. `PERRY_WP_REP_P3=0` restores the tag test.
//!
//! Phase P2 (§10.4), first slice: a parameter is a local like any other, so
//! one the facts give `I32` takes the canonical-i32 slot at entry
//! (`bind_params`). Its arguments are already a plain double there
//! (`facts_number_locals::emit_entry_number`), and the I32 interval is
//! exact, so the slot's value is one `fptosi`. The calling convention does
//! not change yet. `PERRY_WP_REP_P2=0` keeps parameters in their double
//! slots.

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

/// Phase P3: proven Numbers are canonical doubles (module docs).
pub(crate) fn canonical_numbers() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WP_REP_P3").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
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
