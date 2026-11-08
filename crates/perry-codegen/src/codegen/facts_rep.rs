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

use super::FactsRep;

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
