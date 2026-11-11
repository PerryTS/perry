//! The indexed walk of an immutable Number array (CENSUS-residual-2
//! lever 6): `for (const x of a)` where `a` is a rooted `const` that selects
//! among module-const arrays (`const a = c ? KNIGHT : ROOK`), and
//! `for (let k = 0; k < a.length; k++) … a[k] …` over the same kind of
//! local.
//!
//! **The proof** (`indexed`, all from wpfacts). The local holds only
//! immutable Arrays (`ElisionFacts::immutable_arrays`: a rooted `const`, so
//! a read that yields a value yields one of them) and its representation is
//! `arr<elem,floor,dense,fixed,im>` with a Number element and no at-rest
//! words. The rep is the join over the Arrays the local may hold
//! (`bindings::tok_arr_rep`), and each of them has `fixed` (its length is
//! one value for its whole life, so its storage is never reallocated or
//! forwarded) and `dense` (born hole-free up to its floor, which for a
//! fixed length is that length; `im`: nothing writes it afterwards). So,
//! whichever Array the local holds:
//! - its length is a loop invariant: the loop bound is one `i32` load in
//!   the preheader and the test `k <u length` (`stmt::loops::walk_bound`);
//! - every index below its length is a present own element, a plain double
//!   that is an int32 when `elem` is I32: the read is the slot load at
//!   `h + 24 + 8 * k`, with no bound, no hole test and no cold arm
//!   (`proven_array::lower_below_length`).
//!
//! **The key.** `k` is below the length when the loop's descriptor says so
//! (`receiver_descriptors[bounded_index]`: the guard is `k < a.length`, the
//! update is `k++`, the body does not write `k`, `k` is not negative at
//! entry) and no closure can write `k` (a callee of the body is then no
//! writer either).
//!
//! `PERRY_FACTS_WALK_INDEXED=0` turns it off: the loop keeps the guard and
//! the read it had. Negative control `PERRY_FACTS_SABOTAGE=walkdense`: an
//! immutable Array of fixed length counts as dense. Check mode
//! (`PERRY_VERIFY_FACTS=1`) tests each read (`array_rep::witness_read`):
//! the key is below the length the Array has now, the slot is no hole and
//! holds a Number of `elem`. `PERRY_ARRAY_BASE_DIAG=1` prints the verdict
//! per loop (`[perry walk]`).

use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, typed_feedback_emission_enabled, FnCtx};
use crate::codegen::{FactsArrRep, FactsElemRep, FactsRep};
use crate::types::I32;

/// `PERRY_FACTS_WALK_INDEXED=0` turns the indexed walk off.
pub(crate) fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_FACTS_WALK_INDEXED").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

/// Why local `id` is no indexed receiver, or its representation
/// (module docs, the proof).
pub(crate) fn verdict(ctx: &FnCtx<'_>, id: u32) -> std::result::Result<FactsArrRep, &'static str> {
    if !enabled()
        || !super::proven_array::tier_enabled()
        || ctx.wpfacts_off
        || typed_feedback_emission_enabled()
    {
        return Err("off");
    }
    // A local the codegen keeps in scalar slots holds no array word.
    if ctx.scalar_replaced_arrays.contains_key(&id)
        || ctx.array_row_aliases.contains_key(&id)
        || ctx.scalar_replaced.contains_key(&id)
    {
        return Err("scalar-replaced");
    }
    let Some(FactsRep::Arr(rep)) = crate::codegen::facts_rep::local(ctx, id) else {
        return Err("no-array-rep");
    };
    if !rep.im || !rep.fixed || !ctx.wpfacts.immutable_arrays.contains(&id) {
        return Err("not-immutable");
    }
    if rep.at_rest {
        return Err("at-rest");
    }
    if rep.elem == FactsElemRep::Boxed {
        return Err("boxed-element");
    }
    let dense = rep.dense || super::field_base::sabotaged("walkdense");
    if !dense || rep.floor == 0 {
        return Err("not-dense");
    }
    Ok(FactsArrRep { dense, ..rep })
}

/// Local `id` is an indexed receiver (module docs): its representation.
pub(crate) fn indexed(ctx: &FnCtx<'_>, id: u32) -> Option<FactsArrRep> {
    verdict(ctx, id).ok()
}

/// No closure can write local `id`: the statements that name it are its
/// only writers (`stmt::counter_range`, `is_closure_writable`).
pub(crate) fn closure_free(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.boxed_vars.contains(&id)
        && !ctx.prealloc_boxes.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
}

/// The read `expr` when it is `a[k]` of an indexed receiver `a` with `k`
/// proven below its length (module docs): the slot's double.
pub(super) fn try_lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Expr::IndexGet { object, index, .. } = expr else {
        return Ok(None);
    };
    let (Expr::LocalGet(a), Expr::LocalGet(k)) = (object.as_ref(), index.as_ref()) else {
        return Ok(None);
    };
    if !ctx.receiver_descriptors.has_bounded_index(*a, *k) || !closure_free(ctx, *k) {
        return Ok(None);
    }
    let Some(rep) = indexed(ctx, *a) else {
        return Ok(None);
    };
    let Some(slot) = ctx.i32_counter_slots.get(k).cloned() else {
        return Ok(None);
    };
    let arr_box = lower_expr(ctx, object)?;
    let idx = ctx.block().load(I32, &slot);
    Ok(Some(super::proven_array::lower_below_length(
        ctx, &arr_box, &idx, rep, *a,
    )))
}
