//! Merge checked and admitted element bindings before one receiver tail.
//!
//! The array's admission owns only the prefix. Its value is boxed in both
//! arms, so the existing per-element shape/representation decision can run
//! once after their join. No receiver fact crosses that join.
use super::*;

pub(super) fn shareable(
    _ctx: &FnCtx<'_>,
    receivers: &[Receiver],
    arrays: &[ArrayRecv],
    trees: &HashSet<usize>,
    recheck: Recheck,
    inner: &Pending,
    body: &[Stmt],
) -> bool {
    receivers.is_empty()
        && !arrays.is_empty()
        && arrays
            .iter()
            .all(|a| !a.store && !a.dense && a.view.is_none())
        && recheck != Recheck::Always
        && trees.is_empty()
        && body[..inner.split_at].iter().all(|s| {
            matches!(
                s,
                Stmt::Let {
                    init: Some(Expr::IndexGet { .. }),
                    ..
                }
            )
        })
        && inner.arrays.is_empty()
}

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    idx: usize,
    lower_list: fn(&mut FnCtx<'_>, &[Stmt]) -> Result<()>,
) -> Result<bool> {
    let p = ctx.region_loops[idx].clone();
    let Some(mut inner) = p.inner else {
        return Ok(false);
    };
    let Some(valid) = p.valid_slot else {
        return Ok(false);
    };
    if !shareable(
        ctx,
        &p.receivers,
        &p.arrays,
        &p.trees,
        p.recheck,
        &inner,
        body,
    ) {
        return Ok(false);
    }
    let dirty = p.dirty_slot.expect("an array loop has a dirty slot");
    let prefix = &body[..inner.split_at];
    let tail = &body[inner.split_at..];
    let check = ctx.new_block("rloop.element.recheck");
    let top = ctx.new_block("rloop.element.top");
    let fast = ctx.new_block("rloop.element.fast");
    let slow = ctx.new_block("rloop.element.slow");
    let join = ctx.new_block("rloop.element.join");
    let check_l = ctx.block_label(check);
    let top_l = ctx.block_label(top);
    let fast_l = ctx.block_label(fast);
    let slow_l = ctx.block_label(slow);
    let join_l = ctx.block_label(join);
    let stale = ctx.block().load(I1, &dirty);
    ctx.block().cond_br(&stale, &check_l, &top_l);
    ctx.current_block = check;
    let mut pass = "true".to_string();
    for a in &p.arrays {
        let ok = arrays::emit_guard(ctx, a)?;
        pass = ctx.block().and(I1, &pass, &ok);
    }
    if !p.entry_tests.is_empty() {
        let ok = emit_number_entry_tests(ctx, &p.entry_tests)?;
        pass = ctx.block().and(I1, &pass, &ok);
    }
    ctx.block().store(I1, &pass, &valid);
    ctx.block().store(I1, "false", &dirty);
    ctx.block().br(&top_l);
    ctx.current_block = top;
    let pass = ctx.block().load(I1, &valid);
    let decide = ctx.current_block;

    ctx.current_block = fast;
    let start = ctx.func.num_blocks();
    ctx.region_loop_facts.push(Active {
        receivers: Vec::new(),
        bare: p.bare,
        trees: HashSet::new(),
        dirty_slot: Some(dirty.clone()),
        emitted: Vec::new(),
        handles: Vec::new(),
        spill: false,
        arrays: p.arrays,
        emitted_arr: Vec::new(),
        view_index: HashSet::new(),
        dirty_after: HashSet::new(),
    });
    let result = lower_list(ctx, prefix);
    let active = ctx.region_loop_facts.pop().expect("prefix facts pushed");
    result?;
    let end = ctx.func.num_blocks();
    let ok = verify(ctx, fast, start, end, &active.emitted_arr)
        && verify::cannot_collect(ctx, fast, start, end);
    ctx.block().br(&join_l);

    ctx.current_block = slow;
    ctx.block().store(I1, "false", &valid);
    lower_list(ctx, prefix)?;
    // A checked element read may run a getter or move the array. Re-derive
    // its admission before another bare prefix, including when the row's
    // own shape passes immediately after this read.
    ctx.block().store(I1, "true", &dirty);
    ctx.block().br(&join_l);
    ctx.current_block = decide;
    if ok {
        ctx.block().cond_br(&pass, &fast_l, &slow_l);
    } else {
        ctx.block().br(&slow_l);
    }

    ctx.current_block = join;
    if !inner.receivers.iter().all(|rv| {
        receiver_eligible(ctx, rv.recv)
            && (rv.keys.len() <= MAX_KEYS || has_static_supplier(ctx, rv))
    }) {
        ctx.block().store(I1, "false", &valid);
        lower_list(ctx, tail)?;
        return Ok(true);
    }
    inner.body_ptr = tail.as_ptr() as usize;
    inner.body_len = tail.len();
    inner.split_at = 0;
    inner.parent_valid = Some(valid);
    // The parent tested these Numbers. The shared receiver decision tests
    // parent validity and publishes them only in F; G clears that validity
    // before an unproven write can reach another F iteration.
    inner.number_locals = p.number_locals;
    inner.retry = None;
    ctx.region_loops.push(*inner);
    let nested_idx = ctx.region_loops.len() - 1;
    let result = lower_split(ctx, tail, nested_idx, lower_list);
    ctx.region_loops.truncate(nested_idx);
    result?;
    Ok(true)
}
