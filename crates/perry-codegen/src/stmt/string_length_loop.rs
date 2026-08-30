//! Versioned masked string-array `.length` accumulation loops (#9160).

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UpdateOp};

use super::loops::{
    emit_js_value_is_number, lower_for_after_init, lower_for_after_init_with_i32_bound,
    packed_loop_array_binding_storage_is_addressable,
};
use crate::expr::{lower_expr, FnCtx, StringWindowArrayFact};
use crate::types::{DOUBLE, I1, I32};

struct Matched {
    counter_id: u32,
    bound: i64,
    array_id: u32,
    accumulator_id: u32,
    min_idx: i64,
    max_idx_exclusive: i64,
}

fn is_declared_string_array(ctx: &FnCtx<'_>, id: u32) -> bool {
    use perry_hir::types::Type;
    let is_string = |ty: &Type| matches!(ty, Type::String | Type::StringLiteral(_));
    match crate::type_analysis::static_type_of(ctx, &Expr::LocalGet(id)) {
        Some(Type::Array(element)) => is_string(&element),
        Some(Type::Generic { base, type_args }) if base == "Array" && type_args.len() == 1 => {
            is_string(&type_args[0])
        }
        _ => false,
    }
}

/// A deliberately closed, side-effect-free subset for masked index trees.
/// `static_index_window` supplies the range proof; this walk makes it safe to
/// keep that proof for the whole call-free fast clone.
fn index_tree_is_pure(expr: &Expr, counter_id: u32) -> bool {
    match expr {
        Expr::LocalGet(id) => *id == counter_id,
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            index_tree_is_pure(left, counter_id) && index_tree_is_pure(right, counter_id)
        }
        Expr::Unary { operand, .. } | Expr::NumberCoerce(operand) => {
            index_tree_is_pure(operand, counter_id)
        }
        _ => false,
    }
}

fn match_length_read(expr: &Expr, counter_id: u32) -> Option<(u32, i64, i64)> {
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return None;
    };
    if property != "length" {
        return None;
    }
    let Expr::IndexGet { object, index } = object.as_ref() else {
        return None;
    };
    let Expr::LocalGet(array_id) = object.as_ref() else {
        return None;
    };
    let (lo, hi) = crate::collectors::static_index_window(index)?;
    if lo < 0 || hi >= i64::from(i32::MAX) || !index_tree_is_pure(index, counter_id) {
        return None;
    }
    Some((*array_id, lo, hi))
}

fn match_loop(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Matched> {
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let counter_id = match init? {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        } => *id,
        Stmt::Let {
            id,
            init: Some(Expr::Number(n)),
            ..
        } if *n == 0.0 => *id,
        _ => return None,
    };
    let bound = match condition? {
        Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) => {
            match right.as_ref() {
                Expr::Integer(n) if (0..=i64::from(i32::MAX)).contains(n) => *n,
                _ => return None,
            }
        }
        _ => return None,
    };
    if !matches!(
        update?,
        Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        } if *id == counter_id
    ) {
        return None;
    }
    if ctx.boxed_vars.contains(&counter_id)
        || (!ctx.locals.contains_key(&counter_id) && !ctx.local_slot_reps.contains_key(&counter_id))
    {
        return None;
    }

    let (accumulator_id, value) = match body {
        [Stmt::Expr(Expr::LocalSet(id, value))] => (*id, value.as_ref()),
        _ => return None,
    };
    let (array_id, lo, hi) = match value {
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == accumulator_id) => {
            match_length_read(right, counter_id)?
        }
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } if matches!(right.as_ref(), Expr::LocalGet(id) if *id == accumulator_id) => {
            match_length_read(left, counter_id)?
        }
        _ => return None,
    };
    if accumulator_id == counter_id
        || accumulator_id == array_id
        || ctx.boxed_vars.contains(&accumulator_id)
        || ctx.closure_captures.contains_key(&accumulator_id)
        || (!ctx.locals.contains_key(&accumulator_id)
            && !ctx.local_slot_reps.contains_key(&accumulator_id))
        || !packed_loop_array_binding_storage_is_addressable(ctx, array_id)
        || ctx.scalar_replaced_arrays.contains_key(&array_id)
        || !is_declared_string_array(ctx, array_id)
    {
        return None;
    }

    Some(Matched {
        counter_id,
        bound,
        array_id,
        accumulator_id,
        min_idx: lo,
        max_idx_exclusive: hi + 1,
    })
}

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(matched) = match_loop(ctx, init, condition, update, body) else {
        return Ok(false);
    };

    let mut counter_slot_was_fresh = false;
    if !ctx.i32_counter_slots.contains_key(&matched.counter_id) {
        let slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, "0", &slot);
        ctx.i32_counter_slots.insert(matched.counter_id, slot);
        counter_slot_was_fresh = true;
    }

    let accumulator = lower_expr(ctx, &Expr::LocalGet(matched.accumulator_id))?;
    let array = lower_expr(ctx, &Expr::LocalGet(matched.array_id))?;
    let array_ok_i32 = ctx.block().call(
        I32,
        "js_string_array_range_loop_guard",
        &[
            (DOUBLE, &array),
            (I32, &matched.min_idx.to_string()),
            (I32, &matched.max_idx_exclusive.to_string()),
        ],
    );
    let array_ok = ctx.block().icmp_ne(I32, &array_ok_i32, "0");
    let accumulator_ok = emit_js_value_is_number(ctx, &accumulator);
    let fast_ok = ctx.block().and(I1, &array_ok, &accumulator_ok);

    let fast_idx = ctx.new_block("string_length.loop.fast.preheader");
    let slow_idx = ctx.new_block("string_length.loop.slow.preheader");
    let merge_idx = ctx.new_block("string_length.loop.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&fast_ok, &fast_label, &slow_label);

    ctx.current_block = fast_idx;
    let scope_id = ctx.next_loop_proof_scope_id();
    ctx.string_window_array_facts.push(StringWindowArrayFact {
        array_local_id: matched.array_id,
        scope_id,
        min_idx: matched.min_idx,
        max_idx_exclusive: matched.max_idx_exclusive,
        numeric_accumulator: matched.accumulator_id,
    });
    let saved_stride = ctx.poll_stride_counter_slot.take();
    ctx.poll_stride_counter_slot = ctx.i32_counter_slots.get(&matched.counter_id).cloned();
    lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.string_length_fast",
        Some((matched.counter_id, matched.bound.to_string())),
    )?;
    ctx.poll_stride_counter_slot = saved_stride;
    ctx.string_window_array_facts
        .retain(|fact| fact.scope_id != scope_id);
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = slow_idx;
    lower_for_after_init(ctx, init, condition, update, body, "for.string_length_slow")?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    if counter_slot_was_fresh {
        ctx.i32_counter_slots.remove(&matched.counter_id);
    }
    ctx.current_block = merge_idx;
    Ok(true)
}
