//! One canonical shape guard for a counted, non-dispatching read loop.
//!
//! The prediction is the ordered list of keys in the reduction. It is resolved
//! through the runtime's canonical-key tree, not learned from a receiver or IC.
//! Exact identity proves ordinary, generation-zero, hole-free, inline slots.
//! The closed body grammar below and numeric prechecks exclude user callbacks.
//! Polls remain enabled; every read re-derives the address from the receiver root.

use crate::expr::{lower_expr, FnCtx};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I1, I32, I64, PTR};
use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UpdateOp};

#[derive(Clone, Debug)]
pub(crate) struct Fact {
    receiver: u32,
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

fn fold(ctx: &mut FnCtx<'_>, expr: &Expr, fact: &Fact, values: &[String]) -> Result<String> {
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
        Expr::PropertyGet { property, .. } => Ok(values[fact
            .keys
            .iter()
            .position(|k| k == property)
            .expect("proved key")]
        .clone()),
        _ => lower_expr(ctx, expr),
    }
}

/// Intercept only the reduction proved by `plan`, ahead of representation
/// selection and all property/region specializations.
pub(crate) fn lower_reduction(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    let Some(fact) = ctx.canonical_read_loop.clone() else {
        return Ok(None);
    };
    let Expr::LocalSet(id, value) = expr else {
        return Ok(None);
    };
    if *id != fact.accumulator {
        return Ok(None);
    }
    let values = load_slots(ctx, &fact)?;
    let result = fold(ctx, value, &fact, &values)?;
    crate::expr::bind_lowered_value_to_local(ctx, *id, &result, value)?;
    Ok(Some(result))
}

/// The one slow property path. No cache global, priming, or second inline arm.
pub(crate) fn lower_generic_read(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<Option<String>> {
    if !ctx.canonical_read_generic {
        return Ok(None);
    }
    let Expr::PropertyGet {
        object, property, ..
    } = expr
    else {
        return Ok(None);
    };
    let receiver = lower_expr(ctx, object)?;
    let key = ctx.strings.intern(property);
    let global = format!("@{}", ctx.strings.entry(key).handle_global);
    let key_box = ctx.block().load(DOUBLE, &global);
    let key_bits = ctx.block().bitcast_double_to_i64(&key_box);
    let key_raw = ctx.block().and(I64, &key_bits, POINTER_MASK_I64);
    let bits = ctx.block().bitcast_double_to_i64(&receiver);
    Ok(Some(ctx.block().call(
        DOUBLE,
        "js_object_get_field_generic",
        &[(I64, &bits), (I64, &key_raw)],
    )))
}

pub(crate) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    if ctx.canonical_read_loop.is_some()
        || ctx.canonical_read_generic
        || !ctx.pending_labels.is_empty()
        || crate::expr::typed_feedback_emission_enabled()
    {
        return Ok(false);
    }
    let Some(fact) = plan(init, condition, update, body) else {
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

    // Constant key bytes describe a prediction, never a per-site copy of the
    // receiver's shape/slots. The runtime resolves the canonical shape itself.
    let bytes: Vec<u8> = fact
        .keys
        .iter()
        .flat_map(|k| k.bytes().chain(std::iter::once(0)))
        .collect();
    let site = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let name = format!(
        "@{}_shape_keys",
        crate::expr::inline_cache_global_name(ctx, site)
    );
    let encoded: String = bytes.iter().map(|b| format!("\\{b:02X}")).collect();
    ctx.typed_parse_rodata.push(format!(
        "{name} = private constant [{} x i8] c\"{encoded}\", align 1",
        bytes.len()
    ));
    let expected = ctx.block().call(
        I32,
        "js_canonical_read_shape",
        &[(PTR, &name), (I32, &bytes.len().to_string())],
    );

    // Numeric locals make condition/update/reduction coercion-free. This also
    // protects the proof against valueOf deleting a property mid-iteration.
    let mut numeric = "true".to_string();
    for id in &fact.numeric_locals {
        let v = lower_expr(ctx, &Expr::LocalGet(*id))?;
        let ok = super::loops::emit_js_value_is_number(ctx, &v);
        numeric = ctx.block().and(I1, &numeric, &ok);
    }
    ctx.block().cond_br(&numeric, &tag_l, &slow_l);
    ctx.current_block = tag;
    let receiver = lower_expr(ctx, &Expr::LocalGet(fact.receiver))?;
    let bits = ctx.block().bitcast_double_to_i64(&receiver);
    let top = ctx.block().lshr(I64, &bits, "48");
    let is_ptr = ctx.block().icmp_eq(I64, &top, "32765");
    ctx.block().cond_br(&is_ptr, &handle_l, &slow_l);
    ctx.current_block = handle;
    let raw = ctx.block().and(I64, &bits, POINTER_MASK_I64);
    let real = ctx.block().icmp_ugt(I64, &raw, "1048575");
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
    ctx.block().cond_br(&numeric, &fast_l, &slow_l);

    ctx.current_block = fast;
    ctx.canonical_read_loop = Some(fact);
    super::loops::lower_for_after_init(ctx, init, condition, update, body, "for.shape_read")?;
    ctx.canonical_read_loop = None;
    ctx.block().br(&done_l);
    ctx.current_block = slow;
    ctx.canonical_read_generic = true;
    super::loops::lower_for_after_init(ctx, init, condition, update, body, "for.shape_generic")?;
    ctx.canonical_read_generic = false;
    ctx.block().br(&done_l);
    ctx.current_block = done;
    Ok(true)
}
