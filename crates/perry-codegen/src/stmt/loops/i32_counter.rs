use super::*;

/// Select the counter representation once, rather than hoping LLVM unswitches
/// the dynamic-bound flag past the body's calls / safepoint polls. The entry
/// guard proves `start <= INT32_MAX` and `n <= INT32_MAX`; with only `++` and
/// strict `<`, every taken update ends at most at `n`, so no overflow edge is
/// reachable in the integer version. Larger bounds use doubles from entry.
///
/// Deliberately limited to straight-line expression bodies, outside try and
/// labelled regions: every normal exit reaches the double resynchronization,
/// and there is no catch / labelled break that can observe a stale slot.
/// Captures and async contexts retain their existing storage protocol. The
/// literal initializer also excludes -0, which an i32 view cannot preserve.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    use perry_hir::{CompareOp, Expr};
    let Some(Stmt::Let {
        id,
        init: Some(Expr::Integer(start)),
        ..
    }) = init
    else {
        return Ok(false);
    };
    let counter_id = *id;
    if !crate::expr::canonical_i32_locals_enabled()
        || i32::try_from(*start).is_err()
        || !ctx.repsel_context_allows_canonical_i32
        || ctx.try_depth != 0
        || !ctx.pending_labels.is_empty()
        || ctx.repsel_closure_ref_locals.contains(&counter_id)
        || ctx.closure_captures.contains_key(&counter_id)
        || ctx.i32_counter_slots.contains_key(&counter_id)
        || ctx.local_slot_reps.contains_key(&counter_id)
        || !body.iter().all(|stmt| matches!(stmt, Stmt::Expr(_)))
        || !dynamic_bound_private_counter_is_safe(ctx, counter_id, update, body)
    {
        return Ok(false);
    }
    let Some((matched_counter, bound_id, CompareOp::Lt)) =
        condition.and_then(|cond| classify_for_local_bound_dynamic(cond, update, body, ctx))
    else {
        return Ok(false);
    };
    if matched_counter != counter_id || ctx.closure_captures.contains_key(&bound_id) {
        return Ok(false);
    }
    let Some(guard) = emit_guarded_i32_bound(
        ctx,
        counter_id,
        bound_id,
        CompareOp::Lt,
        update,
        body,
        "for.iv",
    ) else {
        return Ok(false);
    };
    let fast_idx = ctx.new_block("for.iv.fast.preheader");
    let slow_idx = ctx.new_block("for.iv.slow.preheader");
    let merge_idx = ctx.new_block("for.iv.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    let flag = ctx.block().load(I1, &guard.flag_slot);
    ctx.block().cond_br(&flag, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let bound = ctx.block().load(I32, &guard.bound_i32_slot);
    // Scoped canonical storage: LocalGet materializes a double only where
    // used, and Update touches only i32. Never publish this slot to the slow
    // version: its counter may legitimately cross 2^31 (the #6072 regression).
    ctx.i32_counter_slots
        .insert(counter_id, guard.counter_i32_slot.clone());
    ctx.local_slot_reps
        .insert(counter_id, crate::expr::SlotRep::I32);
    lower_for_after_init_impl(
        ctx,
        init,
        condition,
        update,
        body,
        "for.iv.fast",
        Some((counter_id, bound)),
        false,
    )?;
    let counter = ctx.block().load(I32, &guard.counter_i32_slot);
    let counter_double = ctx.block().sitofp(I32, &counter, DOUBLE);
    let double_slot = ctx.locals[&counter_id].clone();
    ctx.block().store(DOUBLE, &counter_double, &double_slot);
    ctx.block().br(&merge_label);
    ctx.local_slot_reps.remove(&counter_id);
    ctx.i32_counter_slots.remove(&counter_id);

    ctx.current_block = slow_idx;
    lower_for_after_init_impl(
        ctx,
        init,
        condition,
        update,
        body,
        "for.iv.slow",
        None,
        false,
    )?;
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    Ok(true)
}
