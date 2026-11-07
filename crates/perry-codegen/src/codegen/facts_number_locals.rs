//! Bindings the whole-program facts prove hold only Numbers.
//!
//! wpfacts' value flow is inclusion-based over every write the program can
//! make: each initializer and assignment, the `undefined` of a declaration
//! without one, the `undefined` a short call passes for a missing argument
//! (`perry-wpfacts` `solve/call_and_construct.rs`), and the ⊤ an escaping
//! function receives from unknown callers. A binding whose node holds the
//! Number primitive and nothing else (`ElisionFacts::number_inputs`) holds a
//! Number at every read, through every body variant.
//!
//! Such a binding owns no root slot, and each read is the slot's raw double,
//! on the footing of the field proof (`expr/wpfacts_access.rs`,
//! `FieldProof::number`): a Number has one encoding.
//!
//! A parameter the body never rebinds or mutates converts an INT32 box to its
//! plain double at entry, as a split region's Number parameter does, and is
//! then a Number by construction: the entry conversion is its only write. A
//! `let` only answers `local_is_number` (`FnCtx::facts_number_locals`). It
//! does not join `number_by_construction_locals`, whose members also promise
//! that every write's operands are Numbers: `total += x.length` keeps
//! Number `total` while `x.length` may be `undefined`, and that write must
//! keep its guarded `+`.
//!
//! `PERRY_FACTS_NUMBER_LOCALS=0` turns the proof off for A/B comparison.

use std::collections::{HashMap, HashSet};

use perry_hir::{Param, Stmt};

use crate::types::{DOUBLE, I1, I32, I64};

pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_NUMBER_LOCALS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// The parameters of `params` that enter as Numbers and stay one for the
/// whole `body`: never rebound, never mutated, so the entry conversion is the
/// only write the body sees.
pub(crate) fn facts_number_params(
    facts: &super::ElisionFacts,
    params: &[Param],
    body: &[Stmt],
    boxed_vars: &HashSet<u32>,
) -> HashSet<u32> {
    if !enabled() || facts.number_inputs.is_empty() {
        return HashSet::new();
    }
    let candidates: Vec<u32> = params
        .iter()
        .filter(|p| {
            !p.is_rest
                && p.default.is_none()
                && p.arguments_object.is_none()
                && facts.number_inputs.contains(&p.id)
                && !boxed_vars.contains(&p.id)
        })
        .map(|p| p.id)
        .collect();
    if candidates.is_empty() {
        return HashSet::new();
    }
    let rebound = crate::collectors::reassigned_locals(body);
    candidates
        .into_iter()
        .filter(|id| !rebound.contains(id) && !crate::collectors::has_any_mutation(body, *id))
        .collect()
}

/// The `let` bindings declared in `body` that hold only Numbers. A boxed
/// binding lives in a heap cell and a module global in its global slot; both
/// keep their own read path.
pub(crate) fn facts_number_lets(
    facts: &super::ElisionFacts,
    body: &[Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
) -> HashSet<u32> {
    if !enabled() || facts.number_inputs.is_empty() {
        return HashSet::new();
    }
    let mut lets = HashSet::new();
    crate::collectors::collect_let_ids(body, &mut lets);
    lets.retain(|id| {
        facts.number_inputs.contains(id)
            && !boxed_vars.contains(id)
            && !module_globals.contains_key(id)
    });
    lets
}

/// The empty set, for bodies that take no facts-proven `let`s.
pub(crate) fn none() -> &'static HashSet<u32> {
    static NONE: std::sync::OnceLock<HashSet<u32>> = std::sync::OnceLock::new();
    NONE.get_or_init(HashSet::new)
}

/// The plain double of a Number argument: an INT32 box converts, every other
/// Number is its own bits.
pub(crate) fn emit_entry_number(blk: &mut crate::block::LlBlock, arg: &str) -> String {
    let is_int = super::typed_entry::emit_int32_box_test(blk, arg);
    let bits = blk.bitcast_double_to_i64(arg);
    let low = blk.trunc(I64, &bits, I32);
    let as_f64 = blk.sitofp(I32, &low, DOUBLE);
    blk.select(I1, &is_int, DOUBLE, &as_f64, arg)
}
