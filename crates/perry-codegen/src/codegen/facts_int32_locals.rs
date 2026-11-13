//! Locals the whole-program interval proof (`perry_wpfacts::model::range`)
//! shows hold only int32 values, never `-0` (`ElisionFacts::int32_inputs`).
//!
//! The proof is the whole admission: such a `let` is integer-valued and
//! i32-bounded (`collectors::hir_facts`, beside the loop-bounded range proof),
//! so the representation-selection Phase 1 lowering may give it its i32 slot;
//! whether it does stays the profitability rule's decision
//! (`repsel_benefit`). A double entering that slot is an exact int32, so it
//! converts with `fptosi` instead of ToInt32 (`store_from_double`).
//!
//! The proof's interval is a range proof of the local at every read
//! (`expr::range_facts::facts_local_range`, `PERRY_FACTS_LOCAL_RANGES=0`
//! turns that off): an index or an integer chain built from it needs no
//! dominating guard.
//!
//! Check mode (`PERRY_VERIFY_FACTS=1`) witnesses every store: the stored value
//! must pass the exact-i32 test (`emit_typed_i32_plain_guard_and_raw`) and lie
//! in the interval the range proofs read. Stores the
//! ordinary lowering would compute natively in i32 take the double path
//! there, so no store escapes the witness (`checked`).
//!
//! The claim is witnessed wherever codegen leans on it (`claimed`), not
//! only for this body's proven `let`s: a local the facts give `I32`
//! (`facts_rep::local`) takes the canonical i32 slot in every body, also
//! where no proven-`let` set exists (module init, a closure, a static
//! method), and a proven `let` the facts leave `F64` (no int demand: a
//! `const` with one use) still gets the collectors' i32 shadow and is read
//! from it. Check mode tests each double that enters either
//! (`store_from_double`, `witness`), each `++`/`--` (`witness_update`), and
//! lowers such a local's initializer as a value, so the native initializer
//! path cannot seed the slot untested (`stmt::let_stmt`).
//!
//! `PERRY_FACTS_INT32_LOCALS=0` admits nothing.

use std::collections::{HashMap, HashSet};

use perry_hir::Stmt;

use crate::types::{DOUBLE, I1, I32, I64};

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_INT32_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The body's `let`s the facts prove int32.
pub(crate) fn lets(
    facts: &super::ElisionFacts,
    body: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashSet<u32> {
    if !enabled() || facts.int32_inputs.is_empty() {
        return HashSet::new();
    }
    let mut lets = HashSet::new();
    crate::collectors::collect_let_ids(body, &mut lets);
    lets.retain(|id| {
        facts.int32_inputs.contains(id)
            && !boxed_vars.contains(id)
            && !module_globals.contains_key(id)
    });
    lets
}

/// The empty set, for bodies that take no proven int32 `let`s.
pub(crate) fn none() -> &'static HashSet<u32> {
    static NONE: std::sync::OnceLock<HashSet<u32>> = std::sync::OnceLock::new();
    NONE.get_or_init(HashSet::new)
}

/// Codegen leans on the facts' int32 claim of local `id` in this body: it
/// is one of the body's proven `let`s, or the facts give it `I32` and the
/// canonical slot follows (`facts_rep::local`, any body).
pub(crate) fn claimed(ctx: &crate::expr::FnCtx<'_>, id: u32) -> bool {
    ctx.facts_int32_locals.contains(&id)
        || matches!(super::facts_rep::local(ctx, id), Some(super::FactsRep::I32))
}

/// A store into proven local `id` is witnessed: check mode is on and the
/// local is one. Such a store takes the double path.
pub(crate) fn checked(ctx: &crate::expr::FnCtx<'_>, id: u32) -> bool {
    crate::verify_facts::enabled() && claimed(ctx, id)
}

/// Check mode lowers the initializer of local `id` as a value (the generic
/// path, which witnesses it): the facts prove it an int32 (`checked`) or a
/// Number (`FnCtx::facts_number_locals`). The native initializer path would
/// store its own number-context lowering untested: a wrapped product, or
/// the `NaN` a hole read coerces to.
pub(crate) fn checked_init(ctx: &crate::expr::FnCtx<'_>, id: u32) -> bool {
    crate::verify_facts::enabled() && (claimed(ctx, id) || ctx.facts_number_locals.contains(&id))
}

fn note_consumed(id: u32) {
    crate::verify_facts::consumed(
        crate::verify_facts::Fact::Int32Input,
        || serde_json::json!({ "local": id }),
    );
    crate::facts_consumed::used();
}

/// Witness the double `value` stored into proven local `id`.
pub(crate) fn witness(ctx: &mut crate::expr::FnCtx<'_>, value: &str, id: u32) {
    if !ctx.facts_int32_locals.contains(&id) {
        // The facts' `I32` local of a body without a proven-`let` set
        // (`claimed`): check mode only, the lowering is unchanged.
        if checked(ctx, id) {
            witness_exact(ctx, value, id);
        }
        return;
    }
    note_consumed(id);
    witness_exact(ctx, value, id);
}

/// Check mode: `++` / `--` wrote `old ± 1` into local `id`, a claimed
/// int32 (`claimed`). The update is computed in the slot's own type (an
/// `i32` add wraps), so the witness takes the double the step makes.
pub(crate) fn witness_update(
    ctx: &mut crate::expr::FnCtx<'_>,
    id: u32,
    old: &str,
    increment: bool,
) {
    if !checked(ctx, id) {
        return;
    }
    let new = if increment {
        ctx.block().fadd(old, "1.0")
    } else {
        ctx.block().fsub(old, "1.0")
    };
    witness_exact(ctx, &new, id);
}

/// Check mode: `value`, a proven Number held by local `id` as an int32,
/// is an exact int32 double. A miss reports the false proof and exits 86.
pub(crate) fn witness_exact(ctx: &mut crate::expr::FnCtx<'_>, value: &str, id: u32) {
    if !crate::verify_facts::enabled() {
        return;
    }
    // A Number is a plain double: the witness admits an exact int32 double.
    let bits = ctx.block().bitcast_double_to_i64(value);
    let mut exact =
        super::typed_abi::emit_typed_i32_plain_guard_and_raw(ctx.block(), value, &bits).0;
    // −0 tolerance (wpfacts PR D): the local may receive a `-0` no use
    // observes; it enters the slot as `0`.
    if ctx.wpfacts.zero_tolerant_locals.contains(&id) {
        let nz = ctx.block().icmp_eq(I64, &bits, "-9223372036854775808");
        exact = ctx.block().or(I1, &exact, &nz);
    }
    // The interval the body's range proofs read for this local
    // (`range_facts::facts_local_range`): the stored value lies in it. A
    // tolerated `-0` compares as 0.
    if let Some(range) = crate::expr::facts_local_range(ctx, id) {
        let blk = ctx.block();
        let ge = blk.fcmp(
            "oge",
            value,
            &crate::nanbox::double_literal(range.min as f64),
        );
        let le = blk.fcmp(
            "ole",
            value,
            &crate::nanbox::double_literal(range.max as f64),
        );
        let inside = blk.and(I1, &ge, &le);
        exact = blk.and(I1, &exact, &inside);
    }
    crate::verify_facts::tick(ctx);
    let miss = ctx.new_block("int32.input.verify_miss");
    let next = ctx.new_block("int32.input.admitted");
    let miss_l = ctx.block_label(miss);
    let next_l = ctx.block_label(next);
    ctx.block().cond_br(&exact, &next_l, &miss_l);
    ctx.current_block = miss;
    crate::verify_facts::int32_checked(id);
    let site = crate::verify_facts::site_arg(
        ctx,
        &format!("int32 input local {id}"),
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

/// The i32 of a double stored into proven local `id`, if it is one: an exact
/// int32, so `fptosi`. The value is a proven Number, a plain double. The
/// HIR's release of a temp (`x = undefined` after its last read, which
/// wpfacts skips) keeps the ordinary NaN-safe conversion.
pub(crate) fn store_from_double(
    ctx: &mut crate::expr::FnCtx<'_>,
    value: &str,
    id: u32,
    rhs: Option<&perry_hir::Expr>,
) -> Option<String> {
    if matches!(rhs, Some(perry_hir::Expr::Undefined)) {
        return None;
    }
    if !ctx.facts_int32_locals.contains(&id) {
        // The facts' `I32` local of a body without a proven-`let` set
        // (`claimed`): the slot keeps its conversion, check mode tests the
        // value that enters it.
        if checked(ctx, id) {
            witness_exact(ctx, value, id);
        }
        return None;
    }
    witness(ctx, value, id);
    Some(ctx.block().fptosi(DOUBLE, value, I32))
}

#[cfg(test)]
#[path = "facts_int32_locals_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "facts_local_ranges_tests.rs"]
mod range_tests;
