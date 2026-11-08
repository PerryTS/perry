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
