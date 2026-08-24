//! Fallback-free loop version for checked indexed-reader callbacks.
//!
//! This recognizes a structural family rather than source names: a canonical
//! zero-based loop loads one entity from an array, optionally skips through an
//! immutable filter, evaluates one or more checked indexed-reader methods, and
//! invokes arbitrary callback code last. A preheader admits exact arrays and
//! the direct method target. Each fast iteration revalidates compact scalar
//! fingerprints before any effect; failure resumes the unchanged generic loop
//! at the current counter.

use std::collections::{BTreeSet, HashMap};

use anyhow::Result;
use perry_hir::{CompareOp, Expr, LogicalOp, Stmt, UpdateOp};

use crate::expr::{
    FnCtx, VersionedIndexedArrayFact, VersionedIndexedLoopFact, VersionedIndexedMethodFact,
};
use crate::types::{DOUBLE, I1, I128, I16, I32, I64, I8};

#[derive(Clone)]
struct Candidate {
    counter_id: u32,
    bound_id: u32,
    filter_id: Option<u32>,
    arrays: Vec<u32>,
    class_name: String,
    method_name: String,
}

fn checked_reader_call(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    counter_id: u32,
) -> Option<(String, String, Vec<u32>)> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::PropertyGet {
        object, property, ..
    } = callee.as_ref()
    else {
        return None;
    };
    if !matches!(object.as_ref(), Expr::This) {
        return None;
    }
    let class_name = ctx.class_stack.last()?.clone();
    let key = (class_name.clone(), property.clone());
    let index_params = ctx.nonnegative_index_methods.get(&key)?;
    let method = ctx
        .classes
        .get(&class_name)?
        .methods
        .iter()
        .find(|method| method.name == *property)?;
    if args.len() != method.params.len()
        || !index_params.iter().all(|id| {
            method
                .params
                .iter()
                .position(|param| param.id == *id)
                .and_then(|position| args.get(position))
                .is_some_and(|arg| matches!(arg, Expr::LocalGet(id) if *id == counter_id))
        })
    {
        return None;
    }
    let array_params = crate::codegen::nonnegative_index_fast_array_params(method, index_params);
    if array_params.is_empty() {
        return None;
    }
    let mut arrays = Vec::with_capacity(array_params.len());
    for array_param in array_params {
        let position = method
            .params
            .iter()
            .position(|param| param.id == array_param)?;
        let Expr::LocalGet(local_id) = args.get(position)? else {
            return None;
        };
        arrays.push(*local_id);
    }
    Some((class_name, property.clone(), arrays))
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
    let bound_id = match condition? {
        Expr::Compare {
            op: CompareOp::Lt,
            left,
            right,
        } if matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) => {
            match right.as_ref() {
                Expr::LocalGet(id) => *id,
                _ => return None,
            }
        }
        _ => return None,
    };
    if !matches!(
        update,
        Some(Expr::Update {
            id,
            op: UpdateOp::Increment,
            ..
        }) if *id == counter_id
    ) || ctx.boxed_vars.contains(&counter_id)
        || ctx.closure_captures.contains_key(&counter_id)
        || super::loops::stmts_mutate_local(body, counter_id)
        || ctx.reassigned_locals.contains(&bound_id)
        || ctx.boxed_vars.contains(&bound_id)
        || !ctx.locals.contains_key(&bound_id)
    {
        return None;
    }

    let (entity_stmt, filter_stmt, callback_stmt) = match body {
        [entity, filter, callback] => (entity, Some(filter), callback),
        [entity, callback] => (entity, None, callback),
        _ => return None,
    };
    let (entity_id, entity_array_id) = match entity_stmt {
        Stmt::Let {
            id,
            init: Some(Expr::IndexGet { object, index }),
            ..
        } if matches!(index.as_ref(), Expr::LocalGet(id) if *id == counter_id) => {
            match object.as_ref() {
                Expr::LocalGet(array_id) => (*id, *array_id),
                _ => return None,
            }
        }
        _ => return None,
    };
    let filter_id = match filter_stmt {
        Some(Stmt::If {
            condition:
                Expr::Logical {
                    op: LogicalOp::And,
                    left,
                    ..
                },
            then_branch,
            else_branch: None,
        }) if matches!(then_branch.as_slice(), [Stmt::Continue]) => match left.as_ref() {
            Expr::LocalGet(id) if !ctx.boxed_vars.contains(id) && ctx.locals.contains_key(id) => {
                Some(*id)
            }
            _ => return None,
        },
        Some(_) => return None,
        None => None,
    };

    let Expr::Call {
        callee: callback,
        args: callback_args,
        ..
    } = (match callback_stmt {
        Stmt::Expr(expr) => expr,
        _ => return None,
    })
    else {
        return None;
    };
    let callback_id = match callback.as_ref() {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    if ctx.reassigned_locals.contains(&callback_id)
        || ctx.boxed_vars.contains(&callback_id)
        || !matches!(callback_args.first(), Some(Expr::LocalGet(id)) if *id == entity_id)
        || callback_args.len() < 2
    {
        return None;
    }

    let mut arrays = BTreeSet::from([entity_array_id]);
    let mut selected_method: Option<(String, String)> = None;
    for arg in &callback_args[1..] {
        let (class_name, method_name, method_arrays) = checked_reader_call(ctx, arg, counter_id)?;
        if selected_method
            .as_ref()
            .is_some_and(|selected| selected != &(class_name.clone(), method_name.clone()))
        {
            return None;
        }
        selected_method = Some((class_name, method_name));
        arrays.extend(method_arrays);
    }
    let (class_name, method_name) = selected_method?;

    // Every retained pointer is an immutable plain local with an exact shadow
    // root. The callback cannot rebind these lexical slots; moving GC rewrites
    // them, and the next iteration reloads the rewritten box before dereference.
    if arrays.iter().any(|id| {
        ctx.reassigned_locals.contains(id)
            || ctx.boxed_vars.contains(id)
            || ctx.closure_captures.contains_key(id)
            || ctx.module_globals.contains_key(id)
            || !ctx.locals.contains_key(id)
            || !ctx.shadow_slot_map.contains_key(id)
    }) {
        return None;
    }
    let this_slot = ctx.this_stack.last()?;
    if this_slot.is_empty() {
        return None;
    }

    Some(Candidate {
        counter_id,
        bound_id,
        filter_id,
        arrays: arrays.into_iter().collect(),
        class_name,
        method_name,
    })
}

fn emit_array_admission(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    bound_i32: &str,
    success_label: &str,
    slow_label: &str,
) -> Option<(String, String)> {
    let local_slot = ctx.locals.get(&local_id)?.clone();
    let deref_idx = ctx.new_block("versioned_index.array.deref");
    let deref_label = ctx.block_label(deref_idx);
    let heap_floor =
        crate::target_layout::heap_addr_lower_bound_inclusive(ctx.target_triple).to_string();
    let heap_ceiling =
        crate::target_layout::heap_addr_upper_bound_exclusive(ctx.target_triple).to_string();

    let array_box = ctx.block().load(DOUBLE, &local_slot);
    let array_bits = ctx.block().bitcast_double_to_i64(&array_box);
    let array_handle = ctx
        .block()
        .and(I64, &array_bits, crate::nanbox::POINTER_MASK_I64);
    let tag = ctx.block().lshr(I64, &array_bits, "48");
    let is_pointer = ctx.block().icmp_eq(I64, &tag, "32765");
    let above_floor = ctx.block().icmp_uge(I64, &array_handle, &heap_floor);
    let below_ceiling = ctx.block().icmp_ult(I64, &array_handle, &heap_ceiling);
    let in_heap = ctx.block().and(I1, &above_floor, &below_ceiling);
    let safe = ctx.block().and(I1, &is_pointer, &in_heap);
    ctx.block().cond_br(&safe, &deref_label, slow_label);

    ctx.current_block = deref_idx;
    let fingerprint_addr = ctx.block().sub(I64, &array_handle, "8");
    let fingerprint_ptr = ctx.block().inttoptr(I64, &fingerprint_addr);
    let fingerprint = ctx.block().load_aligned(I128, &fingerprint_ptr, 8);
    let gc_header = ctx.block().trunc(I128, &fingerprint, I64);
    let array_header = ctx.block().lshr(I128, &fingerprint, "64");
    let gc_type = ctx.block().trunc(I64, &gc_header, I8);
    let is_array = ctx.block().icmp_eq(I8, &gc_type, "1");
    let flags_shifted = ctx.block().lshr(I64, &gc_header, "8");
    let flags = ctx.block().trunc(I64, &flags_shifted, I8);
    let forwarded = ctx.block().and(I8, &flags, "128");
    let not_forwarded = ctx.block().icmp_eq(I8, &forwarded, "0");
    let reserved_shifted = ctx.block().lshr(I64, &gc_header, "16");
    let reserved = ctx.block().trunc(I64, &reserved_shifted, I16);
    let descriptors = ctx.block().and(I16, &reserved, "1024");
    let no_descriptors = ctx.block().icmp_eq(I16, &descriptors, "0");
    let prototype_invalidated = ctx
        .block()
        .load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let prototype_ok = ctx.block().icmp_eq(I8, &prototype_invalidated, "0");
    let length = ctx.block().trunc(I128, &array_header, I32);
    let capacity_shifted = ctx.block().lshr(I128, &array_header, "32");
    let capacity = ctx.block().trunc(I128, &capacity_shifted, I32);
    let bound_fits = ctx.block().icmp_ule(I32, bound_i32, &length);
    let length_sane = ctx.block().icmp_ule(I32, &length, "16000000");
    let capacity_sane = ctx.block().icmp_ule(I32, &capacity, "16000000");
    let length_within_capacity = ctx.block().icmp_ule(I32, &length, &capacity);
    let mut pass = ctx.block().and(I1, &is_array, &not_forwarded);
    pass = ctx.block().and(I1, &pass, &no_descriptors);
    pass = ctx.block().and(I1, &pass, &prototype_ok);
    pass = ctx.block().and(I1, &pass, &bound_fits);
    pass = ctx.block().and(I1, &pass, &length_sane);
    pass = ctx.block().and(I1, &pass, &capacity_sane);
    pass = ctx.block().and(I1, &pass, &length_within_capacity);
    ctx.block().cond_br(&pass, success_label, slow_label);
    Some((local_slot, fingerprint))
}

/// Emit the compact per-iteration check and publish fresh live handles for the
/// fallback-free body. Returns true when a fact was consumed.
pub(super) fn emit_iteration_guard(ctx: &mut FnCtx<'_>) -> bool {
    let Some(fact) = ctx.versioned_indexed_loop_facts.last().cloned() else {
        return false;
    };
    let continue_idx = ctx.new_block("versioned_index.iteration.fast");
    let continue_label = ctx.block_label(continue_idx);
    let array_invalidated = ctx
        .block()
        .load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let mut pass = ctx.block().icmp_eq(I8, &array_invalidated, "0");
    let mut live_handles = HashMap::new();

    for array in &fact.arrays {
        let array_box = ctx.block().load(DOUBLE, &array.local_slot);
        let array_bits = ctx.block().bitcast_double_to_i64(&array_box);
        let array_handle = ctx
            .block()
            .and(I64, &array_bits, crate::nanbox::POINTER_MASK_I64);
        let fingerprint_addr = ctx.block().sub(I64, &array_handle, "8");
        let fingerprint_ptr = ctx.block().inttoptr(I64, &fingerprint_addr);
        let current = ctx.block().load_aligned(I128, &fingerprint_ptr, 8);
        let unchanged = ctx
            .block()
            .icmp_eq(I128, &current, &array.expected_fingerprint);
        pass = ctx.block().and(I1, &pass, &unchanged);
        live_handles.insert(array.local_id, array_handle);
    }

    let all_invalidated =
        ctx.block()
            .load_atomic_acquire(I8, "@PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED", 1);
    let all_methods_ok = ctx.block().icmp_eq(I8, &all_invalidated, "0");
    let method_slot_ptr = ctx.block().gep(
        I8,
        "@PERRY_CLASS_PROTOTYPE_FAST_GUARDS_INVALIDATED_BY_METHOD",
        &[(I64, &fact.method.method_guard_slot)],
    );
    let method_invalidated = ctx.block().load_atomic_acquire(I8, &method_slot_ptr, 1);
    let method_ok = ctx.block().icmp_eq(I8, &method_invalidated, "0");
    let this_box = ctx.block().load(DOUBLE, &fact.method.this_slot);
    let this_bits = ctx.block().bitcast_double_to_i64(&this_box);
    let this_handle = ctx
        .block()
        .and(I64, &this_bits, crate::nanbox::POINTER_MASK_I64);
    let object_ptr = ctx.block().inttoptr(I64, &this_handle);
    let gc_header_ptr = ctx.block().gep(I8, &object_ptr, &[(I64, "-8")]);
    let gc_header = ctx.block().load(I32, &gc_header_ptr);
    let guarded_gc_bits = ctx.block().and(I32, &gc_header, "134250751");
    let gc_ok = ctx.block().icmp_eq(I32, &guarded_gc_bits, "2");
    let class_shape = ctx.block().load(I64, &object_ptr);
    let expected_shape_i64 = ctx.block().zext(I32, &fact.method.expected_shape_id, I64);
    let expected_shape_high = ctx.block().shl(I64, &expected_shape_i64, "32");
    let expected_class_shape =
        ctx.block()
            .or(I64, &expected_shape_high, &fact.method.expected_class_id);
    let class_shape_ok = ctx
        .block()
        .icmp_eq(I64, &class_shape, &expected_class_shape);
    pass = ctx.block().and(I1, &pass, &all_methods_ok);
    pass = ctx.block().and(I1, &pass, &method_ok);
    pass = ctx.block().and(I1, &pass, &gc_ok);
    pass = ctx.block().and(I1, &pass, &class_shape_ok);
    ctx.block()
        .cond_br(&pass, &continue_label, &fact.side_exit_label);

    ctx.current_block = continue_idx;
    if let Some(active) = ctx.versioned_indexed_loop_facts.last_mut() {
        active.live_array_handles = live_handles;
    }
    true
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

    let fast_pre_idx = ctx.new_block("versioned_index.loop.fast.preheader");
    let slow_pre_idx = ctx.new_block("versioned_index.loop.slow.preheader");
    let merge_idx = ctx.new_block("versioned_index.loop.merge");
    let convert_idx = ctx.new_block("versioned_index.bound.convert");
    let fast_pre_label = ctx.block_label(fast_pre_idx);
    let slow_pre_label = ctx.block_label(slow_pre_idx);
    let merge_label = ctx.block_label(merge_idx);
    let convert_label = ctx.block_label(convert_idx);

    let bound_slot = ctx
        .locals
        .get(&candidate.bound_id)
        .expect("matched local bound has storage")
        .clone();
    let bound_box = ctx.block().load(DOUBLE, &bound_slot);
    let bound_is_i32 = crate::codegen::emit_typed_arg_guard(
        ctx.block(),
        crate::codegen::TypedParamRep::I32,
        &bound_box,
    );
    ctx.block()
        .cond_br(&bound_is_i32, &convert_label, &slow_pre_label);

    ctx.current_block = convert_idx;
    let bound_i32 = crate::codegen::emit_typed_arg_to_raw(
        ctx.block(),
        crate::codegen::TypedParamRep::I32,
        &bound_box,
    );
    let bound_nonnegative = ctx.block().icmp_sge(I32, &bound_i32, "0");

    let array_entry_idxs: Vec<usize> = candidate
        .arrays
        .iter()
        .map(|_| ctx.new_block("versioned_index.array.admit"))
        .collect();
    let array_entry_labels: Vec<String> = array_entry_idxs
        .iter()
        .map(|idx| ctx.block_label(*idx))
        .collect();
    let method_entry_idx = ctx.new_block("versioned_index.method.admit");
    let method_entry_label = ctx.block_label(method_entry_idx);
    ctx.block()
        .cond_br(&bound_nonnegative, &array_entry_labels[0], &slow_pre_label);

    let mut array_facts = Vec::with_capacity(candidate.arrays.len());
    for (position, local_id) in candidate.arrays.iter().copied().enumerate() {
        ctx.current_block = array_entry_idxs[position];
        let next = array_entry_labels
            .get(position + 1)
            .map(String::as_str)
            .unwrap_or(method_entry_label.as_str());
        let Some((local_slot, expected_fingerprint)) =
            emit_array_admission(ctx, local_id, &bound_i32, next, &slow_pre_label)
        else {
            return Ok(false);
        };
        array_facts.push(VersionedIndexedArrayFact {
            local_id,
            local_slot,
            expected_fingerprint,
        });
    }

    ctx.current_block = method_entry_idx;
    if let Some(filter_id) = candidate.filter_id {
        let filter_slot = ctx
            .locals
            .get(&filter_id)
            .expect("matched filter local has storage")
            .clone();
        let filter_box = ctx.block().load(DOUBLE, &filter_slot);
        let filter_bits = ctx.block().bitcast_double_to_i64(&filter_box);
        let filter_is_undefined = ctx.block().icmp_eq(
            I64,
            &filter_bits,
            &crate::nanbox::TAG_UNDEFINED_I64.to_string(),
        );
        let filter_ok_idx = ctx.new_block("versioned_index.filter.falsy");
        let filter_ok_label = ctx.block_label(filter_ok_idx);
        ctx.block()
            .cond_br(&filter_is_undefined, &filter_ok_label, &slow_pre_label);
        ctx.current_block = filter_ok_idx;
    }

    let expected_class_id = ctx
        .class_ids
        .get(&candidate.class_name)
        .expect("matched class has a runtime id")
        .to_string();
    let keys_global = ctx
        .class_keys_globals
        .get(&candidate.class_name)
        .expect("matched class has a keys global")
        .clone();
    let expected_shape_id =
        crate::typed_shape::load_class_shape_id(ctx, &candidate.class_name, &keys_global);
    let key_idx = ctx.strings.intern(&candidate.method_name);
    let method_guard_slot = (ctx.strings.entry(key_idx).dispatch_hash & 0xffff).to_string();
    let this_slot = ctx
        .this_stack
        .last()
        .expect("matched method body has this storage")
        .clone();
    let this_box = ctx.block().load(DOUBLE, &this_slot);
    crate::lower_call::emit_inline_direct_method_shape_guard(
        ctx,
        &this_box,
        &expected_class_id,
        &expected_shape_id,
        &method_guard_slot,
        &fast_pre_label,
        &slow_pre_label,
    );

    let method_fact = VersionedIndexedMethodFact {
        class_name: candidate.class_name.clone(),
        method_name: candidate.method_name.clone(),
        this_slot,
        expected_class_id,
        expected_shape_id,
        method_guard_slot,
    };
    ctx.current_block = fast_pre_idx;
    ctx.versioned_indexed_loop_facts
        .push(VersionedIndexedLoopFact {
            counter_local_id: candidate.counter_id,
            falsy_local_id: candidate.filter_id,
            side_exit_label: slow_pre_label.clone(),
            arrays: array_facts,
            method: method_fact,
            live_array_handles: HashMap::new(),
        });
    super::loops::lower_for_after_init_with_i32_bound(
        ctx,
        init,
        condition,
        update,
        body,
        "for.versioned_index_fast",
        Some((candidate.counter_id, bound_i32.clone())),
    )?;
    ctx.versioned_indexed_loop_facts.pop();
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }

    ctx.current_block = slow_pre_idx;
    super::loops::lower_for_after_init(
        ctx,
        init,
        condition,
        update,
        body,
        "for.versioned_index_slow",
    )?;
    if !ctx.block().is_terminated() {
        ctx.block().br(&merge_label);
    }
    ctx.current_block = merge_idx;
    Ok(true)
}
