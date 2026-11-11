use super::*;

/// Select the counter representation once, rather than hoping LLVM unswitches
/// the dynamic-bound flag past the body's calls / safepoint polls. The entry
/// guard proves `start <= INT32_MAX` and `n <= INT32_MAX`; with only `++` and
/// strict `<`, every taken update ends at most at `n`, so no overflow edge is
/// reachable in the integer version. Larger bounds use doubles from entry.
///
/// The body may branch, call and nest loops, but every edge that leaves the
/// integer copy while the frame lives must reach the double
/// resynchronization at its exit: unlabelled `break` and labels declared in
/// the body stay inside the loop (`jumps_stay_in_loop`), `return` / `throw`
/// leave the frame (no `try` around the loop here, so no catch observes a
/// stale slot), and the loop itself carries no label. Captures and async
/// contexts retain their existing storage protocol. The literal initializer
/// (`let i = 0`, `i = 0`, `i = 0, n = a.length`) also excludes -0, which an
/// i32 view cannot preserve.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some((counter_id, guard)) = prepare(ctx, init, condition, update, body, false) else {
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
    lower_proven(ctx, counter_id, &guard, init, condition, update, body)?;
    ctx.block().br(&merge_label);

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
        None,
    )?;
    ctx.block().br(&merge_label);
    ctx.current_block = merge_idx;
    Ok(true)
}

/// A receiver region can include this range proof in its existing entry
/// decision. Its plain loop is already the semantic fallback; emitting a
/// second pair of integer/double loops inside that region would copy it again.
pub(super) fn prepare(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    bindings: bool,
) -> Option<(u32, DynamicI32Bound)> {
    use perry_hir::{CompareOp, Expr};
    let Some((counter_id, bound_id, CompareOp::Lt)) =
        condition.and_then(|cond| classify_for_local_bound_dynamic(cond, update, body, ctx))
    else {
        return None;
    };
    let start = for_init_literal_counter(init, counter_id)?;
    let canonical = bindings
        && ctx.local_slot_reps.get(&counter_id) == Some(&crate::expr::SlotRep::I32)
        && ctx.i32_counter_slots.contains_key(&counter_id);
    let private_update = canonical
        && !ctx.boxed_vars.contains(&counter_id)
        && !ctx.module_globals.contains_key(&counter_id)
        && matches!(update, Some(Expr::Update {
            id, op: perry_hir::UpdateOp::Increment, ..
        }) if *id == counter_id)
        && !stmts_mutate_local(body, counter_id);
    if !crate::expr::canonical_i32_locals_enabled()
        || !ctx.repsel_context_allows_canonical_i32
        || ctx.try_depth != 0
        || !ctx.pending_labels.is_empty()
        || ctx.repsel_closure_ref_locals.contains(&counter_id)
        || ctx.closure_captures.contains_key(&counter_id)
        || (!bindings && ctx.i32_counter_slots.contains_key(&counter_id))
        || (ctx.local_slot_reps.contains_key(&counter_id) && !canonical)
        || !jumps_stay_in_loop(body)
        || !(private_update || dynamic_bound_private_counter_is_safe(ctx, counter_id, update, body))
        || ctx.closure_captures.contains_key(&bound_id)
    {
        return None;
    }
    // A native i32 bound already carries the range/integrality contract.
    // Reuse its slot, including specialized entries that have no double
    // storage. Literal start plus strict `<` proves every increment fits.
    let mut guard =
        if bindings && ctx.local_slot_reps.get(&bound_id) == Some(&crate::expr::SlotRep::I32) {
            let bound_i32_slot = ctx.i32_counter_slots.get(&bound_id)?.clone();
            let shared = ctx.i32_counter_slots.get(&counter_id).cloned();
            let counter_is_private = shared.is_none();
            let counter_i32_slot = shared.unwrap_or_else(|| {
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &start.to_string(), &slot);
                slot
            });
            let flag_slot = ctx.func.alloca_entry(I1);
            ctx.block().store(I1, "true", &flag_slot);
            DynamicI32Bound {
                op: CompareOp::Lt,
                flag_slot,
                bound_i32_slot,
                counter_i32_slot,
                counter_is_private,
            }
        } else {
            emit_guarded_i32_bound_with_start(
                ctx,
                counter_id,
                bound_id,
                CompareOp::Lt,
                update,
                body,
                "for.iv",
                None,
                Some(start),
            )?
        };
    if canonical {
        // An entry miss may have a bound beyond i32. Give the checked
        // loop Number storage instead of leaving `++` on a canonical i32
        // slot that would wrap at 2^31. The fast copy borrows the old slot
        // only under its range guard and resynchronizes this Number slot.
        let slot = ctx.func.alloca_entry(DOUBLE);
        let current = ctx.block().load(I32, &guard.counter_i32_slot);
        let number = ctx.block().sitofp(I32, &current, DOUBLE);
        ctx.block().store(DOUBLE, &number, &slot);
        ctx.locals.insert(counter_id, slot);
        ctx.local_slot_reps.remove(&counter_id);
        ctx.i32_counter_slots.remove(&counter_id);
        guard.counter_is_private = true;
    }
    Some((counter_id, guard))
}

/// Called only in a branch dominated by the prepared guard's passing flag.
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_proven(
    ctx: &mut FnCtx<'_>,
    counter_id: u32,
    guard: &DynamicI32Bound,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<()> {
    let bound = ctx.block().load(I32, &guard.bound_i32_slot);
    // Scoped canonical storage: LocalGet materializes a double only where
    // used, and Update touches only i32. Never publish this slot to the slow
    // version: its counter may legitimately cross 2^31 (the #6072 regression).
    ctx.i32_counter_slots
        .insert(counter_id, guard.counter_i32_slot.clone());
    let previous_rep = ctx
        .local_slot_reps
        .insert(counter_id, crate::expr::SlotRep::I32);
    // The i32 slot is the counter's only storage inside this copy, exactly
    // as for a canonical-i32 local (which has no `ctx.locals` entry): a
    // nested loop, call argument or branch in the body can then never read
    // the double slot, which is stale until the exit below.
    let double_slot = if previous_rep != Some(crate::expr::SlotRep::I32) {
        ctx.locals.remove(&counter_id)
    } else {
        None
    };
    let lowered = lower_for_after_init_impl(
        ctx,
        init,
        condition,
        update,
        body,
        "for.iv.fast",
        Some((counter_id, bound)),
        false,
        None,
    );
    if let Some(double_slot) = double_slot {
        // The copy's one exit block: the failed cond, `break`, and labels
        // declared in the body all land here (`jumps_stay_in_loop`).
        if lowered.is_ok() && !ctx.block().is_terminated() {
            let counter = ctx.block().load(I32, &guard.counter_i32_slot);
            let counter_double = ctx.block().sitofp(I32, &counter, DOUBLE);
            ctx.block().store(DOUBLE, &counter_double, &double_slot);
        }
        ctx.locals.insert(counter_id, double_slot);
    }
    match previous_rep {
        Some(rep) => {
            ctx.local_slot_reps.insert(counter_id, rep);
        }
        None => {
            ctx.local_slot_reps.remove(&counter_id);
        }
    }
    if guard.counter_is_private {
        ctx.i32_counter_slots.remove(&counter_id);
    }
    lowered
}

/// Every labelled `break` / `continue` in `body` (closures excluded: they
/// cannot jump out of this frame) names a label declared inside `body`, so
/// no jump bypasses the loop's own exit or update block.
fn jumps_stay_in_loop(body: &[Stmt]) -> bool {
    let mut declared = Vec::new();
    let mut targets = Vec::new();
    perry_hir::tdz_check::for_each_stmt_shallow(body, &mut |stmt| match stmt {
        Stmt::Labeled { label, .. } => declared.push(label.clone()),
        Stmt::LabeledBreak(label) | Stmt::LabeledContinue(label) => targets.push(label.clone()),
        _ => {}
    });
    targets.iter().all(|label| declared.contains(label))
}
