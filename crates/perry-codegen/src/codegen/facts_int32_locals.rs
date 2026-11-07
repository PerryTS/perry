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
//! Check mode (`PERRY_VERIFY_FACTS=1`) witnesses every store: the stored value
//! must pass the exact-i32 test (`emit_typed_i32_guard_and_raw`). Stores the
//! ordinary lowering would compute natively in i32 take the double path
//! there, so no store escapes the witness (`checked`).
//!
//! `PERRY_FACTS_INT32_LOCALS=0` admits nothing.

use std::collections::{HashMap, HashSet};

use perry_hir::Stmt;

use crate::types::{DOUBLE, I32, I64};

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

/// A store into proven local `id` is witnessed: check mode is on and the
/// local is one. Such a store takes the double path.
pub(crate) fn checked(ctx: &crate::expr::FnCtx<'_>, id: u32) -> bool {
    crate::verify_facts::enabled() && ctx.facts_int32_locals.contains(&id)
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
        return;
    }
    note_consumed(id);
    if !crate::verify_facts::enabled() {
        return;
    }
    let (exact, _) = super::typed_abi::emit_typed_i32_guard_and_raw(ctx.block(), value);
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
/// int32, so `fptosi` (an INT32 box converts through its payload). The HIR's
/// release of a temp (`x = undefined` after its last read, which wpfacts
/// skips) keeps the ordinary NaN-safe conversion.
pub(crate) fn store_from_double(
    ctx: &mut crate::expr::FnCtx<'_>,
    value: &str,
    id: u32,
    rhs: Option<&perry_hir::Expr>,
) -> Option<String> {
    if !ctx.facts_int32_locals.contains(&id) || matches!(rhs, Some(perry_hir::Expr::Undefined)) {
        return None;
    }
    witness(ctx, value, id);
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(value);
    let is_int = super::emit_int32_box_test(blk, value);
    let low = blk.trunc(I64, &bits, I32);
    let safe = blk.select(crate::types::I1, &is_int, DOUBLE, "0.0", value);
    let plain = blk.fptosi(DOUBLE, &safe, I32);
    Some(blk.select(crate::types::I1, &is_int, I32, &low, &plain))
}
