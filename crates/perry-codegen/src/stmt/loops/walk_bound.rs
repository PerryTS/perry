//! The loop over an immutable Number array (`expr::index_get::walk_read`,
//! CENSUS-residual-2 lever 6):
//!
//! ```text
//! for (let k = n; k < a.length; k++) body        // n an int32 literal >= 0
//! ```
//!
//! where the facts make `a` an indexed receiver (`walk_read::indexed`: a
//! rooted `const` of immutable, dense Arrays of fixed length with Number
//! elements) and `k` is the loop's own `let`, written by nothing but the
//! update: the body does not write it and no closure can. The for-of walk
//! of such a local has this shape (`stmt::const_array_walk`), whose counter
//! the body cannot name.
//!
//! The Array `a` holds never changes its length, so the bound is read once
//! in the preheader and the guard is `k <u length` on the counter's i32
//! slot: no receiver test, no length load, no Number compare and no poll in
//! the header. In the body `0 <= k < length`
//! (`receiver_descriptors[bounded_index]`), which makes `a[k]` the slot
//! load (`walk_read::try_lower`).

use super::*;
use crate::expr::index_get::walk_read;

/// The bound of a walk whose guard is lowered here.
pub(super) struct WalkBound {
    counter: u32,
    /// The i32 slot of the length, stored in the preheader.
    len_slot: String,
}

/// `(a, k)` of `for (let k = n; k < a.length; k++)`.
fn shape(
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
) -> Option<(u32, u32)> {
    use perry_hir::{CompareOp, Expr, UpdateOp};
    let Some(Stmt::Let { id: counter, .. }) = init else {
        return None;
    };
    let Some(Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    }) = condition
    else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = right.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(array) = object.as_ref() else {
        return None;
    };
    let stepped = matches!(
        update,
        Some(Expr::Update { id, op: UpdateOp::Increment, .. }) if id == counter
    );
    if property != "length"
        || !stepped
        || array == counter
        || !matches!(left.as_ref(), Expr::LocalGet(k) if k == counter)
    {
        return None;
    }
    Some((*array, *counter))
}

fn diag(ctx: &FnCtx<'_>, array: u32, counter: u32, verdict: &str) {
    if std::env::var("PERRY_ARRAY_BASE_DIAG").as_deref() == Ok("1") {
        eprintln!(
            "[perry walk] fn={} array={array} counter={counter} verdict={verdict}",
            ctx.func.name
        );
    }
}

/// When the loop is a walk (module docs): record that its counter is below
/// the Array's length for `scope_id`, and with `bound` read the length into
/// the preheader (the current block) for [`emit_cond`].
pub(super) fn install(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    scope_id: u32,
    bound: bool,
) -> Result<Option<WalkBound>> {
    let Some((array, counter)) = shape(init, condition, update) else {
        return Ok(None);
    };
    let rep = match walk_read::verdict(ctx, array) {
        Ok(rep) => rep,
        Err(why) => {
            // Only a local the facts give an immutable Array is reported.
            if !matches!(why, "off" | "no-array-rep" | "not-immutable") {
                diag(ctx, array, counter, &format!("refused:{why}"));
            }
            return Ok(None);
        }
    };
    if !loop_counter_entry_i32_range_is_safe(init, counter)
        || stmts_mutate_local(body, counter)
        || !walk_read::closure_free(ctx, counter)
        || !ctx.i32_counter_slots.contains_key(&counter)
    {
        diag(ctx, array, counter, "refused:counter");
        return Ok(None);
    }
    ctx.receiver_descriptors
        .materialize_bounded_index(array, counter, scope_id);
    diag(
        ctx,
        array,
        counter,
        &format!(
            "indexed elem={:?} floor={} bound={}",
            rep.elem,
            rep.floor,
            if bound { "hoisted" } else { "kept" }
        ),
    );
    if !bound {
        return Ok(None);
    }
    // The Array's length never moves: one load serves every iteration.
    let recv = lower_expr(ctx, &perry_hir::Expr::LocalGet(array))?;
    let blk = ctx.block();
    let bits = blk.bitcast_double_to_i64(&recv);
    let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let ptr = blk.inttoptr(I64, &handle);
    let length = blk.load(I32, &ptr);
    let len_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &length, &len_slot);
    Ok(Some(WalkBound { counter, len_slot }))
}

/// The guard `k <u length` of a walk, in the loop's condition block.
pub(super) fn emit_cond(ctx: &mut FnCtx<'_>, walk: &WalkBound, body_label: &str, exit_label: &str) {
    let slot = ctx.i32_counter_slots[&walk.counter].clone();
    let blk = ctx.block();
    let counter = blk.load(I32, &slot);
    let length = blk.load(I32, &walk.len_slot);
    let inside = blk.icmp_ult(I32, &counter, &length);
    blk.cond_br(&inside, body_label, exit_label);
}
