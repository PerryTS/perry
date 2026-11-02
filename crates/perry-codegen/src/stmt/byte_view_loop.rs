//! A byte loop over a Buffer/Uint8Array parameter, versioned on ONE receiver
//! admission in the preheader.
//!
//! ```ts
//! function incInPlace(buf: Buffer): number {
//!   for (let i = 0; i < buf.length; i++) buf[i] = (buf[i] + 1) & 255;
//! }
//! ```
//!
//! The generic (exported) entry of such a function takes an arbitrary JS
//! value. Each access therefore revalidates the hoisted B4 byte proof and
//! compares `i` with the proof's length, while the loop guard reads
//! `buf.length` through the header: two length sources, so LLVM cannot fold
//! the in-bounds test, and every access keeps its runtime arm.
//!
//! The rule: when the loop body cannot run JS (no call, no allocation, no
//! property access other than `P[i]`, no coercion of a non-Number), nothing
//! inside the loop can detach, resize or rebind `P`. So ONE admission before
//! the loop (the B4 proof is clean for this receiver, no typed array has a
//! named `length`, and every outer local the body uses is a Number) proves
//! the whole loop: the fast copy binds `P`
//! as a native byte view whose data and length are the proof's, read once.
//! `P.length` inside the fast copy reads that same length slot, so the loop
//! guard `i < len` is the bounds proof of every `P[i]` and no access has a
//! slow arm. When the admission fails, the original loop runs, unchanged.
//!
//! The body grammar is closed: literals, the counter, locals declared in the
//! body, outer locals (each one not already a proven Number gets the entry
//! Number test and an F64 slot copied back on exit), `P[i]` reads and
//! `P[i] = v` writes with
//! the key exactly the counter, and arithmetic/bitwise operators over those.
//! Every value in the fast copy is then a Number, so no operator can call
//! `valueOf`/`toString`.

use std::collections::BTreeSet;

use anyhow::Result;
use perry_hir::{BinaryOp, CompareOp, Expr, Stmt, UnaryOp, UpdateOp};

use super::loops::{emit_js_value_is_number, lower_for_after_init};
use crate::expr::{lower_expr, FnCtx};
use crate::native_value::{
    AliasState, BufferElem, BufferIndexUnit, BufferViewPointerState, BufferViewSlot, LengthSource,
};
use crate::types::{DOUBLE, I1, I32, PTR};

/// Expression nodes beyond which the second copy is not worth its size.
const MAX_LOOP_NODES: usize = 256;

/// `PERRY_BYTE_VIEW_LOOP=0` keeps every loop on the ordinary lowering.
fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_BYTE_VIEW_LOOP").as_deref(),
        Ok("0") | Ok("off") | Ok("false")
    )
}

struct Plan {
    receiver: u32,
    /// Locals the body declares: each copy gets its own slot.
    declared: Vec<u32>,
    /// Outer locals the loop reads or writes that are not already proven
    /// Numbers: each gets one entry test and an F64 slot in the fast copy.
    inputs: Vec<u32>,
    /// The subset of `inputs` the loop writes: copied back on exit.
    written: Vec<u32>,
}

struct Walk<'c, 'a> {
    ctx: &'c FnCtx<'a>,
    receiver: u32,
    counter: u32,
    declared: BTreeSet<u32>,
    reads: BTreeSet<u32>,
    writes: BTreeSet<u32>,
    nodes: usize,
}

impl Walk<'_, '_> {
    fn stmt(&mut self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let { id, init, .. } => {
                self.declared.insert(*id);
                init.as_ref().is_some_and(|init| self.expr(init))
            }
            Stmt::Expr(expr) => self.expr(expr),
            _ => false,
        }
    }

    fn element(&self, object: &Expr, index: &Expr) -> bool {
        matches!(object, Expr::LocalGet(id) if *id == self.receiver)
            && matches!(index, Expr::LocalGet(id) if *id == self.counter)
    }

    fn expr(&mut self, expr: &Expr) -> bool {
        self.nodes += 1;
        if self.nodes > MAX_LOOP_NODES {
            return false;
        }
        match expr {
            Expr::Integer(_) => true,
            Expr::Number(_) => true,
            Expr::LocalGet(id) => {
                if *id == self.receiver {
                    return false;
                }
                if *id != self.counter && !self.declared.contains(id) {
                    self.reads.insert(*id);
                }
                true
            }
            Expr::LocalSet(id, value) => {
                if *id == self.receiver || *id == self.counter {
                    return false;
                }
                if !self.declared.contains(id) {
                    self.writes.insert(*id);
                }
                self.expr(value)
            }
            Expr::IndexGet { object, index, .. }
            | Expr::Uint8ArrayGet {
                array: object,
                index,
            }
            | Expr::BufferIndexGet {
                buffer: object,
                index,
            } => self.element(object, index),
            Expr::IndexSet {
                object,
                index,
                value,
                ..
            }
            | Expr::Uint8ArraySet {
                array: object,
                index,
                value,
            }
            | Expr::BufferIndexSet {
                buffer: object,
                index,
                value,
            } => self.element(object, index) && self.expr(value),
            Expr::Binary { op, left, right } => {
                matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Mod
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Shr
                        | BinaryOp::UShr
                ) && self.expr(left)
                    && self.expr(right)
            }
            Expr::Unary { op, operand } => {
                matches!(op, UnaryOp::Neg | UnaryOp::BitNot | UnaryOp::Pos) && self.expr(operand)
            }
            _ => false,
        }
    }
}

/// `for (let i = 0; i < P.length; i++)` with a body this tier can prove.
fn match_plan(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Plan> {
    // With a shape-barrier site in the module, `P.length` always takes full
    // property semantics (a call that may run a getter) and cannot be the
    // fast copy's length source.
    if ctx.is_async_fn
        || ctx.try_depth != 0
        || ctx.disable_buffer_fast_path
        || ctx.module_has_shape_barrier_sites
    {
        return None;
    }
    let Some(Stmt::Let {
        id: counter,
        init: Some(start),
        ..
    }) = init
    else {
        return None;
    };
    if !matches!(start, Expr::Integer(0)) && !matches!(start, Expr::Number(n) if *n == 0.0) {
        return None;
    }
    let Some(Expr::Compare {
        op: CompareOp::Lt,
        left,
        right,
    }) = condition
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expr::LocalGet(id) if id == counter) {
        return None;
    }
    let Expr::PropertyGet {
        object, property, ..
    } = right.as_ref()
    else {
        return None;
    };
    let Expr::LocalGet(receiver) = object.as_ref() else {
        return None;
    };
    if property != "length" {
        return None;
    }
    if !matches!(update, Some(Expr::Update { id, op: UpdateOp::Increment, .. }) if id == counter) {
        return None;
    }
    let receiver = *receiver;
    // Only a receiver whose B4 proof the function installed: a fixed,
    // unboxed parameter. A receiver that is already a proven native view
    // (a spec `TaPtr` param, a sealed construction) needs no version.
    let fixed = ctx
        .receiver_descriptors
        .byte_view_access(receiver, &crate::expr::u8_buffer_read::U8_BRANDS)
        .is_some_and(|access| access.fixed_receiver);
    if !fixed
        || ctx.receiver_descriptors.contains_buffer_view(receiver)
        || ctx.buffer_data_slots.contains_key(&receiver)
        || ctx.boxed_vars.contains(&receiver)
        || ctx.reassigned_locals.contains(&receiver)
        || ctx.closure_captures.contains_key(&receiver)
        || ctx.module_globals.contains_key(&receiver)
    {
        return None;
    }
    let mut walk = Walk {
        ctx,
        receiver,
        counter: *counter,
        declared: BTreeSet::new(),
        reads: BTreeSet::new(),
        writes: BTreeSet::new(),
        nodes: 0,
    };
    if !body.iter().all(|stmt| walk.stmt(stmt)) {
        return None;
    }
    // A proven Number may live in a representation slot (a canonical i32)
    // instead of `locals`; its ordinary lowering is kept as is.
    let plain = |id: &u32| {
        (walk.ctx.locals.contains_key(id) || crate::type_analysis::local_is_number(walk.ctx, *id))
            && !walk.ctx.boxed_vars.contains(id)
            && !walk.ctx.closure_captures.contains_key(id)
            && !walk.ctx.repsel_closure_ref_locals.contains(id)
            && !walk.ctx.module_globals.contains_key(id)
    };
    // Every outer local the body reads or writes is either a proven Number
    // already or gets one entry test; inside the fast copy all of them hold
    // Numbers (every write there produces one).
    let mut inputs = Vec::new();
    let mut written = Vec::new();
    for id in walk.reads.union(&walk.writes) {
        if !plain(id) {
            return None;
        }
        if crate::type_analysis::local_is_number(walk.ctx, *id) {
            continue;
        }
        if walk.ctx.i32_counter_slots.contains_key(id)
            || walk.ctx.local_slot_reps.contains_key(id)
            || walk.ctx.numeric_accumulator_f64_slots.contains_key(id)
        {
            return None;
        }
        if walk.writes.contains(id) {
            written.push(*id);
        }
        inputs.push(*id);
    }
    Some(Plan {
        receiver,
        declared: walk.declared.into_iter().collect(),
        inputs,
        written,
    })
}

/// Try the tier. `init` has already been lowered by the caller.
pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    if !enabled() {
        return Ok(false);
    }
    let Some(plan) = match_plan(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    let object = Expr::LocalGet(plan.receiver);
    let boxed = lower_expr(ctx, &object)?;
    let Some(access) = crate::expr::u8_buffer_read::byte_view_param_for(
        ctx,
        &object,
        &boxed,
        &crate::expr::u8_buffer_read::U8_BRANDS,
    ) else {
        return Ok(false);
    };
    let mut admitted = access.valid_i1.clone();
    // `P.length` reads the native length only while no typed array has a
    // named `length` property; nothing in the fast copy can define one, so
    // testing the flag once here keeps that arm dead for the whole loop.
    let invalidated = ctx.block().load_atomic_acquire(
        crate::types::I8,
        "@PERRY_TYPED_NAMED_PROPS_INVALIDATED",
        1,
    );
    let pristine = ctx.block().icmp_eq(crate::types::I8, &invalidated, "0");
    admitted = ctx.block().and(I1, &admitted, &pristine);
    let mut input_values = Vec::with_capacity(plan.inputs.len());
    for id in &plan.inputs {
        let value = lower_expr(ctx, &Expr::LocalGet(*id))?;
        let is_number = emit_js_value_is_number(ctx, &value);
        admitted = ctx.block().and(I1, &admitted, &is_number);
        input_values.push((*id, value));
    }
    // The proof's data and length, read once. Nothing in the fast copy can
    // run JS, so neither can change while it runs.
    let data_i64 = access.data_i64.clone();
    let length = ctx.block().load(I32, &access.length_slot);

    let fast_idx = ctx.new_block("for.byte_view.fast.preheader");
    let slow_idx = ctx.new_block("for.byte_view.slow.preheader");
    let merge_idx = ctx.new_block("for.byte_view.merge");
    let fast_label = ctx.block_label(fast_idx);
    let slow_label = ctx.block_label(slow_idx);
    let merge_label = ctx.block_label(merge_idx);
    ctx.block().cond_br(&admitted, &fast_label, &slow_label);

    let labels = ctx.pending_labels.clone();
    ctx.current_block = fast_idx;
    let data_ptr = ctx.block().inttoptr(crate::types::I64, &data_i64);
    let data_slot = ctx.func.alloca_entry(PTR);
    ctx.block().store(PTR, &data_ptr, &data_slot);
    let length_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &length, &length_slot);
    ctx.block().emit_raw(format!(
        "; bytes.view_loop.admitted data={} length={}",
        data_slot.trim_start_matches('%'),
        length_slot.trim_start_matches('%')
    ));
    // The scope index only keys `P.length`'s native read; the view claims no
    // noalias (a generic receiver may be any buffer), so no alias metadata
    // is emitted for it.
    let scope_idx = ctx.buffer_alias_base + ctx.buffer_data_slots.len() as u32;
    ctx.buffer_data_slots
        .insert(plan.receiver, (data_slot.clone(), scope_idx));
    ctx.receiver_descriptors.materialize_buffer_view(
        plan.receiver,
        BufferViewSlot {
            data_slot,
            length_slot: Some(length_slot),
            scope_idx: None,
            elem: BufferElem::U8,
            element_width_bytes: 1,
            index_unit: BufferIndexUnit::Element,
            view_byte_offset: Some(0),
            length_offset_from_data: 0,
            alias: AliasState::Unknown,
            length_source: Some(LengthSource::Unknown),
            native_owned: None,
            pointer_state: BufferViewPointerState::Stable,
            storage_inline_proven: false,
            // No call, allocation or coercion runs in the fast copy.
            length_fixed: true,
        },
    );
    let previous_type = ctx.snapshot_guarded_proof(&plan.receiver);
    ctx.proven_local_types.insert(
        plan.receiver,
        perry_hir::types::Type::Union(vec![
            perry_hir::types::Type::Named("Uint8Array".into()),
            perry_hir::types::Type::Named("Buffer".into()),
        ]),
    );
    let scope_id = ctx.next_loop_proof_scope_id();
    ctx.receiver_descriptors
        .materialize_number_locals(scope_id, &plan.inputs);
    ctx.receiver_descriptors
        .materialize_raw_callback_locals(scope_id, plan.inputs.clone());
    let mut redirected = Vec::with_capacity(input_values.len());
    for (id, value) in &input_values {
        let alloca = ctx.func.alloca_entry(DOUBLE);
        ctx.block().store(DOUBLE, value, &alloca);
        ctx.numeric_accumulator_f64_slots.insert(*id, alloca);
        redirected.push(*id);
    }
    // A body local's fast-copy slot holds only Numbers; the original copy
    // (any receiver) allocates its own slot with its own root protocol.
    let declared: Vec<(u32, Option<u32>, Option<perry_hir::types::Type>)> = plan
        .declared
        .iter()
        .filter(|id| !ctx.locals.contains_key(id))
        .map(|id| {
            (
                *id,
                ctx.shadow_slot_map.get(id).copied(),
                ctx.snapshot_guarded_proof(id),
            )
        })
        .collect();
    let fast = lower_for_after_init(ctx, init, condition, update, body, "for.byte_view_fast");
    if fast.is_ok() && !ctx.block().is_terminated() {
        // The body has no `break`/`return`/`throw`: the loop's exit is the
        // only way out. A Number carries no heap edge, so no barrier.
        for id in &plan.written {
            if let (Some(alloca), Some(real)) = (
                ctx.numeric_accumulator_f64_slots.get(id).cloned(),
                ctx.locals.get(id).cloned(),
            ) {
                let value = ctx.block().load(DOUBLE, &alloca);
                ctx.block().store(DOUBLE, &value, &real);
            }
        }
    }
    for (id, shadow, proof) in declared {
        ctx.locals.remove(&id);
        match shadow {
            Some(slot) => {
                ctx.shadow_slot_map.insert(id, slot);
            }
            None => {
                ctx.shadow_slot_map.remove(&id);
            }
        }
        match proof {
            Some(ty) => {
                ctx.proven_local_types.insert(id, ty);
            }
            None => {
                ctx.proven_local_types.remove(&id);
            }
        }
    }
    for id in redirected {
        ctx.numeric_accumulator_f64_slots.remove(&id);
    }
    ctx.receiver_descriptors.dematerialize_scope(scope_id);
    ctx.receiver_descriptors
        .dematerialize_buffer_view(plan.receiver);
    ctx.buffer_data_slots.remove(&plan.receiver);
    match previous_type {
        Some(ty) => {
            ctx.proven_local_types.insert(plan.receiver, ty);
        }
        None => {
            ctx.proven_local_types.remove(&plan.receiver);
        }
    }
    fast?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    // The original copy runs only for a receiver the admission refused (not
    // a plain writable byte view) or for a non-Number input, where any
    // operator may run JS. It is the generic element access, not a byte cell
    // whose hoisted proof was just refused.
    ctx.current_block = slow_idx;
    ctx.pending_labels = labels;
    let saved_fast_path = std::mem::replace(&mut ctx.disable_buffer_fast_path, true);
    let slow = lower_for_after_init(ctx, init, condition, update, body, "for.byte_view_slow");
    ctx.disable_buffer_fast_path = saved_fast_path;
    slow?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
    Ok(true)
}

#[cfg(test)]
#[path = "byte_view_loop_tests.rs"]
mod tests;
