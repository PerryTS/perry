//! One canonical shape guard for a counted, non-dispatching read loop.
//!
//! The prediction is the ordered list of keys in the reduction. It is resolved
//! through the runtime's canonical-key tree, not learned from a receiver or IC.
//! Exact identity proves ordinary, generation-zero, hole-free, inline slots.
//! The closed body grammar below and numeric prechecks exclude user callbacks.
//! Polls remain enabled; every read re-derives the address from the receiver root.

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64};
use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UpdateOp};

#[derive(Clone, Debug)]
pub(crate) struct Fact {
    receiver: u32,
    counter: u32,
    invariant_values: Vec<(u32, String)>,
    keys: Vec<String>,
    accumulator: u32,
    numeric_locals: Vec<u32>,
    allocation_local: Option<u32>,
}

fn collect(expr: &Expr, fact: &mut Fact) -> bool {
    match expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => collect(left, fact) && collect(right, fact),
        Expr::Number(_) | Expr::Integer(_) => true,
        Expr::LocalGet(id) => {
            if !fact.numeric_locals.contains(id) {
                fact.numeric_locals.push(*id);
            }
            true
        }
        Expr::PropertyGet {
            object, property, ..
        } => {
            let Expr::LocalGet(id) = object.as_ref() else {
                return false;
            };
            if fact.keys.is_empty() {
                fact.receiver = *id;
            }
            if fact.receiver != *id || property.as_bytes().contains(&0) {
                return false;
            }
            if !fact.keys.contains(property) {
                fact.keys.push(property.clone());
            }
            true
        }
        _ => false,
    }
}

fn plan(
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Fact> {
    let counter = match init? {
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
    if !matches!(update?, Expr::Update { id, op: UpdateOp::Increment, .. } if *id == counter) {
        return None;
    }
    let bound = match condition? {
        Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter) => right.as_ref(),
        _ => return None,
    };
    // The sole allocating extension: assigning a fresh array whose elements
    // are primitive literals or the numeric counter to an otherwise unread
    // local. Array-literal construction cannot dispatch a getter/setter or
    // mutate the receiver; it may collect. No spread, method, or user call.
    let (allocation_local, reduction) = match body {
        [reduction] => (None, reduction),
        [Stmt::Expr(Expr::LocalSet(id, value)), reduction] => {
            let Expr::Array(items) = value.as_ref() else {
                return None;
            };
            if items.iter().any(|e| {
                !matches!(e, Expr::Number(_) | Expr::Integer(_))
                    && !matches!(e, Expr::LocalGet(id) if *id == counter)
            }) {
                return None;
            }
            (Some(*id), reduction)
        }
        _ => return None,
    };
    let Stmt::Expr(Expr::LocalSet(accumulator, value)) = reduction else {
        return None;
    };
    if !matches!(
        value.as_ref(),
        Expr::Binary {
            op: BinaryOp::Add,
            ..
        }
    ) {
        return None;
    }
    let mut fact = Fact {
        receiver: 0,
        counter,
        invariant_values: vec![],
        keys: vec![],
        accumulator: *accumulator,
        numeric_locals: vec![],
        allocation_local,
    };
    if !collect(value, &mut fact) || fact.keys.is_empty() {
        return None;
    }
    match bound {
        Expr::LocalGet(id) if *id != *accumulator && *id != counter => {
            if !fact.numeric_locals.contains(id) {
                fact.numeric_locals.push(*id);
            }
        }
        Expr::Number(_) | Expr::Integer(_) => {}
        _ => return None,
    }
    if fact.receiver == counter
        || fact.receiver == *accumulator
        || fact.numeric_locals.contains(&fact.receiver)
        || *accumulator == counter
    {
        return None;
    }
    if allocation_local.is_some_and(|id| {
        id == fact.receiver
            || id == counter
            || id == fact.accumulator
            || fact.numeric_locals.contains(&id)
    }) {
        return None;
    }
    Some(fact)
}

fn addressable(ctx: &FnCtx<'_>, id: u32) -> bool {
    !ctx.boxed_vars.contains(&id)
        && !ctx.closure_captures.contains_key(&id)
        // Scalar replacement leaves a bookkeeping local but no receiver.
        // Its existing field lowering must keep reading the field allocas;
        // even the generic clone cannot read a nonexistent object root.
        && !ctx.scalar_replaced.contains_key(&id)
        && !ctx.scalar_replaced_arrays.contains_key(&id)
        && !ctx.scalar_replaced_uppercase_sources.contains_key(&id)
        // Module variables are direct registered roots too. The closed body
        // writes only the accumulator and optional fresh-array binding, and
        // cannot dispatch JS; neither receiver nor numeric inputs can change.
        && (ctx.locals.contains_key(&id) || ctx.local_slot_reps.contains_key(&id)
            || ctx.module_globals.contains_key(&id))
}

fn load_slots(ctx: &mut FnCtx<'_>, fact: &Fact) -> Result<Vec<String>> {
    // Deliberately read LocalGet HERE. A preheader raw address cannot survive
    // the back-edge's moving poll, even though its shape proof survives it.
    let recv = lower_expr(ctx, &Expr::LocalGet(fact.receiver))?;
    let bits = ctx.block().bitcast_double_to_i64(&recv);
    let raw = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let header = crate::target_layout::object_header_size_bytes(ctx.target_triple);
    let mut values = Vec::new();
    for slot in 0..fact.keys.len() {
        let offset = (header + slot as u64 * 8).to_string();
        let addr = ctx.block().add(I64, &raw, &offset);
        let ptr = ctx.block().inttoptr(I64, &addr);
        values.push(ctx.block().load(DOUBLE, &ptr));
    }
    Ok(values)
}

fn fold(
    ctx: &mut FnCtx<'_>,
    expr: &Expr,
    fact: &Fact,
    values: &mut Option<Vec<String>>,
) -> Result<String> {
    match expr {
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => {
            let l = fold(ctx, left, fact, values)?;
            let r = fold(ctx, right, fact, values)?;
            Ok(ctx.block().fadd(&l, &r))
        }
        Expr::PropertyGet { property, .. } => {
            // Preserve operand evaluation order. Nothing in this closed
            // expression can collect between the accumulator and slot loads.
            if values.is_none() {
                *values = Some(load_slots(ctx, fact)?);
            }
            Ok(values.as_ref().unwrap()[fact
                .keys
                .iter()
                .position(|k| k == property)
                .expect("proved key")]
            .clone())
        }
        _ => lower_expr(ctx, expr),
    }
}

/// Lower the proved reduction and invariant numeric reads ahead of
/// representation selection and property/region specializations.
pub(crate) fn lower_reduction(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Some(fact) = ctx.canonical_read_loop.clone() else {
        return Ok(None);
    };
    if let Expr::LocalGet(id) = expr {
        if let Some((_, value)) = fact.invariant_values.iter().find(|(local, _)| local == id) {
            return Ok(Some(value.clone()));
        }
    }
    let Expr::LocalSet(id, value) = expr else {
        return Ok(None);
    };
    if *id != fact.accumulator {
        return Ok(None);
    }
    let result = fold(ctx, value, &fact, &mut None)?;
    crate::expr::bind_lowered_value_to_local(ctx, *id, &result, value)?;
    Ok(Some(result))
}

// Independent 50/50 tag checks compound into an artificially cold fast
// clone, even when the shape comparison itself is balanced. Keep ordinary
// numeric/object admission likely; leave the actual shape hit unweighted so
// LLVM allocates registers for both loop versions. This emits no instruction.
fn likely_plain_input(ctx: &mut FnCtx<'_>, test: &str) -> String {
    ctx.block()
        .call(I1, "llvm.expect.i1", &[(I1, test), (I1, "true")])
}

pub(crate) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    if ctx.canonical_read_loop.is_some()
        || !ctx.pending_labels.is_empty()
        || crate::expr::typed_feedback_emission_enabled()
    {
        return Ok(false);
    }
    // A short loop can pay this preheader on every call without amortizing
    // it. Leave known short trips entirely on the existing lowering, with no
    // runtime profitability branch or extra per-entry instruction.
    if let Some(Expr::Compare { right, .. }) = condition {
        let short = match right.as_ref() {
            Expr::LocalGet(id) => ctx.canonical_read_short_bounds.contains(id),
            Expr::Integer(n) => {
                *n >= 0 && (*n as f64) < super::canonical_read_profit::SHORT_TRIP_LIMIT
            }
            Expr::Number(n) => *n >= 0.0 && *n < super::canonical_read_profit::SHORT_TRIP_LIMIT,
            _ => false,
        };
        if short {
            return Ok(false);
        }
    }
    let Some(mut fact) = plan(init, condition, update, body) else {
        return Ok(false);
    };
    if fact
        .allocation_local
        .is_some_and(|id| !addressable(ctx, id))
        || !addressable(ctx, fact.receiver)
        || !addressable(ctx, fact.accumulator)
        || fact.numeric_locals.iter().any(|id| !addressable(ctx, *id))
    {
        return Ok(false);
    }

    let slow = ctx.new_block("shape_loop.generic");
    let tag = ctx.new_block("shape_loop.tag");
    let handle = ctx.new_block("shape_loop.handle");
    let shape = ctx.new_block("shape_loop.shape");
    let numbers = ctx.new_block("shape_loop.numbers");
    let fast = ctx.new_block("shape_loop.fast");
    let done = ctx.new_block("shape_loop.done");
    let tag_l = ctx.block_label(tag);
    let handle_l = ctx.block_label(handle);
    let shape_l = ctx.block_label(shape);
    let numbers_l = ctx.block_label(numbers);
    let fast_l = ctx.block_label(fast);
    let slow_l = ctx.block_label(slow);
    let done_l = ctx.block_label(done);

    // The module initializer resolves and roots the prediction once. Every
    // entry only loads its expectation; it never allocates strings or walks
    // the weak canonical trie. Volatile observes class-guard poisoning.
    let name = ctx.strings.canonical_read_shape(&fact.keys);
    let expected = ctx.block().load_volatile(I32, &format!("@{name}"));

    // Numeric locals make condition/update/reduction coercion-free. This also
    // protects the proof against valueOf deleting a property mid-iteration.
    let mut numeric = "true".to_string();
    for id in &fact.numeric_locals {
        let v = lower_expr(ctx, &Expr::LocalGet(*id))?;
        let ok = super::loops::emit_js_value_is_number(ctx, &v);
        numeric = ctx.block().and(I1, &numeric, &ok);
        // This scalar is proven numeric on the fast edge. The admitted body
        // cannot assign it or run JS, so it remains valid across moving polls.
        // Keep counter/accumulator loads live: those bindings do change.
        if *id != fact.counter && *id != fact.accumulator {
            fact.invariant_values.push((*id, v));
        }
    }
    let numeric = likely_plain_input(ctx, &numeric);
    ctx.block().cond_br(&numeric, &tag_l, &slow_l);
    ctx.current_block = tag;
    let receiver = lower_expr(ctx, &Expr::LocalGet(fact.receiver))?;
    let bits = ctx.block().bitcast_double_to_i64(&receiver);
    let top = ctx.block().lshr(I64, &bits, "48");
    let is_ptr = ctx.block().icmp_eq(I64, &top, "32765");
    let is_ptr = likely_plain_input(ctx, &is_ptr);
    ctx.block().cond_br(&is_ptr, &handle_l, &slow_l);
    ctx.current_block = handle;
    let raw = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let real = ctx.block().icmp_ugt(I64, &raw, "1048575");
    let real = likely_plain_input(ctx, &real);
    ctx.block().cond_br(&real, &shape_l, &slow_l);
    ctx.current_block = shape;
    let addr = ctx.block().add(I64, &raw, "4");
    let ptr = ctx.block().inttoptr(I64, &addr);
    let sid = ctx.block().load(I32, &ptr);
    let hit = ctx.block().icmp_eq(I32, &sid, &expected);
    ctx.block().cond_br(&hit, &numbers_l, &slow_l);
    ctx.current_block = numbers;
    let mut numeric = "true".to_string();
    for v in load_slots(ctx, &fact)? {
        let ok = super::loops::emit_js_value_is_number(ctx, &v);
        numeric = ctx.block().and(I1, &numeric, &ok);
    }
    let numeric = likely_plain_input(ctx, &numeric);
    ctx.block().cond_br(&numeric, &fast_l, &slow_l);

    ctx.current_block = fast;
    ctx.canonical_read_loop = Some(fact);
    super::loops::lower_for_after_init(ctx, init, condition, update, body, "for.shape_read")?;
    ctx.canonical_read_loop = None;
    ctx.block().br(&done_l);
    ctx.current_block = slow;
    // Re-enter exactly the pre-existing lowering after initialization. No
    // read hooks or IC suppression: this clone retains all baseline choices.
    super::loops::lower_for_baseline_after_init(ctx, init, condition, update, body)?;
    ctx.block().br(&done_l);
    ctx.current_block = done;
    Ok(true)
}
