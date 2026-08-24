//! Guarded loop versions for counted Array and Array-subclass iteration.
//!
//! A one-time runtime admission publishes scalar layout facts. The fast copy
//! is entered only after its emitted blocks are proven call-free, so its
//! preheader-cached receiver and storage bases stay valid for the whole copy.
//! Failed admission runs the unchanged generic loop from the current counter.

use anyhow::Result;
use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};

use crate::expr::{FnCtx, StablePackedLoopFact, StablePackedNumericAccess};
use crate::native_value::{BoundsState, BufferAccessMode, LoweredValue, MaterializationReason};
use crate::types::{DOUBLE, I1, I32, I64, PTR};

#[derive(Clone, Copy)]
enum LoopBound {
    Snapshot(u32),
    LiveLength,
}

struct Candidate {
    counter_id: u32,
    array_id: u32,
    bound: LoopBound,
    numeric_elements: bool,
}

fn target_below_numeric_operator(
    expr: &Expr,
    array_id: u32,
    counter_id: u32,
    numeric_context: bool,
) -> bool {
    if matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    ) {
        return numeric_context;
    }
    if matches!(expr, Expr::Closure { .. }) {
        return false;
    }
    let child_numeric_context =
        numeric_context || matches!(expr, Expr::Binary { .. } | Expr::NumberCoerce(_));
    let mut found = false;
    perry_hir::walker::walk_expr_children(expr, &mut |child| {
        if !found
            && target_below_numeric_operator(child, array_id, counter_id, child_numeric_context)
        {
            found = true;
        }
    });
    found
}

fn leading_read_requires_numeric(body: &[Stmt], array_id: u32, counter_id: u32) -> bool {
    let Some(first) = body.first() else {
        return false;
    };
    let expr = match first {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => expr,
        _ => return false,
    };
    target_below_numeric_operator(expr, array_id, counter_id, false)
}

fn expr_flags(expr: &Expr, array_id: u32, counter_id: u32, target: &mut bool, call: &mut bool) {
    if matches!(
        expr,
        Expr::IndexGet { object, index }
            if matches!(object.as_ref(), Expr::LocalGet(id) if *id == array_id)
                && matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id)
    ) {
        *target = true;
    }
    if matches!(expr, Expr::Call { .. } | Expr::New { .. }) {
        *call = true;
    }
    if !matches!(expr, Expr::Closure { .. }) {
        perry_hir::walker::walk_expr_children(expr, &mut |child| {
            expr_flags(child, array_id, counter_id, target, call);
        });
    }
}

fn stmt_flags(stmt: &Stmt, array_id: u32, counter_id: u32) -> (bool, bool) {
    let mut target = false;
    let mut call = false;
    match stmt {
        Stmt::Let {
            init: Some(expr), ..
        }
        | Stmt::Expr(expr)
        | Stmt::Throw(expr)
        | Stmt::Return(Some(expr)) => {
            expr_flags(expr, array_id, counter_id, &mut target, &mut call);
        }
        _ => {}
    }
    (target, call)
}

/// The direct read must be in the first straight-line statement and before any
/// explicit user call. Later statements may allocate or invoke callbacks: the
/// next iteration reloads the root and validates before using it again.
fn body_has_safe_leading_read(body: &[Stmt], array_id: u32, counter_id: u32) -> bool {
    let Some(first) = body.first() else {
        return false;
    };
    let (first_target, first_call) = stmt_flags(first, array_id, counter_id);
    if !first_target || first_call {
        return false;
    }
    !body[1..]
        .iter()
        .any(|stmt| stmt_flags(stmt, array_id, counter_id).0)
}

fn stmt_contains_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break | Stmt::LabeledBreak(_) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.iter().any(stmt_contains_break)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| branch.iter().any(stmt_contains_break))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            body.iter().any(stmt_contains_break)
        }
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_break) || body.iter().any(stmt_contains_break)
        }
        Stmt::Labeled { body, .. } => stmt_contains_break(body),
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter().any(stmt_contains_break)
                || catch
                    .as_ref()
                    .is_some_and(|clause| clause.body.iter().any(stmt_contains_break))
                || finally
                    .as_ref()
                    .is_some_and(|body| body.iter().any(stmt_contains_break))
        }
        Stmt::Switch { cases, .. } => cases
            .iter()
            .any(|case| case.body.iter().any(stmt_contains_break)),
        _ => false,
    }
}

fn match_candidate(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Option<Candidate> {
    if !ctx.pending_labels.is_empty() {
        return None;
    }
    let counter_id = match init? {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(0)),
            ..
        } => *id,
        _ => return None,
    };
    if !matches!(
        update,
        Some(Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        }) if *id == counter_id
    ) {
        return None;
    }
    let right = match condition? {
        Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) => right.as_ref(),
        _ => return None,
    };
    let (array_id, bound) = match right {
        Expr::LocalGet(bound_id) => {
            let array_id = *ctx.array_length_snapshots.get(bound_id)?;
            if ctx.reassigned_locals.contains(bound_id) {
                return None;
            }
            (array_id, LoopBound::Snapshot(*bound_id))
        }
        Expr::PropertyGet {
            object, property, ..
        } if property == "length" => match object.as_ref() {
            Expr::LocalGet(array_id) => (*array_id, LoopBound::LiveLength),
            _ => return None,
        },
        _ => return None,
    };
    if ctx.reassigned_locals.contains(&array_id)
        || ctx.closure_captures.contains_key(&array_id)
        || (ctx.locals.contains_key(&array_id) && ctx.boxed_vars.contains(&array_id))
        || (!ctx.locals.contains_key(&array_id) && !ctx.module_globals.contains_key(&array_id))
        || super::loops::stmts_mutate_local(body, counter_id)
        // A fast-loop `break` reaches that clone's exit block. Live-length
        // versions use the same block to enter the generic continuation, so
        // replaying the current iteration would duplicate preceding effects.
        || body.iter().any(stmt_contains_break)
        || !body_has_safe_leading_read(body, array_id, counter_id)
    {
        return None;
    }
    Some(Candidate {
        counter_id,
        array_id,
        bound,
        numeric_elements: leading_read_requires_numeric(body, array_id, counter_id),
    })
}

fn descriptor_word(ctx: &mut FnCtx<'_>, descriptor: &str, index: u64) -> String {
    let ptr = ctx
        .block()
        .gep(I64, descriptor, &[(I64, &index.to_string())]);
    ctx.block().load(I64, &ptr)
}

fn record_artifacts(ctx: &mut FnCtx<'_>, array_id: u32, receiver: &str) {
    let lowered = LoweredValue::js_value(receiver.to_string());
    ctx.record_lowered_value_with_access_mode_and_facts(
        "StablePackedArraylikeLoop",
        Some(array_id),
        "stable_packed_arraylike_preheader",
        &lowered,
        Some(BoundsState::Guarded {
            guard_id: "packed_arraylike_loop_guard".to_string(),
        }),
        None,
        Some(BufferAccessMode::CheckedNative),
        None,
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        vec![
            "loop_versioning=stable_packed_arraylike".to_string(),
            "proof=preheader_scalar_layout".to_string(),
            "revalidation=none_call_free_clone".to_string(),
            "side_exit=current_index".to_string(),
        ],
    );
    ctx.record_lowered_value_with_access_mode_and_facts(
        "StablePackedArraylikeLoop",
        Some(array_id),
        "stable_packed_arraylike_generic_side_exit",
        &lowered,
        Some(BoundsState::Unknown),
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::RuntimeApi),
        None,
        None,
        Vec::new(),
        Vec::new(),
        false,
        false,
        vec![
            "loop_versioning=stable_packed_arraylike_fallback".to_string(),
            "resume=current_index".to_string(),
        ],
    );
}

pub(crate) fn try_lower_index_get(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    index: &Expr,
) -> Option<String> {
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object, index) else {
        return None;
    };
    let fact = ctx
        .stable_packed_loop_facts
        .iter()
        .rev()
        .find(|fact| fact.array_local_id == *array_id && fact.counter_local_id == *counter_id)?
        .clone();
    let raw = fact.live_receiver_handle?;
    let counter_slot = ctx.i32_counter_slots.get(counter_id)?.clone();
    let idx_i32 = ctx.block().load(I32, &counter_slot);
    let idx_i64 = ctx.block().zext(I32, &idx_i32, I64);
    if let Some(access) = fact.numeric_access {
        let byte_offset = ctx.block().shl(I64, &idx_i64, "3");
        let plain_addr = ctx.block().add(I64, &access.plain_base, &byte_offset);
        let inline_addr = ctx
            .block()
            .add(I64, &access.object_inline_base, &byte_offset);
        let spill_addr = ctx
            .block()
            .add(I64, &access.object_spill_base, &byte_offset);
        let is_inline = ctx
            .block()
            .icmp_ult(I64, &idx_i64, &access.object_inline_count);
        let object_addr = ctx
            .block()
            .select(I1, &is_inline, I64, &inline_addr, &spill_addr);
        let element_addr = ctx
            .block()
            .select(I1, &access.is_plain, I64, &plain_addr, &object_addr);
        let element_ptr = ctx.block().inttoptr(I64, &element_addr);
        return Some(ctx.block().load(DOUBLE, &element_ptr));
    }
    let kind = descriptor_word(ctx, &fact.descriptor, 0);

    let plain_idx = ctx.new_block("stable_packed.load.plain");
    let object_idx = ctx.new_block("stable_packed.load.object");
    let object_inline_idx = ctx.new_block("stable_packed.load.object.inline");
    let object_spill_idx = ctx.new_block("stable_packed.load.object.spill");
    let object_spill_ptr_idx = ctx.new_block("stable_packed.load.object.spill_ptr");
    let merge_idx = ctx.new_block("stable_packed.load.merge");
    let plain_label = ctx.block_label(plain_idx);
    let object_label = ctx.block_label(object_idx);
    let object_inline_label = ctx.block_label(object_inline_idx);
    let object_spill_label = ctx.block_label(object_spill_idx);
    let object_spill_ptr_label = ctx.block_label(object_spill_ptr_idx);
    let merge_label = ctx.block_label(merge_idx);
    let is_plain = ctx.block().icmp_eq(I64, &kind, "1");
    ctx.block().cond_br(&is_plain, &plain_label, &object_label);

    ctx.current_block = plain_idx;
    let byte_offset = ctx.block().shl(I64, &idx_i64, "3");
    let with_header = ctx.block().add(I64, &byte_offset, "8");
    let element_addr = ctx.block().add(I64, &raw, &with_header);
    let element_ptr = ctx.block().inttoptr(I64, &element_addr);
    let plain_raw = ctx.block().load(DOUBLE, &element_ptr);
    let plain_bits = ctx.block().bitcast_double_to_i64(&plain_raw);
    let is_hole = ctx
        .block()
        .icmp_eq(I64, &plain_bits, crate::nanbox::TAG_HOLE_I64);
    let undefined = ctx
        .block()
        .bitcast_i64_to_double(crate::nanbox::TAG_UNDEFINED_I64);
    let plain_value = ctx
        .block()
        .select(I1, &is_hole, DOUBLE, &undefined, &plain_raw);
    let plain_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = object_idx;
    let element_base = descriptor_word(ctx, &fact.descriptor, 4);
    let packed_bounds = descriptor_word(ctx, &fact.descriptor, 5);
    let inline_bound = ctx.block().lshr(I64, &packed_bounds, "32");
    let slot = ctx.block().add(I64, &element_base, &idx_i64);
    let inline = ctx.block().icmp_ult(I64, &slot, &inline_bound);
    ctx.block()
        .cond_br(&inline, &object_inline_label, &object_spill_label);

    ctx.current_block = object_inline_idx;
    let object_header_size =
        crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
    let slot_bytes = ctx.block().shl(I64, &slot, "3");
    let slot_offset = ctx.block().add(I64, &slot_bytes, &object_header_size);
    let slot_addr = ctx.block().add(I64, &raw, &slot_offset);
    let slot_ptr = ctx.block().inttoptr(I64, &slot_addr);
    let inline_value = ctx.block().load(DOUBLE, &slot_ptr);
    let inline_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = object_spill_idx;
    let pointer_size = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        4
    } else {
        8
    };
    let meta_offset = (crate::target_layout::object_header_size_bytes(ctx.target_triple)
        - pointer_size)
        .to_string();
    let meta_addr = ctx.block().add(I64, &raw, &meta_offset);
    let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
    let meta_native = ctx
        .block()
        .load(if pointer_size == 4 { I32 } else { I64 }, &meta_slot);
    let meta = if pointer_size == 4 {
        ctx.block().zext(I32, &meta_native, I64)
    } else {
        meta_native
    };
    let has_meta = ctx.block().icmp_ne(I64, &meta, "0");
    ctx.block()
        .cond_br(&has_meta, &object_spill_ptr_label, &fact.side_exit_label);

    ctx.current_block = object_spill_ptr_idx;
    let meta_ptr = ctx.block().inttoptr(I64, &meta);
    let spill_slot = ctx.block().gep(I64, &meta_ptr, &[(I64, "4")]);
    let spill = ctx.block().load(I64, &spill_slot);
    let has_spill = ctx.block().icmp_ne(I64, &spill, "0");
    let spill_deref_idx = ctx.new_block("stable_packed.load.object.spill_deref");
    let spill_deref_label = ctx.block_label(spill_deref_idx);
    ctx.block()
        .cond_br(&has_spill, &spill_deref_label, &fact.side_exit_label);

    ctx.current_block = spill_deref_idx;
    let spill_ptr = ctx.block().inttoptr(I64, &spill);
    let spill_len = ctx.block().load(I32, &spill_ptr);
    let spill_len64 = ctx.block().zext(I32, &spill_len, I64);
    let in_bounds = ctx.block().icmp_ult(I64, &slot, &spill_len64);
    let spill_load_idx = ctx.new_block("stable_packed.load.object.spill_load");
    let spill_load_label = ctx.block_label(spill_load_idx);
    ctx.block()
        .cond_br(&in_bounds, &spill_load_label, &fact.side_exit_label);

    ctx.current_block = spill_load_idx;
    let spill_word = ctx.block().add(I64, &slot, "1");
    let spill_element = ctx
        .block()
        .gep_inbounds(I64, &spill_ptr, &[(I64, &spill_word)]);
    let spill_value = ctx.block().load(DOUBLE, &spill_element);
    let spill_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    Some(ctx.block().phi(
        DOUBLE,
        &[
            (&plain_value, &plain_end),
            (&inline_value, &inline_end),
            (&spill_value, &spill_end),
        ],
    ))
}

pub(crate) fn has_numeric_index_fact(ctx: &FnCtx<'_>, expr: &Expr) -> bool {
    let Expr::IndexGet { object, index } = expr else {
        return false;
    };
    let (Expr::LocalGet(array_id), Expr::LocalGet(counter_id)) = (object.as_ref(), index.as_ref())
    else {
        return false;
    };
    ctx.stable_packed_loop_facts.iter().rev().any(|fact| {
        fact.numeric_elements
            && fact.array_local_id == *array_id
            && fact.counter_local_id == *counter_id
    })
}

pub(super) fn lower(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&Expr>,
    update: Option<&Expr>,
    body: &[Stmt],
) -> Result<bool> {
    let Some(candidate) = match_candidate(ctx, init, condition, update, body) else {
        return Ok(false);
    };
    let inserted_counter = if ctx.i32_counter_slots.contains_key(&candidate.counter_id) {
        false
    } else {
        let Some(counter_slot) = ctx.locals.get(&candidate.counter_id).cloned() else {
            return Ok(false);
        };
        let slot = ctx.func.alloca_entry(I32);
        let value = ctx.block().load(DOUBLE, &counter_slot);
        let i32_value = ctx.block().fptosi(DOUBLE, &value, I32);
        ctx.block().store(I32, &i32_value, &slot);
        ctx.i32_counter_slots.insert(candidate.counter_id, slot);
        true
    };

    let receiver = crate::expr::lower_expr(ctx, &Expr::LocalGet(candidate.array_id))?;
    let bound_box = match candidate.bound {
        LoopBound::Snapshot(bound_id) => crate::expr::lower_expr(ctx, &Expr::LocalGet(bound_id))?,
        LoopBound::LiveLength => "-1.0".to_string(),
    };
    let descriptor = ctx.func.alloca_entry_array(I64, 7);
    let guard = ctx.block().call(
        I32,
        "js_packed_arraylike_loop_guard",
        &[
            (DOUBLE, &receiver),
            (DOUBLE, &bound_box),
            (I32, if candidate.numeric_elements { "1" } else { "0" }),
            (PTR, &descriptor),
        ],
    );
    let admitted = ctx.block().icmp_ne(I32, &guard, "0");
    // Deliberately left unterminated until the emitted fast clone has been
    // scanned. The cached receiver below is safe only when no runtime call can
    // allocate, collect, or revoke an admitted layout while that clone runs.
    let admission_idx = ctx.current_block;

    let fast_pre_idx = ctx.new_block("stable_packed.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("stable_packed.loop.slow.preheader");
    let merge_idx = ctx.new_block("stable_packed.loop.merge");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);

    let bound64 = {
        ctx.current_block = fast_pre_idx;
        descriptor_word(ctx, &descriptor, 6)
    };
    let bound_i32 = ctx.block().trunc(I64, &bound64, I32);
    // Reload after the runtime admission call. Once the clone scan succeeds,
    // this root cannot move until the clone returns because the clone contains
    // no GC-unsafe call or allocation point.
    let fast_receiver = crate::expr::lower_expr(ctx, &Expr::LocalGet(candidate.array_id))?;
    let fast_bits = ctx.block().bitcast_double_to_i64(&fast_receiver);
    let fast_raw = ctx
        .block()
        .and(I64, &fast_bits, crate::nanbox::POINTER_MASK_I64);
    let fast_scan_start = ctx.func.num_blocks();
    let numeric_access = if candidate.numeric_elements {
        let kind = descriptor_word(ctx, &descriptor, 0);
        let is_plain = ctx.block().icmp_eq(I64, &kind, "1");
        let plain_base = ctx.block().add(I64, &fast_raw, "8");

        let element_base = descriptor_word(ctx, &descriptor, 4);
        let packed_bounds = descriptor_word(ctx, &descriptor, 5);
        let inline_bound = ctx.block().lshr(I64, &packed_bounds, "32");
        let has_inline = ctx.block().icmp_ult(I64, &element_base, &inline_bound);
        let inline_span = ctx.block().sub(I64, &inline_bound, &element_base);
        let object_inline_count = ctx.block().select(I1, &has_inline, I64, &inline_span, "0");
        let element_bytes = ctx.block().shl(I64, &element_base, "3");
        let object_header_size =
            crate::target_layout::object_header_size_bytes(ctx.target_triple).to_string();
        let inline_offset = ctx.block().add(I64, &object_header_size, &element_bytes);
        let object_inline_base = ctx.block().add(I64, &fast_raw, &inline_offset);

        // Only Array-subclass objects own ObjectMeta. Keep the metadata load
        // control-dependent so a plain Array never interprets element bits as
        // a pointer. A missing spill is valid when the admitted bound fits in
        // inline storage; the selected fallback address is then never loaded.
        let plain_setup_idx = ctx.new_block("stable_packed.setup.plain");
        let object_setup_idx = ctx.new_block("stable_packed.setup.object");
        let meta_setup_idx = ctx.new_block("stable_packed.setup.meta");
        let setup_merge_idx = ctx.new_block("stable_packed.setup.merge");
        let plain_setup_label = ctx.block_label(plain_setup_idx);
        let object_setup_label = ctx.block_label(object_setup_idx);
        let meta_setup_label = ctx.block_label(meta_setup_idx);
        let setup_merge_label = ctx.block_label(setup_merge_idx);
        ctx.block()
            .cond_br(&is_plain, &plain_setup_label, &object_setup_label);

        ctx.current_block = plain_setup_idx;
        ctx.block().br(&setup_merge_label);

        ctx.current_block = object_setup_idx;
        let pointer_size = if crate::target_layout::target_is_ilp32(ctx.target_triple) {
            4
        } else {
            8
        };
        let meta_offset = (crate::target_layout::object_header_size_bytes(ctx.target_triple)
            - pointer_size)
            .to_string();
        let meta_addr = ctx.block().add(I64, &fast_raw, &meta_offset);
        let meta_slot = ctx.block().inttoptr(I64, &meta_addr);
        let meta_native = ctx
            .block()
            .load(if pointer_size == 4 { I32 } else { I64 }, &meta_slot);
        let meta = if pointer_size == 4 {
            ctx.block().zext(I32, &meta_native, I64)
        } else {
            meta_native
        };
        let has_meta = ctx.block().icmp_ne(I64, &meta, "0");
        ctx.block()
            .cond_br(&has_meta, &meta_setup_label, &setup_merge_label);

        ctx.current_block = meta_setup_idx;
        let meta_ptr = ctx.block().inttoptr(I64, &meta);
        let spill_slot = ctx.block().gep(I64, &meta_ptr, &[(I64, "4")]);
        let spill = ctx.block().load(I64, &spill_slot);
        ctx.block().br(&setup_merge_label);

        ctx.current_block = setup_merge_idx;
        let spill = ctx.block().phi(
            I64,
            &[
                ("0", &plain_setup_label),
                ("0", &object_setup_label),
                (&spill, &meta_setup_label),
            ],
        );
        let has_spill = ctx.block().icmp_ne(I64, &spill, "0");
        let safe_spill = ctx.block().select(I1, &has_spill, I64, &spill, &fast_raw);
        let spill_offset = ctx.block().add(I64, &element_bytes, "8");
        let object_spill_base = ctx.block().add(I64, &safe_spill, &spill_offset);
        Some(StablePackedNumericAccess {
            is_plain,
            plain_base,
            object_inline_count,
            object_inline_base,
            object_spill_base,
        })
    } else {
        None
    };
    ctx.stable_packed_loop_facts.push(StablePackedLoopFact {
        counter_local_id: candidate.counter_id,
        array_local_id: candidate.array_id,
        side_exit_label: slow_pre_label.clone(),
        descriptor,
        live_receiver_handle: Some(fast_raw),
        numeric_elements: candidate.numeric_elements,
        numeric_access,
    });
    super::loops::lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.stable_packed_fast",
        Some((candidate.counter_id, bound_i32)),
    )?;
    ctx.stable_packed_loop_facts.pop();
    if !ctx.block().is_terminated() {
        // A call-free clone cannot grow or shrink its receiver, so exhausting
        // the admitted bound is also the exact live-length loop exit.
        ctx.block().br(&merge_label);
    }
    let fast_scan_end = ctx.func.num_blocks();
    let fast_clone_call_free = !ctx.func.blocks()[fast_pre_idx].contains_gc_unsafe_call()
        && (fast_scan_start..fast_scan_end)
            .all(|idx| !ctx.func.blocks()[idx].contains_gc_unsafe_call());
    ctx.current_block = admission_idx;
    if fast_clone_call_free {
        record_artifacts(ctx, candidate.array_id, &receiver);
        ctx.block()
            .cond_br(&admitted, &fast_pre_label, &slow_pre_label);
    } else {
        ctx.block().br(&slow_pre_label);
    }

    ctx.current_block = slow_pre_idx;
    super::loops::lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.stable_packed_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
    if inserted_counter {
        ctx.i32_counter_slots.remove(&candidate.counter_id);
    }
    Ok(true)
}
