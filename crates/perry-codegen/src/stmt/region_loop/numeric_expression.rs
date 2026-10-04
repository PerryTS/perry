//! A single numeric comparison within the existing loop and exception scope.
//!
//! The statement/handler tree is lowered once. Only the named read and its
//! comparison are versioned, after every earlier evaluation effect. No fact
//! escapes the expression, and the generic arm sees the original operands.

use super::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
thread_local! {
    static TEST_REENTER_BEFORE_READ: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn named_read<'a>(ctx: &FnCtx<'_>, e: &'a Expr) -> Option<(Recv, &'a str)> {
    let Expr::PropertyGet {
        object, property, ..
    } = e
    else {
        return None;
    };
    let recv = Recv::of(object)?;
    if !receiver_eligible(ctx, recv)
        || matches!(recv, Recv::Local(id) if ctx.closure_captures.contains_key(&id))
    {
        return None;
    }
    Some((recv, property))
}

fn number_literal(e: &Expr) -> bool {
    match e {
        Expr::Integer(_) => true,
        // Mirror the strict entry predicate, including negative NaN payloads.
        Expr::Number(n) => n.to_bits() & 0x7fff_ffff_ffff_ffff < 0x7ff9_0000_0000_0000,
        _ => false,
    }
}

fn active(receivers: Vec<Receiver>, read: Option<&Expr>) -> Active {
    Active {
        receivers,
        bare: read
            .into_iter()
            .map(|e| e as *const Expr as usize)
            .collect(),
        trees: HashSet::new(),
        dirty_slot: None,
        emitted: Vec::new(),
        handles: Vec::new(),
        spill: false,
        arrays: Vec::new(),
        emitted_arr: Vec::new(),
        view_index: HashSet::new(),
        dirty_after: HashSet::new(),
    }
}

pub(crate) fn try_lower_numeric_compare(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    generic: fn(&mut FnCtx<'_>, &Expr) -> Result<String>,
) -> Result<Option<String>> {
    if disabled()
        || ctx.loop_targets.is_empty()
        || !ctx.region_loop_facts.is_empty()
        || !ctx.label_targets.is_empty()
        || !ctx.pending_labels.is_empty()
    {
        return Ok(None);
    }
    let Expr::Compare { op, left, right } = expr else {
        return Ok(None);
    };
    if !matches!(
        op,
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
    ) {
        return Ok(None);
    }
    if expr_refused(left) || expr_refused(right) {
        return Ok(None);
    }
    // A right read admits an arbitrary, already-evaluated left value. A left
    // read admits only an inert Number literal on the right: no later getter
    // or callback may run between the guard and that bare read.
    let (read, recv, key, read_left) = if let Some((recv, key)) = named_read(ctx, right) {
        (right.as_ref(), recv, key, false)
    } else if number_literal(right) {
        let Some((recv, key)) = named_read(ctx, left) else {
            return Ok(None);
        };
        (left.as_ref(), recv, key, true)
    } else {
        return Ok(None);
    };
    // Only the read and comparison are copied, regardless of how expensive
    // evaluating the saved left operand is. Retain the region cost gate.
    if !pays(ctx, "numeric-expression", 4, 1) {
        return Ok(None);
    }
    if read_left {
        return emit(ctx, expr, *op, left, right, read, recv, key, true, generic).map(Some);
    }
    // This root owns the LEFT GetValue, not the right receiver. The exact-node
    // materialization hook makes both arms reuse it, even if a guard helper
    // collects. It also preserves one evaluation when the left getter mutates
    // the right receiver's shape, descriptor, prototype or representation.
    let saved = lower_expr(ctx, left)?;
    crate::rooting::with_materialized_receiver(
        ctx,
        left.as_ref() as *const Expr as usize,
        &saved,
        |ctx| emit(ctx, expr, *op, left, right, read, recv, key, false, generic),
    )
    .map(Some)
}

#[allow(clippy::too_many_arguments)]
fn emit(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    op: CompareOp,
    left: &Expr,
    right: &Expr,
    read: &Expr,
    recv: Recv,
    key: &str,
    read_left: bool,
    generic: fn(&mut FnCtx<'_>, &Expr) -> Result<String>,
) -> Result<String> {
    let mut rv = Receiver {
        recv,
        uses_ptr_shape_class: false,
        keys: vec![key.to_string()],
        has_store: false,
        // This expression never stores. R requires an inline lane (an
        // identity F64 one, or a value-tested one), so it also refuses spill
        // words without a store bit.
        stored_mask: 0,
        boxed_mask: 0,
        r_mask: 1,
        vt_mask: 1,
        spill: "false".to_string(),
        sites: None,
        word: String::new(),
        expected_shape: None,
        slots: Vec::new(),
        static_slots: None,
        inherited: false,
    };
    let (sites, word) = emit_guard_word(ctx, &mut rv);
    decode_slots(ctx, &mut rv, &word);
    let guard = ctx.current_block;
    let number = ctx.new_block("rexpr.number");
    let fast = ctx.new_block("rexpr.fast");
    let slow = ctx.new_block("rexpr.slow");
    let join = ctx.new_block("rexpr.merge");
    let number_l = ctx.block_label(number);
    let fast_l = ctx.block_label(fast);
    let slow_l = ctx.block_label(slow);
    let join_l = ctx.block_label(join);

    ctx.current_block = number;
    if read_left {
        ctx.block().br(&fast_l);
    } else {
        let saved = lower_expr(ctx, left)?;
        let canonical = crate::stmt::loops::emit_js_value_is_number(ctx, &saved);
        ctx.block().cond_br(&canonical, &fast_l, &slow_l);
    }

    ctx.current_block = fast;
    ctx.region_loop_facts
        .push(active(vec![rv.clone()], Some(read)));
    let result: Result<String> = (|| {
        // Negative control: the emitted verifier must discard a genuinely
        // JS-capable invoke inserted before the protected load. Test-only,
        // thread-local, and absent from shipping builds and feature graphs.
        #[cfg(test)]
        if TEST_REENTER_BEFORE_READ.with(|flag| flag.get()) {
            ctx.block()
                .call(DOUBLE, "js_rel_lt", &[(DOUBLE, "0.0"), (DOUBLE, "0.0")]);
        }
        // The entry Number proof applies to the saved operand only. Do not
        // materialize a Number fact for its source local or any later read.
        let l = lower_expr(ctx, left)?;
        let r = lower_expr(ctx, right)?;
        note(ctx, Route::RloopF);
        note(ctx, Route::RloopFRep);
        let pred = match op {
            CompareOp::Lt => "olt",
            CompareOp::Le => "ole",
            CompareOp::Gt => "ogt",
            CompareOp::Ge => "oge",
            _ => unreachable!("relational admission above"),
        };
        let bit = ctx.block().fcmp(pred, &l, &r);
        let bits = ctx.block().select(
            I1,
            &bit,
            I64,
            crate::nanbox::TAG_TRUE_I64,
            crate::nanbox::TAG_FALSE_I64,
        );
        Ok(ctx.block().bitcast_i64_to_double(&bits))
    })();
    // Restore on compiler error as well as normal paths. No unwind path is
    // lowered while these facts are installed: only a bare load and fcmp.
    let facts = ctx
        .region_loop_facts
        .pop()
        .expect("expression fact installed");
    let f = result?;
    let f_end = ctx.block().label.clone();
    ctx.block().br(&join_l);
    ctx.current_block = slow;
    note(ctx, Route::RloopG);
    // Empty innermost facts suppress overlapping expression selection while
    // the same original expression is lowered by the generic comparator.
    ctx.region_loop_facts.push(active(Vec::new(), None));
    let result = generic(ctx, expr);
    ctx.region_loop_facts
        .pop()
        .expect("generic suppression installed");
    let g = result?;
    let g_end = ctx.block().label.clone();
    ctx.block().br(&join_l);

    // Walk the actual guard-to-read path before committing the entry edge.
    // A future JS-capable invoke inserted anywhere in that window discards F.
    let check = ctx.new_block("rexpr.guard");
    let check_l = ctx.block_label(check);
    ctx.current_block = check;
    // Prime is bounded and publishes only for the next visit. Guard after
    // left effects with the same full descriptor/prototype/rep admission.
    emit_body_guard_direct(ctx, &rv, &sites, &word, &[], &number_l, &slow_l, &slow_l)?;
    let verified = facts.emitted.len() == 1
        && verify(ctx, check, number, ctx.func.num_blocks(), &facts.emitted);
    if !verified {
        stat(4, 1);
    }
    ctx.current_block = guard;
    ctx.block().br(if verified { &check_l } else { &slow_l });
    ctx.current_block = join;
    Ok(ctx.block().phi(DOUBLE, &[(&f, &f_end), (&g, &g_end)]))
}
