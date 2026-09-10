//! Hoist one invariant indexed own-number read out of a numeric reduction.
//!
//! This deliberately narrow clone accepts `acc = acc + arr[constant].field`.
//! The positive trip-count guard precedes all receiver inspection. A leaf probe
//! reads only own data, never a getter/coercion, and returns a genuine double or
//! a miss. The admitted loop contains only scalar arithmetic; it has no borrowed
//! heap pointers, allocations, callbacks, or GC polls. All rejected cases enter
//! the existing loop with pristine accumulator/counter state.

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UpdateOp};

use super::loops::{emit_js_value_is_number, lower_for_after_init};
use crate::expr::{lower_expr, FnCtx};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

struct Reduction<'a> {
    counter: u32,
    start: i32,
    bound: &'a Expr,
    accumulator: u32,
    array: u32,
    index: u32,
    property: &'a str,
}

fn integer(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Integer(n) => Some(*n),
        Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => Some(*n as i64),
        _ => None,
    }
}

// Require ordinary directly-addressed locals. A captured/boxed/scalar-replaced
// binding has a different read/write contract and must retain its own lowering.
fn plain_local(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.locals.contains_key(&id) && unaliased_local(ctx, id)
}

fn unaliased_local(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.module_globals.contains_key(&id)
        && !ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        && !ctx.pod_records.contains_key(&id)
        && !ctx.scalar_replaced.contains_key(&id)
        && !ctx.scalar_replaced_arrays.contains_key(&id)
        && !ctx.array_row_aliases.contains_key(&id)
        && !ctx.numeric_accumulator_f64_slots.contains_key(&id)
        && !ctx.deferred_integer_update_accumulators.contains(&id)
}

fn matches<'a>(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&'a Expr>,
    update: Option<&Expr>,
    body: &'a [Stmt],
) -> Option<Reduction<'a>> {
    if crate::codegen::full_outline_ic_enabled() || !ctx.pending_labels.is_empty() {
        return None;
    }
    let Stmt::Let {
        id: counter,
        init: Some(start),
        ..
    } = init?
    else {
        return None;
    };
    let start = i32::try_from(integer(start)?).ok()?;
    if start < 0
        || !unaliased_local(ctx, *counter)
        || !(ctx.locals.contains_key(counter)
            || (ctx.local_slot_reps.get(counter) == Some(&crate::expr::SlotRep::I32)
                && ctx.i32_counter_slots.contains_key(counter)))
    {
        return None;
    }
    let Expr::Compare {
        op: CompareOp::Lt,
        left,
        right: bound,
    } = condition?
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if id == counter)
        || !matches!(update?, Expr::Update { id, op: UpdateOp::Increment, .. } if id == counter)
    {
        return None;
    }
    let [Stmt::Expr(Expr::LocalSet(accumulator, value))] = body else {
        return None;
    };
    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if id == accumulator)
        || accumulator == counter
        || !plain_local(ctx, *accumulator)
        || ctx.i32_counter_slots.contains_key(accumulator)
    {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = right.as_ref()
    else {
        return None;
    };
    let Expr::IndexGet { object, index } = object.as_ref() else {
        return None;
    };
    let Expr::LocalGet(array) = object.as_ref() else {
        return None;
    };
    let index = u32::try_from(integer(index)?).ok()?;
    if index == u32::MAX
        || array == accumulator
        || array == counter
        || !plain_local(ctx, *array)
        || super::loops::CLASS_FIELD_LOOP_PROP_DENYLIST.contains(&property.as_str())
    {
        return None;
    }
    match bound.as_ref() {
        Expr::LocalGet(id)
            if id != counter && id != accumulator && id != array && plain_local(ctx, *id) => {}
        expr if integer(expr).is_some_and(|n| (0..=i64::from(i32::MAX)).contains(&n)) => {}
        _ => return None,
    }
    Some(Reduction {
        counter: *counter,
        start,
        bound,
        accumulator: *accumulator,
        array: *array,
        index,
        property,
    })
}

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(m) = matches(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    let slow = ctx.new_block("invariant_field.slow");
    let merge = ctx.new_block("invariant_field.merge");
    let range = ctx.new_block("invariant_field.range");
    let convert = ctx.new_block("invariant_field.convert");
    let probe = ctx.new_block("invariant_field.probe");
    let fast_pre = ctx.new_block("invariant_field.fast.preheader");
    let fast_body = ctx.new_block("invariant_field.fast.body");
    let fast_exit = ctx.new_block("invariant_field.fast.exit");
    let slow_label = ctx.block_label(slow);
    let merge_label = ctx.block_label(merge);
    let range_label = ctx.block_label(range);
    let convert_label = ctx.block_label(convert);
    let probe_label = ctx.block_label(probe);
    let fast_pre_label = ctx.block_label(fast_pre);
    let fast_body_label = ctx.block_label(fast_body);
    let fast_exit_label = ctx.block_label(fast_exit);

    let bound = lower_expr(ctx, m.bound)?;
    let number = emit_js_value_is_number(ctx, &bound);
    ctx.block().cond_br(&number, &range_label, &slow_label);
    ctx.current_block = range;
    // Even null/proxy/getter receivers remain completely untouched when the
    // source loop has zero trips. Negative/fractional/NaN bounds use fallback.
    let positive = ctx
        .block()
        .fcmp("ogt", &bound, &format!("{:.1}", m.start as f64));
    let bounded = ctx.block().fcmp("ole", &bound, "2147483647.0");
    let ok = ctx.block().and(I1, &positive, &bounded);
    ctx.block().cond_br(&ok, &convert_label, &slow_label);
    ctx.current_block = convert;
    let count = ctx.block().fptosi(DOUBLE, &bound, I32);
    let roundtrip = ctx.block().sitofp(I32, &count, DOUBLE);
    let integral = ctx.block().fcmp("oeq", &bound, &roundtrip);
    ctx.block().cond_br(&integral, &probe_label, &slow_label);

    ctx.current_block = probe;
    let accumulator_slot = ctx.locals[&m.accumulator].clone();
    let counter_slot = ctx.locals.get(&m.counter).cloned();
    let counter_i32_slot = ctx.i32_counter_slots.get(&m.counter).cloned();
    let accumulator = ctx.block().load(DOUBLE, &accumulator_slot);
    let acc_number = emit_js_value_is_number(ctx, &accumulator);
    let receiver = lower_expr(ctx, &Expr::LocalGet(m.array))?;
    let key = ctx.strings.intern(m.property);
    let key_bytes = format!("@{}", ctx.strings.entry(key).bytes_global);
    let value = ctx.block().call(
        DOUBLE,
        "js_array_index_own_number",
        &[
            (DOUBLE, &receiver),
            (I32, &m.index.to_string()),
            (PTR, &key_bytes),
            (I64, &m.property.len().to_string()),
        ],
    );
    let value_number = emit_js_value_is_number(ctx, &value);
    let admitted = ctx.block().and(I1, &acc_number, &value_number);
    let admission_block = ctx.current_block;

    // Separate unrooted scalar slots let mem2reg keep the reduction in SSA.
    // No reassociation: preserve every sequential IEEE addition and signed zero.
    let sum_slot = ctx.func.alloca_entry(DOUBLE);
    let index_slot = ctx.func.alloca_entry(I32);
    ctx.current_block = fast_pre;
    ctx.block().store(DOUBLE, &accumulator, &sum_slot);
    ctx.block().store(I32, &m.start.to_string(), &index_slot);
    ctx.block().br(&fast_body_label);
    ctx.current_block = fast_body;
    let sum = ctx.block().load(DOUBLE, &sum_slot);
    let next_sum = ctx.block().fadd(&sum, &value);
    ctx.block().store(DOUBLE, &next_sum, &sum_slot);
    let index = ctx.block().load(I32, &index_slot);
    let next_index = ctx.block().add(I32, &index, "1");
    ctx.block().store(I32, &next_index, &index_slot);
    let more = ctx.block().icmp_slt(I32, &next_index, &count);
    ctx.block()
        .cond_br(&more, &fast_body_label, &fast_exit_label);
    ctx.current_block = fast_exit;
    ctx.block().store(DOUBLE, &next_sum, &accumulator_slot);
    if let Some(slot) = counter_i32_slot {
        ctx.block().store(I32, &next_index, &slot);
    }
    if let Some(slot) = counter_slot {
        let end = ctx.block().sitofp(I32, &next_index, DOUBLE);
        ctx.block().store(DOUBLE, &end, &slot);
    }
    ctx.block().br(&merge_label);
    // Post-lowering certification is mandatory even for this hand-built body.
    let call_free = [fast_pre, fast_body, fast_exit]
        .into_iter()
        .all(|i| !ctx.func.blocks()[i].contains_gc_unsafe_call());
    ctx.current_block = admission_block;
    if call_free {
        ctx.block().cond_br(&admitted, &fast_pre_label, &slow_label);
    } else {
        ctx.block().br(&slow_label);
    }

    ctx.current_block = slow;
    lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.invariant_field_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge;
    Ok(true)
}
