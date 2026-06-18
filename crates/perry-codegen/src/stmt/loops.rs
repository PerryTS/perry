//! `Stmt::For`, `Stmt::While`, `Stmt::DoWhile` lowering and supporting helpers.

use super::*;

use crate::expr::{
    emit_typed_feedback_register_site, expr_has_numeric_pointer_free_array_layout,
    lower_guarded_array_index_get_trusted_i32, BoundedIndexPair, IntRangeFact,
    PreguardedAffineIndexExpr, PreguardedModuloIndexExpr, PreguardedNumericArrayAffineIndexGet,
    PreguardedNumericArrayAffineIndexSet, PreguardedNumericArrayIndexGet,
    PreguardedNumericArrayIndexSet, PreguardedNumericArrayModuloIndexGet,
    PreguardedPlainArrayIndexSet, TypedFeedbackContract, TypedFeedbackKind, MAX_SAFE_INTEGER_I64,
};
use crate::loop_purity::body_needs_asm_barrier;
use crate::lower_conditional::lower_truthy;
use crate::nanbox::POINTER_MASK_I64;
use crate::native_value::{BoundedBufferIndex, BoundsProof, BoundsState, LengthSource};
use crate::type_analysis::{is_array_expr, is_numeric_expr};
use crate::types::{DOUBLE, I32, I64, I8};

#[derive(Clone, Copy, Debug)]
struct InvariantArrayIndexGetHoist {
    array_local_id: u32,
    index_local_id: u32,
    inner_counter_local_id: u32,
}

#[derive(Clone, Copy, Debug)]
struct RangeNumericArrayIndexGetPreguard {
    array_local_id: u32,
    index_local_id: u32,
}

#[derive(Clone, Copy, Debug)]
struct RangeNumericArrayIndexSetPreguard {
    array_local_id: u32,
    index_local_id: u32,
}

#[derive(Clone, Debug)]
enum PlainArraySetMaxIndex {
    ArrayLength,
    BoundExpr(perry_hir::Expr),
}

#[derive(Clone, Debug)]
struct RangePlainArrayIndexSetPreguard {
    array_local_id: u32,
    index_local_id: u32,
    max_index: PlainArraySetMaxIndex,
    f64_numeric_update: bool,
}

#[derive(Clone, Debug)]
struct AffineNumericArrayIndexGetPreguard {
    array_local_id: u32,
    index: PreguardedAffineIndexExpr,
}

#[derive(Clone, Debug)]
struct ModuloNumericArrayIndexGetPreguard {
    array_local_id: u32,
    index: PreguardedModuloIndexExpr,
}

#[derive(Clone, Debug)]
struct AffineNumericArrayIndexSetPreguard {
    array_local_id: u32,
    index: PreguardedAffineIndexExpr,
}

#[derive(Clone, Debug)]
struct I64LoopAccumulator {
    local_id: u32,
    slot: String,
}

#[derive(Clone, Copy, Debug)]
struct AccumulatorClosedForm {
    acc_local_id: u32,
    counter_local_id: u32,
    final_counter: i64,
    final_acc: i64,
}

#[derive(Clone, Debug)]
struct DirectFieldAccumulatorClosedForm {
    object_local_id: u32,
    field: String,
    field_index: u32,
    counter_local_id: u32,
    final_counter: i64,
    final_field: i64,
}

#[derive(Clone, Copy, Debug)]
struct AffineAccumulatorAddend {
    coeff: i128,
    constant: i128,
}

/// For-loop lowering: classic init / cond / body / update / exit CFG.
///
/// ```text
///   <current>:
///     <init>
///     br cond
///   for.cond:
///     <condition>          ; if missing, treat as `true` (infinite loop)
///     fcmp one cond, 0.0
///     br i1, body, exit
///   for.body:
///     <body>
///     br update            ; if not already terminated
///   for.update:
///     <update>
///     br cond              ; if not already terminated
///   for.exit:
///     <continues here>
/// ```
///
/// Phase 2.1 does not support `break` / `continue`. The body must fall
/// through to update; otherwise codegen produces dead code that LLVM will
/// reject. We don't yet pass the loop's break/continue targets through
/// FnCtx — that lands when we need it.
pub(crate) fn lower_for(
    ctx: &mut FnCtx<'_>,
    init: Option<&Stmt>,
    condition: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Result<()> {
    // Init runs once in the current block. A `let i = 0` here adds `i` to
    // ctx.locals, which the body can then load via LocalGet.
    if let Some(init_stmt) = init {
        lower_stmt(ctx, init_stmt)?;
    }
    let loop_proof_scope_id = ctx.next_loop_proof_scope_id();

    // Loop-invariant length hoisting peephole. Detect the very common
    // shape `for (...; i < arr.length; ...)` where `arr` is a local
    // that the body never mutates length-wise, and pre-load
    // `arr.length` into a stack slot before entering the cond block.
    // The length load inside the cond is then replaced with a load
    // from the slot — saves two instructions per iteration (the
    // `and` to unbox arr + the `ldr` of the length field) and lets
    // LLVM hoist a couple more downstream loads now that the slot
    // is the loop-invariant source of truth.
    //
    // Without this, LLVM's LICM declines to hoist the length load
    // because the loop body's `IndexSet` slow path (`js_array_set_f64
    // _extend`) is an external call that LLVM can't prove won't
    // modify the array's length field. We do the analysis ourselves
    // and only hoist when our (more domain-specific) walker can
    // prove the body won't change `arr.length`.
    //
    // Saves ~25-30% on `for (let i = 0; i < arr.length; i++) arr[i] = i`
    // and `for (let i = 0; i < arr.length; i++) for (let j = 0; j <
    // arr.length; j++) ...` patterns.
    let hoist_classification: Option<(u32, u32, perry_hir::CompareOp)> =
        condition.and_then(|cond| classify_for_length_hoist(cond, body));
    let hoisted_length_arr_id: Option<u32> = hoist_classification.map(|(arr, _, _)| arr);
    let hoisted_index_bounds_are_safe = hoist_classification.is_some_and(|(_, counter_id, op)| {
        matches!(op, perry_hir::CompareOp::Lt)
            && loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    });
    let hoisted_length_slot: Option<String> =
        if let Some((arr_id, counter_id, _op)) = hoist_classification {
            let arr_box_loaded = lower_expr(
                ctx,
                &perry_hir::Expr::PropertyGet {
                    object: Box::new(perry_hir::Expr::LocalGet(arr_id)),
                    property: "length".to_string(),
                },
            )?;
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.block().store(DOUBLE, &arr_box_loaded, &slot);
            ctx.cached_lengths.insert(arr_id, slot.clone());
            // Also tell `lower_index_set_fast` (and similar sites) that
            // `arr[counter_id]` is statically inbounds for this body, so
            // it can skip the runtime length-load + bound check.
            if hoisted_index_bounds_are_safe {
                ctx.bounded_index_pairs.push(BoundedIndexPair {
                    index_local_id: counter_id,
                    array_local_id: arr_id,
                    scope_id: loop_proof_scope_id,
                });
                if ctx.buffer_view_slots.contains_key(&arr_id) {
                    ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                        index_local_id: counter_id,
                        buffer_local_id: arr_id,
                        scope_id: loop_proof_scope_id,
                        bounds_width_units: 1,
                        bounds: BoundsState::Proven {
                            proof: BoundsProof::LoopGuard,
                        },
                    });
                }
            }

            // If the counter is provably integer-valued (initialized from
            // an Integer literal, only mutated via Update ++/--), allocate
            // a parallel i32 slot. The Update lowering will keep it in sync,
            // and IndexGet/IndexSet will load the i32 directly instead of
            // emitting a `fptosi double → i32` on every iteration.
            if ctx.integer_locals.contains(&counter_id) {
                if let Some(counter_slot) = ctx.locals.get(&counter_id).cloned() {
                    let i32_slot = ctx.func.alloca_entry(I32);
                    // Initialize from the current double value.
                    let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                    let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                    ctx.block().store(I32, &cur_i32, &i32_slot);
                    ctx.i32_counter_slots.insert(counter_id, i32_slot);
                }
            }

            Some(slot)
        } else {
            None
        };

    // If we have an i32 counter AND a hoisted length, pre-compute the
    // length as i32 so the loop condition can use `icmp slt/sle i32`
    // instead of `fcmp olt/ole double`. This eliminates the float counter fadd +
    // fcmp per iteration — saves ~2 instructions on the inner loop of
    // nested_loops and similar patterns.
    let i32_length_slot: Option<String> = if let Some((_, counter_id, _op)) = hoist_classification {
        if let (Some(_), Some(len_dbl_slot)) = (
            ctx.i32_counter_slots.get(&counter_id).cloned(),
            hoisted_length_slot.as_ref(),
        ) {
            let len_dbl = ctx.block().load(DOUBLE, len_dbl_slot);
            let len_i32 = ctx.block().fptosi(DOUBLE, &len_dbl, I32);
            let slot = ctx.func.alloca_entry(I32);
            ctx.block().store(I32, &len_i32, &slot);
            Some(slot)
        } else {
            None
        }
    } else {
        None
    };

    // Issue #168: when the `i < arr.length` peephole didn't fire, also
    // detect the simpler `i < n` shape where `n` is a number-typed local
    // or function parameter. Emitting `fptosi(n)` once at the loop head
    // and using `icmp slt i32 %i, %n.i32` in the condition block
    // replaces `fcmp olt double`, letting LLVM's SCEV model `i` as a
    // clean integer induction variable — prerequisite for LoopVectorizer
    // to widen Buffer-read and similar intrinsic-heavy bodies.
    let local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)> =
        if hoist_classification.is_none() {
            condition.and_then(|cond| classify_for_local_bound(cond, ctx))
        } else {
            None
        };
    // Track whether *we* allocated the counter's i32 slot (vs. the Let
    // site having done so already).  Only the site that inserted should
    // remove it at loop exit to avoid disturbing a pre-existing slot.
    let local_bound_counter_i32_was_fresh: bool;
    let local_bound_bound_i32_was_fresh: bool;
    let i32_local_bound_slot: Option<String> =
        if let Some((counter_id, bound_id, _op)) = local_bound_classification {
            // Allocate a parallel i32 slot for the counter if not already
            // present.  Counters that fall outside `integer_locals`
            // (e.g. `for (let i = 0; i < arr.length; i++)` where `i` is
            // captured by a closure or escapes) skip the Let-site
            // allocation; providing one here enables both `icmp slt i32`
            // in the condition and `add i32 1` in Update.
            let fresh = if !ctx.i32_counter_slots.contains_key(&counter_id) {
                if let Some(counter_slot) = ctx.locals.get(&counter_id).cloned() {
                    let i32_slot = ctx.func.alloca_entry(I32);
                    let cur_dbl = ctx.block().load(DOUBLE, &counter_slot);
                    let cur_i32 = ctx.block().fptosi(DOUBLE, &cur_dbl, I32);
                    ctx.block().store(I32, &cur_i32, &i32_slot);
                    ctx.i32_counter_slots.insert(counter_id, i32_slot);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            local_bound_counter_i32_was_fresh = fresh;
            // Hoist `fptosi(n)` to a fresh i32 alloca before the cond block
            // so LLVM sees a loop-invariant integer bound — critical for
            // SCEV / LoopVectorizer to recognize the induction variable. Also
            // expose that slot while lowering the loop body so integer index
            // expressions like `i * n + k` can reuse the same trusted bound
            // instead of rebuilding the index through double arithmetic.
            if let Some(existing) = ctx.i32_counter_slots.get(&bound_id).cloned() {
                local_bound_bound_i32_was_fresh = false;
                Some(existing)
            } else if let Some(bound_slot) = ctx.locals.get(&bound_id).cloned() {
                let bound_dbl = ctx.block().load(DOUBLE, &bound_slot);
                let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_dbl, I32);
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &bound_i32, &slot);
                ctx.i32_counter_slots.insert(bound_id, slot.clone());
                local_bound_bound_i32_was_fresh = true;
                Some(slot)
            } else {
                local_bound_bound_i32_was_fresh = false;
                None
            }
        } else {
            local_bound_counter_i32_was_fresh = false;
            local_bound_bound_i32_was_fresh = false;
            None
        };
    let local_bound_index_bounds_are_safe =
        local_bound_classification.is_some_and(|(counter_id, _, op)| {
            matches!(op, perry_hir::CompareOp::Lt)
                && loop_counter_bounds_are_safe(ctx, counter_id, update, body)
        });
    if let Some((counter_id, bound_id, _op)) = local_bound_classification {
        if local_bound_index_bounds_are_safe {
            if let Some(buffer_ids) = ctx.min_length_bounds.get(&bound_id).cloned() {
                for buffer_local_id in buffer_ids {
                    if ctx.buffer_view_slots.contains_key(&buffer_local_id) {
                        ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                            index_local_id: counter_id,
                            buffer_local_id,
                            scope_id: loop_proof_scope_id,
                            bounds_width_units: 1,
                            bounds: BoundsState::Proven {
                                proof: BoundsProof::MinLength,
                            },
                        });
                    }
                }
            }
            let alloc_bound_ids: Vec<u32> = ctx
                .buffer_view_slots
                .iter()
                .filter_map(|(buffer_local_id, view)| match &view.length_source {
                    Some(LengthSource::Local { id, addend }) if *id == bound_id && *addend >= 0 => {
                        Some(*buffer_local_id)
                    }
                    _ => None,
                })
                .collect();
            for buffer_local_id in alloc_bound_ids {
                ctx.bounded_buffer_index_pairs.push(BoundedBufferIndex {
                    index_local_id: counter_id,
                    buffer_local_id,
                    scope_id: loop_proof_scope_id,
                    bounds_width_units: 1,
                    bounds: BoundsState::Proven {
                        proof: BoundsProof::LoopGuard,
                    },
                });
            }
        }
    }
    if let Some(fact) =
        classify_for_counter_range(init, condition, update, ctx, loop_proof_scope_id)
    {
        ctx.int_range_facts.push(fact);
    }

    let invariant_array_get_hoist: Option<InvariantArrayIndexGetHoist> =
        if let (Some((arr_id, inner_counter_id, op)), Some(_)) =
            (hoist_classification, i32_length_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && hoisted_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&inner_counter_id)
            {
                classify_invariant_numeric_array_index_get_hoist(
                    ctx,
                    arr_id,
                    inner_counter_id,
                    update,
                    body,
                )
            } else {
                None
            }
        } else {
            None
        };
    let invariant_array_get_slot: Option<String> = invariant_array_get_hoist
        .as_ref()
        .map(|_| ctx.func.alloca_entry(DOUBLE));
    let range_array_get_preguard: Option<RangeNumericArrayIndexGetPreguard> =
        if let (Some((arr_id, counter_id, op)), Some(_)) =
            (hoist_classification, i32_length_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && hoisted_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&counter_id)
            {
                classify_range_numeric_array_index_get_preguard(
                    ctx,
                    arr_id,
                    counter_id,
                    invariant_array_get_hoist.map(|hoist| hoist.index_local_id),
                    update,
                    body,
                )
            } else {
                None
            }
        } else {
            None
        };
    let range_array_set_preguard: Option<RangeNumericArrayIndexSetPreguard> =
        if let (Some((arr_id, counter_id, op)), Some(_)) =
            (hoist_classification, i32_length_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && hoisted_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&counter_id)
            {
                classify_range_numeric_array_index_set_preguard(
                    ctx, arr_id, counter_id, update, body,
                )
            } else {
                None
            }
        } else {
            None
        };
    let range_plain_array_set_preguard: Option<RangePlainArrayIndexSetPreguard> =
        if range_array_set_preguard.is_none() {
            if let Some((arr_id, counter_id, op)) = hoist_classification {
                if matches!(op, perry_hir::CompareOp::Lt) {
                    classify_range_plain_array_index_set_preguard_for_bounds(
                        ctx,
                        arr_id,
                        counter_id,
                        PlainArraySetMaxIndex::ArrayLength,
                        update,
                        body,
                    )
                } else {
                    None
                }
            } else {
                condition.and_then(|cond| {
                    classify_range_plain_array_index_set_preguard_for_condition(
                        ctx, cond, update, body,
                    )
                })
            }
        } else {
            None
        };
    let affine_array_get_preguards: Vec<AffineNumericArrayIndexGetPreguard> =
        if let (Some((counter_id, bound_id, op)), Some(_)) =
            (local_bound_classification, i32_local_bound_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && local_bound_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&counter_id)
                && ctx.i32_counter_slots.contains_key(&bound_id)
            {
                classify_affine_numeric_array_index_get_preguards(
                    ctx, counter_id, bound_id, update, body,
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
    let modulo_array_get_preguards: Vec<ModuloNumericArrayIndexGetPreguard> =
        if let (Some((counter_id, _, op)), Some(_)) =
            (local_bound_classification, i32_local_bound_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && local_bound_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&counter_id)
            {
                classify_modulo_numeric_array_index_get_preguards(ctx, counter_id, update, body)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
    let affine_array_set_preguards: Vec<AffineNumericArrayIndexSetPreguard> =
        if let (Some((counter_id, bound_id, op)), Some(_)) =
            (local_bound_classification, i32_local_bound_slot.as_ref())
        {
            if matches!(op, perry_hir::CompareOp::Lt)
                && local_bound_index_bounds_are_safe
                && ctx.i32_counter_slots.contains_key(&counter_id)
                && ctx.i32_counter_slots.contains_key(&bound_id)
            {
                classify_affine_numeric_array_index_set_preguards(
                    ctx, counter_id, bound_id, update, body,
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
    let has_loop_prebody = invariant_array_get_hoist.is_some()
        || range_array_get_preguard.is_some()
        || range_array_set_preguard.is_some()
        || range_plain_array_set_preguard.is_some()
        || !affine_array_get_preguards.is_empty()
        || !modulo_array_get_preguards.is_empty()
        || !affine_array_set_preguards.is_empty();
    let i32_only_counter_update = classify_i32_only_counter_update(ctx, init, update, body);
    let i64_loop_accumulator =
        prepare_i64_loop_accumulator(ctx, local_bound_classification, update, body);
    if let Some((acc, closed_form)) = i64_loop_accumulator.as_ref().and_then(|acc| {
        classify_constant_accumulator_closed_form(
            ctx,
            local_bound_classification,
            update,
            body,
            acc,
        )
        .or_else(|| {
            classify_affine_accumulator_closed_form(
                ctx,
                local_bound_classification,
                update,
                body,
                acc,
            )
        })
        .or_else(|| {
            classify_modulo_accumulator_closed_form(
                ctx,
                local_bound_classification,
                update,
                body,
                acc,
            )
        })
        .map(|closed_form| (acc, closed_form))
    }) {
        emit_accumulator_closed_form(ctx, acc, closed_form);
        if local_bound_counter_i32_was_fresh {
            if let Some((counter_id, _, _)) = local_bound_classification {
                ctx.i32_counter_slots.remove(&counter_id);
            }
        }
        if local_bound_bound_i32_was_fresh {
            if let Some((_, bound_id, _)) = local_bound_classification {
                ctx.i32_counter_slots.remove(&bound_id);
            }
        }
        ctx.bounded_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.bounded_buffer_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.guarded_buffer_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.int_range_facts
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        return Ok(());
    }
    if let Some(closed_form) =
        classify_direct_field_accumulator_closed_form(ctx, local_bound_classification, update, body)
    {
        emit_direct_field_accumulator_closed_form(ctx, closed_form)?;
        if local_bound_counter_i32_was_fresh {
            if let Some((counter_id, _, _)) = local_bound_classification {
                ctx.i32_counter_slots.remove(&counter_id);
            }
        }
        if local_bound_bound_i32_was_fresh {
            if let Some((_, bound_id, _)) = local_bound_classification {
                ctx.i32_counter_slots.remove(&bound_id);
            }
        }
        ctx.bounded_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.bounded_buffer_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.guarded_buffer_index_pairs
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        ctx.int_range_facts
            .retain(|fact| fact.scope_id != loop_proof_scope_id);
        return Ok(());
    }

    let cond_idx = ctx.new_block("for.cond");
    let prebody_idx = if has_loop_prebody {
        Some(ctx.new_block("for.prebody"))
    } else {
        None
    };
    let body_idx = ctx.new_block("for.body");
    let update_idx = ctx.new_block("for.update");
    let backedge_cond_idx = if has_loop_prebody {
        Some(ctx.new_block("for.cond.backedge"))
    } else {
        None
    };
    let exit_idx = ctx.new_block("for.exit");

    let cond_label = ctx.block_label(cond_idx);
    let prebody_label = prebody_idx.map(|idx| ctx.block_label(idx));
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let backedge_cond_label = backedge_cond_idx.map(|idx| ctx.block_label(idx));
    let exit_label = ctx.block_label(exit_idx);
    let cond_true_label = prebody_label.as_ref().unwrap_or(&body_label);

    // Branch from the block holding the init into the cond block.
    ctx.block().br(&cond_label);

    // Cond block — fast i32 path when both counter and length are i32.
    ctx.current_block = cond_idx;
    let used_i32_cond = if let (Some((_, counter_id, op)), Some(ref len_i32_slot)) =
        (hoist_classification, &i32_length_slot)
    {
        // Existing path: `i < arr.length` / `i <= arr.length` with
        // hoisted i32 length.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let len = ctx.block().load(I32, len_i32_slot);
            let cmp = match op {
                perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &len),
                _ => ctx.block().icmp_slt(I32, &ctr, &len),
            };
            ctx.block().cond_br(&cmp, cond_true_label, &exit_label);
            true
        } else {
            false
        }
    } else if let (Some((counter_id, _, op)), Some(ref bound_i32_slot)) =
        (local_bound_classification, &i32_local_bound_slot)
    {
        // Issue #168: `i < n` / `i <= n` where `n` is a number-typed local
        // or parameter.  The fptosi(n) was hoisted above; use icmp i32.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let bound = ctx.block().load(I32, bound_i32_slot);
            let cmp = match op {
                perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &bound),
                _ => ctx.block().icmp_slt(I32, &ctr, &bound),
            };
            ctx.block().cond_br(&cmp, cond_true_label, &exit_label);
            true
        } else {
            false
        }
    } else {
        false
    };
    if !used_i32_cond {
        if let Some(cond_expr) = condition {
            let cv = lower_expr(ctx, cond_expr)?;
            let i1 = lower_truthy(ctx, &cv, cond_expr);
            ctx.block().cond_br(&i1, cond_true_label, &exit_label);
        } else {
            // `for (;;)` — unconditional jump into the body. May be an
            // infinite loop unless the body contains a `break`.
            ctx.block().br(cond_true_label);
        }
    }

    // Push break/continue targets so nested `break`/`continue` know where
    // to jump. For for-loops, continue runs the update step.
    ctx.loop_targets
        .push((update_label.clone(), exit_label.clone()));

    // If this for-loop has a pending label (from an enclosing Stmt::Labeled),
    // register it so `break label;` / `continue label;` resolve here.
    let consumed_label = ctx.pending_label.take();
    let previous_region_id = ctx.active_region_id.clone();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (update_label.clone(), exit_label.clone()));
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    let mut range_preguard_runtime: Option<((u32, u32), PreguardedNumericArrayIndexGet)> = None;
    let mut range_set_preguard_runtime: Option<((u32, u32), PreguardedNumericArrayIndexSet)> = None;
    let mut range_plain_set_preguard_runtime: Option<((u32, u32), PreguardedPlainArrayIndexSet)> =
        None;
    let mut affine_preguard_runtime: Vec<PreguardedNumericArrayAffineIndexGet> = Vec::new();
    let mut modulo_preguard_runtime: Vec<PreguardedNumericArrayModuloIndexGet> = Vec::new();
    let mut affine_set_preguard_runtime: Vec<PreguardedNumericArrayAffineIndexSet> = Vec::new();
    if let Some(pre_idx) = prebody_idx {
        ctx.current_block = pre_idx;
        if let (Some(hoist), Some(slot)) =
            (invariant_array_get_hoist, invariant_array_get_slot.as_ref())
        {
            let value = emit_invariant_numeric_array_index_get_hoist(
                ctx,
                hoist,
                i32_length_slot
                    .as_ref()
                    .expect("invariant array get hoist requires an i32 length slot"),
            )?;
            ctx.block().store(DOUBLE, &value, slot);
        }
        if let Some(preguard) = range_array_get_preguard {
            let info = emit_range_numeric_array_index_get_preguard(
                ctx,
                preguard,
                i32_length_slot
                    .as_ref()
                    .expect("range array get preguard requires an i32 length slot"),
            )?;
            range_preguard_runtime =
                Some(((preguard.array_local_id, preguard.index_local_id), info));
        }
        if let Some(preguard) = range_array_set_preguard {
            let info = emit_range_numeric_array_index_set_preguard(
                ctx,
                preguard,
                i32_length_slot
                    .as_ref()
                    .expect("range array set preguard requires an i32 length slot"),
            )?;
            range_set_preguard_runtime =
                Some(((preguard.array_local_id, preguard.index_local_id), info));
        }
        if let Some(preguard) = range_plain_array_set_preguard.clone() {
            let info = emit_range_plain_array_index_set_preguard(
                ctx,
                preguard.clone(),
                i32_length_slot.as_deref(),
                hoisted_length_slot.as_deref(),
            )?;
            range_plain_set_preguard_runtime =
                Some(((preguard.array_local_id, preguard.index_local_id), info));
        }
        if !affine_array_get_preguards.is_empty() {
            let bound_slot = i32_local_bound_slot
                .as_ref()
                .expect("affine array get preguard requires an i32 bound slot");
            let counter_id = local_bound_classification
                .map(|(counter_id, _, _)| counter_id)
                .expect("affine preguard requires local-bound counter");
            for preguard in affine_array_get_preguards.iter().cloned() {
                let info = emit_affine_numeric_array_index_get_preguard(
                    ctx, preguard, counter_id, bound_slot,
                )?;
                affine_preguard_runtime.push(info);
            }
        }
        if !modulo_array_get_preguards.is_empty() {
            let bound_slot = i32_local_bound_slot
                .as_ref()
                .expect("modulo array get preguard requires an i32 bound slot");
            let counter_id = local_bound_classification
                .map(|(counter_id, _, _)| counter_id)
                .expect("modulo preguard requires local-bound counter");
            for preguard in modulo_array_get_preguards.iter().cloned() {
                let info = emit_modulo_numeric_array_index_get_preguard(
                    ctx, preguard, counter_id, bound_slot,
                )?;
                modulo_preguard_runtime.push(info);
            }
        }
        if !affine_array_set_preguards.is_empty() {
            let bound_slot = i32_local_bound_slot
                .as_ref()
                .expect("affine array set preguard requires an i32 bound slot");
            let counter_id = local_bound_classification
                .map(|(counter_id, _, _)| counter_id)
                .expect("affine set preguard requires local-bound counter");
            for preguard in affine_array_set_preguards.iter().cloned() {
                let info = emit_affine_numeric_array_index_set_preguard(
                    ctx, preguard, counter_id, bound_slot,
                )?;
                affine_set_preguard_runtime.push(info);
            }
        }
        if !ctx.block().is_terminated() {
            ctx.block().br(&body_label);
        }
    }

    // Body block.
    ctx.current_block = body_idx;
    if let Some(cond) = condition {
        let mut guarded =
            crate::expr::guarded_buffer_indices_for_condition(ctx, cond, loop_proof_scope_id);
        guarded.retain(|fact| loop_counter_bounds_are_safe(ctx, fact.index_local_id, update, body));
        ctx.guarded_buffer_index_pairs.extend(guarded);
    }
    let hoisted_replacement = if let (Some(hoist), Some(slot)) =
        (invariant_array_get_hoist, invariant_array_get_slot.as_ref())
    {
        Some((
            (hoist.array_local_id, hoist.index_local_id),
            ctx.hoisted_array_index_gets
                .insert((hoist.array_local_id, hoist.index_local_id), slot.clone()),
        ))
    } else {
        None
    };
    let range_preguard_replacement = if let Some((key, preguard)) = range_preguard_runtime.as_ref()
    {
        Some((
            *key,
            ctx.preguarded_numeric_array_index_gets
                .insert(*key, preguard.clone()),
        ))
    } else {
        None
    };
    let range_set_preguard_replacement =
        if let Some((key, preguard)) = range_set_preguard_runtime.as_ref() {
            Some((
                *key,
                ctx.preguarded_numeric_array_index_sets
                    .insert(*key, preguard.clone()),
            ))
        } else {
            None
        };
    let range_plain_set_preguard_replacement =
        if let Some((key, preguard)) = range_plain_set_preguard_runtime.as_ref() {
            Some((
                *key,
                ctx.preguarded_plain_array_index_sets
                    .insert(*key, preguard.clone()),
            ))
        } else {
            None
        };
    let affine_preguard_base_len = ctx.preguarded_numeric_array_affine_index_gets.len();
    ctx.preguarded_numeric_array_affine_index_gets
        .extend(affine_preguard_runtime.iter().cloned());
    let modulo_preguard_base_len = ctx.preguarded_numeric_array_modulo_index_gets.len();
    ctx.preguarded_numeric_array_modulo_index_gets
        .extend(modulo_preguard_runtime.iter().cloned());
    let affine_set_preguard_base_len = ctx.preguarded_numeric_array_affine_index_sets.len();
    ctx.preguarded_numeric_array_affine_index_sets
        .extend(affine_set_preguard_runtime.iter().cloned());
    let previous_i64_loop_accumulator = i64_loop_accumulator.as_ref().map(|acc| {
        ctx.i64_loop_accumulator_slots
            .insert(acc.local_id, acc.slot.clone())
    });
    let lower_result = lower_stmts(ctx, body);
    if let Some(acc) = i64_loop_accumulator.as_ref() {
        if let Some(previous) = previous_i64_loop_accumulator.flatten() {
            ctx.i64_loop_accumulator_slots
                .insert(acc.local_id, previous);
        } else {
            ctx.i64_loop_accumulator_slots.remove(&acc.local_id);
        }
    }
    ctx.preguarded_numeric_array_affine_index_sets
        .truncate(affine_set_preguard_base_len);
    ctx.preguarded_numeric_array_modulo_index_gets
        .truncate(modulo_preguard_base_len);
    ctx.preguarded_numeric_array_affine_index_gets
        .truncate(affine_preguard_base_len);
    if let Some((key, previous)) = range_plain_set_preguard_replacement {
        if let Some(previous) = previous {
            ctx.preguarded_plain_array_index_sets.insert(key, previous);
        } else {
            ctx.preguarded_plain_array_index_sets.remove(&key);
        }
    }
    if let Some((key, previous)) = range_set_preguard_replacement {
        if let Some(previous) = previous {
            ctx.preguarded_numeric_array_index_sets
                .insert(key, previous);
        } else {
            ctx.preguarded_numeric_array_index_sets.remove(&key);
        }
    }
    if let Some((key, previous)) = range_preguard_replacement {
        if let Some(previous) = previous {
            ctx.preguarded_numeric_array_index_gets
                .insert(key, previous);
        } else {
            ctx.preguarded_numeric_array_index_gets.remove(&key);
        }
    }
    if let Some((key, previous)) = hoisted_replacement {
        if let Some(previous) = previous {
            ctx.hoisted_array_index_gets.insert(key, previous);
        } else {
            ctx.hoisted_array_index_gets.remove(&key);
        }
    }
    lower_result?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: insert an empty `asm sideeffect` in bodies whose
    // statements are all LLVM-pure (local-only arithmetic, no calls,
    // no heap mutation). Without this, clang -O3's loop-deletion
    // pass folds patterns like `for (let i=0;i<N;i++) sum+=1;` to
    // `sum=N` and eliminates the loop entirely — so two `Date.now()`
    // calls bracketing the loop end up adjacent in the binary and
    // report 0ms wall-clock. The barrier emits zero machine
    // instructions but is opaque to IndVarSimplify.
    // The i64 accumulator shadow makes loops like `sum = sum + 1` exact
    // enough for LLVM to derive the closed-form result and remove the loop.
    // Keep Date.now-bracketed benchmark loops observable, matching the
    // existing pure-loop preservation path.
    if !ctx.block().is_terminated()
        && (body_needs_asm_barrier(body) || i64_loop_accumulator.is_some())
    {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&update_label);
    }

    // Update block.
    ctx.current_block = update_idx;
    if let Some(update_expr) = update {
        let inserted_i32_only_update = i32_only_counter_update
            .map(|counter_id| ctx.i32_only_counter_updates.insert(counter_id));
        let previous_discard_expr_value = ctx.discard_expr_value;
        if i32_only_counter_update.is_some() {
            ctx.discard_expr_value = true;
        }
        let update_result = lower_expr(ctx, update_expr);
        ctx.discard_expr_value = previous_discard_expr_value;
        if let Some(counter_id) = i32_only_counter_update {
            if inserted_i32_only_update == Some(true) {
                ctx.i32_only_counter_updates.remove(&counter_id);
            }
        }
        let _ = update_result?;
    }
    if !ctx.block().is_terminated() {
        ctx.block()
            .br(backedge_cond_label.as_ref().unwrap_or(&cond_label));
    }

    if let Some(backedge_idx) = backedge_cond_idx {
        ctx.current_block = backedge_idx;
        if let (Some((_, counter_id, op)), Some(ref len_i32_slot)) =
            (hoist_classification, &i32_length_slot)
        {
            if !emit_i32_length_loop_condition(
                ctx,
                counter_id,
                op,
                len_i32_slot,
                &body_label,
                &exit_label,
            ) {
                return Err(anyhow::anyhow!(
                    "loop prebody missing backedge i32 condition"
                ));
            }
        } else if let (Some((counter_id, _, op)), Some(ref bound_i32_slot)) =
            (local_bound_classification, &i32_local_bound_slot)
        {
            if !emit_i32_length_loop_condition(
                ctx,
                counter_id,
                op,
                bound_i32_slot,
                &body_label,
                &exit_label,
            ) {
                return Err(anyhow::anyhow!(
                    "loop prebody missing local-bound backedge i32 condition"
                ));
            }
        } else if let Some(cond_expr) = condition {
            let cv = lower_expr(ctx, cond_expr)?;
            let i1 = lower_truthy(ctx, &cv, cond_expr);
            ctx.block().cond_br(&i1, &body_label, &exit_label);
        } else {
            ctx.block().br(&body_label);
        }
    }
    // Exit block — subsequent statements continue here. Sync before popping
    // loop-local i32 slots so after-loop reads observe the final counter.
    ctx.current_block = exit_idx;
    if let Some(counter_id) = i32_only_counter_update {
        sync_i32_counter_to_double_slot(ctx, counter_id);
    }
    if let Some(acc) = i64_loop_accumulator.as_ref() {
        sync_i64_accumulator_to_double_slot(ctx, acc);
    }
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();

    // Pop the hoisted-length entry so nested loops or sibling loops
    // don't see a stale slot.
    if let Some((_, counter_id, _op)) = hoist_classification {
        ctx.i32_counter_slots.remove(&counter_id);
    }
    if let Some(arr_id) = hoisted_length_arr_id {
        ctx.cached_lengths.remove(&arr_id);
    }
    let _ = hoisted_length_slot;
    // Pop the i32 counter slot we inserted for the `i < n` number-bound
    // path, but only if *we* were the ones that inserted it (the Let site
    // may have already provided a slot, which should outlive the loop).
    if local_bound_counter_i32_was_fresh {
        if let Some((counter_id, _, _)) = local_bound_classification {
            ctx.i32_counter_slots.remove(&counter_id);
        }
    }
    if local_bound_bound_i32_was_fresh {
        if let Some((_, bound_id, _)) = local_bound_classification {
            ctx.i32_counter_slots.remove(&bound_id);
        }
    }
    let _ = i32_local_bound_slot;
    ctx.bounded_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.bounded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.int_range_facts
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    Ok(())
}

fn classify_i32_only_counter_update(
    ctx: &FnCtx<'_>,
    init: Option<&Stmt>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<u32> {
    let perry_hir::Expr::Update { id, .. } = update? else {
        return None;
    };
    if !matches!(init, Some(Stmt::Let { id: init_id, mutable: true, .. }) if init_id == id) {
        return None;
    }
    if let_init_is_negative_zero(init, *id) {
        return None;
    }
    if ctx.boxed_vars.contains(id)
        || ctx.module_globals.contains_key(id)
        || ctx.closure_captures.contains_key(id)
        || ctx.unsigned_i32_locals.contains(id)
        || !ctx.i32_counter_slots.contains_key(id)
        || !loop_counter_bounds_are_safe(ctx, *id, update, body)
    {
        return None;
    }
    Some(*id)
}

fn let_init_is_negative_zero(init: Option<&Stmt>, local_id: u32) -> bool {
    matches!(
        init,
        Some(Stmt::Let {
            id,
            init: Some(perry_hir::Expr::Number(n)),
            ..
        }) if *id == local_id && *n == 0.0 && n.is_sign_negative()
    )
}

fn sync_i32_counter_to_double_slot(ctx: &mut FnCtx<'_>, counter_id: u32) {
    let (Some(i32_slot), Some(double_slot)) = (
        ctx.i32_counter_slots.get(&counter_id).cloned(),
        ctx.locals.get(&counter_id).cloned(),
    ) else {
        return;
    };
    let i32_value = ctx.block().load(I32, &i32_slot);
    let double_value = ctx.block().sitofp(I32, &i32_value, DOUBLE);
    ctx.block().store(DOUBLE, &double_value, &double_slot);
}

fn prepare_i64_loop_accumulator(
    ctx: &mut FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<I64LoopAccumulator> {
    let local_id = classify_i64_loop_accumulator(ctx, local_bound_classification, update, body)?;
    let initial = *ctx.exact_safe_integer_locals.get(&local_id)?;
    let slot = ctx.func.alloca_entry(I64);
    ctx.block().store(I64, &initial.to_string(), &slot);
    Some(I64LoopAccumulator { local_id, slot })
}

fn classify_i64_loop_accumulator(
    ctx: &FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<u32> {
    let (counter_id, bound_id, op) = local_bound_classification?;
    if !matches!(op, perry_hir::CompareOp::Lt)
        || !ctx.i32_counter_slots.contains_key(&counter_id)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    {
        return None;
    }

    let [Stmt::Expr(perry_hir::Expr::LocalSet(acc_id, value))] = body else {
        return None;
    };
    if *acc_id == counter_id
        || ctx.i32_counter_slots.contains_key(acc_id)
        || ctx.boxed_vars.contains(acc_id)
        || ctx.module_globals.contains_key(acc_id)
        || ctx.closure_captures.contains_key(acc_id)
        || ctx.buffer_view_slots.contains_key(acc_id)
        || !ctx.locals.contains_key(acc_id)
    {
        return None;
    }

    let initial = *ctx.exact_safe_integer_locals.get(acc_id)?;
    if initial < 0 {
        return None;
    }
    let addend = self_add_accumulator_addend(*acc_id, value.as_ref())?;
    let addend_max = nonnegative_i64_addend_max(ctx, counter_id, addend)?;
    let start = *ctx.exact_safe_integer_locals.get(&counter_id)?;
    if start < 0 {
        return None;
    }
    let bound_range = crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(bound_id))?;
    let iterations = bound_range.max.checked_sub(start)?.max(0);
    let growth = iterations.checked_mul(addend_max)?;
    let max_value = initial.checked_add(growth)?;
    (max_value <= MAX_SAFE_INTEGER_I64).then_some(*acc_id)
}

fn self_add_accumulator_addend<'a>(
    acc_id: u32,
    value: &'a perry_hir::Expr,
) -> Option<&'a perry_hir::Expr> {
    let perry_hir::Expr::Binary {
        op: perry_hir::BinaryOp::Add,
        left,
        right,
    } = value
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (perry_hir::Expr::LocalGet(left_id), addend) if *left_id == acc_id => Some(addend),
        (addend, perry_hir::Expr::LocalGet(right_id)) if *right_id == acc_id => Some(addend),
        _ => None,
    }
}

fn exact_nonnegative_integer_const(expr: &perry_hir::Expr) -> Option<i64> {
    let value = match expr {
        perry_hir::Expr::Integer(n) => *n,
        perry_hir::Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => {
            if *n == 0.0 && n.is_sign_negative() {
                return None;
            }
            let value = *n as i64;
            if (value as f64) != *n {
                return None;
            }
            value
        }
        _ => return None,
    };
    (0..=MAX_SAFE_INTEGER_I64).contains(&value).then_some(value)
}

fn bounded_byte_get_addend_max(
    ctx: &FnCtx<'_>,
    counter_id: u32,
    buffer: &perry_hir::Expr,
    index: &perry_hir::Expr,
) -> Option<i64> {
    let perry_hir::Expr::LocalGet(index_id) = index else {
        return None;
    };
    if *index_id != counter_id {
        return None;
    }
    let perry_hir::Expr::LocalGet(buffer_id) = buffer else {
        return None;
    };
    if !ctx.buffer_view_slots.contains_key(buffer_id) {
        return None;
    }
    crate::expr::bounds_for_buffer_access_width(ctx, *buffer_id, index, 1)
        .allows_inbounds()
        .then_some(255)
}

fn nonnegative_i64_addend_max(
    ctx: &FnCtx<'_>,
    counter_id: u32,
    expr: &perry_hir::Expr,
) -> Option<i64> {
    if let Some(value) = exact_nonnegative_integer_const(expr) {
        return Some(value);
    }
    match expr {
        perry_hir::Expr::LocalGet(id)
            if *id == counter_id
                && crate::expr::int_range_expr(ctx, expr).is_some_and(|range| range.min >= 0) =>
        {
            crate::expr::int_range_expr(ctx, expr).map(|range| range.max)
        }
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Mod,
            left,
            right,
        } => {
            let perry_hir::Expr::LocalGet(id) = left.as_ref() else {
                return None;
            };
            if *id != counter_id {
                return None;
            }
            let divisor = exact_nonnegative_integer_const(right.as_ref())?;
            (divisor > 0).then_some(divisor - 1)
        }
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => {
            let left = nonnegative_i64_addend_max(ctx, counter_id, left)?;
            let right = nonnegative_i64_addend_max(ctx, counter_id, right)?;
            left.checked_add(right)
        }
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Mul,
            left,
            right,
        } => {
            let left = nonnegative_i64_addend_max(ctx, counter_id, left)?;
            let right = nonnegative_i64_addend_max(ctx, counter_id, right)?;
            left.checked_mul(right)
        }
        perry_hir::Expr::Uint8ArrayGet { array, index } => {
            bounded_byte_get_addend_max(ctx, counter_id, array, index)
        }
        perry_hir::Expr::BufferIndexGet { buffer, index } => {
            bounded_byte_get_addend_max(ctx, counter_id, buffer, index)
        }
        _ => None,
    }
}

fn sync_i64_accumulator_to_double_slot(ctx: &mut FnCtx<'_>, acc: &I64LoopAccumulator) {
    let Some(double_slot) = ctx.locals.get(&acc.local_id).cloned() else {
        return;
    };
    let i64_value = ctx.block().load(I64, &acc.slot);
    let double_value = ctx.block().sitofp(I64, &i64_value, DOUBLE);
    ctx.block().store(DOUBLE, &double_value, &double_slot);
    ctx.exact_safe_integer_locals.remove(&acc.local_id);
}

fn classify_modulo_accumulator_closed_form(
    ctx: &FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    acc: &I64LoopAccumulator,
) -> Option<AccumulatorClosedForm> {
    let (counter_id, bound_id, op) = local_bound_classification?;
    if !matches!(op, perry_hir::CompareOp::Lt)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    {
        return None;
    }

    let [Stmt::Expr(perry_hir::Expr::LocalSet(acc_id, value))] = body else {
        return None;
    };
    if *acc_id != acc.local_id {
        return None;
    }
    let modulus = modulo_accumulator_addend(acc.local_id, counter_id, value.as_ref())?;
    let start = *ctx.exact_safe_integer_locals.get(&counter_id)?;
    let bound = *ctx.exact_safe_integer_locals.get(&bound_id)?;
    let initial_acc = *ctx.exact_safe_integer_locals.get(&acc.local_id)?;
    if start < 0 || bound < 0 || initial_acc < 0 {
        return None;
    }
    let final_counter = if start < bound { bound } else { start };
    i32::try_from(final_counter).ok()?;

    let delta = if start < bound {
        let end_sum = modulo_prefix_sum(bound as i128, modulus as i128)?;
        let start_sum = modulo_prefix_sum(start as i128, modulus as i128)?;
        end_sum.checked_sub(start_sum)?
    } else {
        0
    };
    let final_acc = (initial_acc as i128).checked_add(delta)?;
    if final_acc < 0 || final_acc > MAX_SAFE_INTEGER_I64 as i128 {
        return None;
    }

    Some(AccumulatorClosedForm {
        acc_local_id: acc.local_id,
        counter_local_id: counter_id,
        final_counter,
        final_acc: final_acc as i64,
    })
}

fn classify_constant_accumulator_closed_form(
    ctx: &FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    acc: &I64LoopAccumulator,
) -> Option<AccumulatorClosedForm> {
    let (counter_id, bound_id, op) = local_bound_classification?;
    if !matches!(op, perry_hir::CompareOp::Lt)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    {
        return None;
    }

    let [Stmt::Expr(perry_hir::Expr::LocalSet(acc_id, value))] = body else {
        return None;
    };
    if *acc_id != acc.local_id {
        return None;
    }
    let addend = exact_nonnegative_integer_const(self_add_accumulator_addend(
        acc.local_id,
        value.as_ref(),
    )?)?;
    let start = *ctx.exact_safe_integer_locals.get(&counter_id)?;
    let bound = *ctx.exact_safe_integer_locals.get(&bound_id)?;
    let initial_acc = *ctx.exact_safe_integer_locals.get(&acc.local_id)?;
    if start < 0 || bound < 0 || initial_acc < 0 {
        return None;
    }
    let final_counter = if start < bound { bound } else { start };
    i32::try_from(final_counter).ok()?;

    let iterations = if start < bound {
        (bound as i128).checked_sub(start as i128)?
    } else {
        0
    };
    let delta = iterations.checked_mul(addend as i128)?;
    let final_acc = (initial_acc as i128).checked_add(delta)?;
    if final_acc < 0 || final_acc > MAX_SAFE_INTEGER_I64 as i128 {
        return None;
    }

    Some(AccumulatorClosedForm {
        acc_local_id: acc.local_id,
        counter_local_id: counter_id,
        final_counter,
        final_acc: final_acc as i64,
    })
}

fn classify_affine_accumulator_closed_form(
    ctx: &FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
    acc: &I64LoopAccumulator,
) -> Option<AccumulatorClosedForm> {
    let (counter_id, bound_id, op) = local_bound_classification?;
    if !matches!(op, perry_hir::CompareOp::Lt)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    {
        return None;
    }

    let [Stmt::Expr(perry_hir::Expr::LocalSet(acc_id, value))] = body else {
        return None;
    };
    if *acc_id != acc.local_id {
        return None;
    }
    let addend = affine_accumulator_addend(
        counter_id,
        self_add_accumulator_addend(acc.local_id, value.as_ref())?,
    )?;
    let start = *ctx.exact_safe_integer_locals.get(&counter_id)?;
    let bound = *ctx.exact_safe_integer_locals.get(&bound_id)?;
    let initial_acc = *ctx.exact_safe_integer_locals.get(&acc.local_id)?;
    if start < 0 || bound < 0 || initial_acc < 0 {
        return None;
    }
    let final_counter = if start < bound { bound } else { start };
    i32::try_from(final_counter).ok()?;

    let iterations = if start < bound {
        (bound as i128).checked_sub(start as i128)?
    } else {
        0
    };
    let counter_sum = arithmetic_series_sum(start as i128, final_counter as i128)?;
    let delta = addend
        .coeff
        .checked_mul(counter_sum)?
        .checked_add(addend.constant.checked_mul(iterations)?)?;
    let final_acc = (initial_acc as i128).checked_add(delta)?;
    if final_acc < 0 || final_acc > MAX_SAFE_INTEGER_I64 as i128 {
        return None;
    }

    Some(AccumulatorClosedForm {
        acc_local_id: acc.local_id,
        counter_local_id: counter_id,
        final_counter,
        final_acc: final_acc as i64,
    })
}

fn affine_accumulator_addend(
    counter_id: u32,
    expr: &perry_hir::Expr,
) -> Option<AffineAccumulatorAddend> {
    if let Some(value) = exact_nonnegative_integer_const(expr) {
        return Some(AffineAccumulatorAddend {
            coeff: 0,
            constant: value as i128,
        });
    }

    match expr {
        perry_hir::Expr::LocalGet(id) if *id == counter_id => Some(AffineAccumulatorAddend {
            coeff: 1,
            constant: 0,
        }),
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Add,
            left,
            right,
        } => {
            let left = affine_accumulator_addend(counter_id, left)?;
            let right = affine_accumulator_addend(counter_id, right)?;
            Some(AffineAccumulatorAddend {
                coeff: left.coeff.checked_add(right.coeff)?,
                constant: left.constant.checked_add(right.constant)?,
            })
        }
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::Mul,
            left,
            right,
        } => {
            let left = affine_accumulator_addend(counter_id, left)?;
            let right = affine_accumulator_addend(counter_id, right)?;
            match (left.coeff == 0, right.coeff == 0) {
                (true, true) => left.scale(right.constant),
                (true, false) => right.scale(left.constant),
                (false, true) => left.scale(right.constant),
                (false, false) => None,
            }
        }
        _ => None,
    }
}

impl AffineAccumulatorAddend {
    fn scale(self, factor: i128) -> Option<Self> {
        Some(Self {
            coeff: self.coeff.checked_mul(factor)?,
            constant: self.constant.checked_mul(factor)?,
        })
    }
}

fn arithmetic_series_sum(start: i128, end_exclusive: i128) -> Option<i128> {
    if start >= end_exclusive {
        return Some(0);
    }
    let terms = end_exclusive.checked_sub(start)?;
    let last = end_exclusive.checked_sub(1)?;
    start.checked_add(last)?.checked_mul(terms)?.checked_div(2)
}

fn modulo_accumulator_addend(acc_id: u32, counter_id: u32, value: &perry_hir::Expr) -> Option<i64> {
    let addend = self_add_accumulator_addend(acc_id, value)?;
    let perry_hir::Expr::Binary {
        op: perry_hir::BinaryOp::Mod,
        left,
        right,
    } = addend
    else {
        return None;
    };
    let perry_hir::Expr::LocalGet(id) = left.as_ref() else {
        return None;
    };
    if *id != counter_id {
        return None;
    }
    let modulus = exact_nonnegative_integer_const(right.as_ref())?;
    (modulus > 0).then_some(modulus)
}

fn modulo_prefix_sum(n: i128, modulus: i128) -> Option<i128> {
    if n < 0 || modulus <= 0 {
        return None;
    }
    let period_sum = modulus
        .checked_mul(modulus.checked_sub(1)?)?
        .checked_div(2)?;
    let full_periods = n.checked_div(modulus)?;
    let remainder = n.checked_rem(modulus)?;
    let full_sum = full_periods.checked_mul(period_sum)?;
    let tail_sum = remainder
        .checked_mul(remainder.checked_sub(1)?)?
        .checked_div(2)?;
    full_sum.checked_add(tail_sum)
}

fn emit_accumulator_closed_form(
    ctx: &mut FnCtx<'_>,
    acc: &I64LoopAccumulator,
    closed_form: AccumulatorClosedForm,
) {
    ctx.block()
        .store(I64, &closed_form.final_acc.to_string(), &acc.slot);
    sync_i64_accumulator_to_double_slot(ctx, acc);
    ctx.exact_safe_integer_locals
        .insert(closed_form.acc_local_id, closed_form.final_acc);
    ctx.nonnegative_integer_locals
        .insert(closed_form.acc_local_id);

    if let Some(i32_slot) = ctx
        .i32_counter_slots
        .get(&closed_form.counter_local_id)
        .cloned()
    {
        ctx.block()
            .store(I32, &closed_form.final_counter.to_string(), &i32_slot);
    }
    if let Some(double_slot) = ctx.locals.get(&closed_form.counter_local_id).cloned() {
        let final_counter = crate::nanbox::double_literal(closed_form.final_counter as f64);
        ctx.block().store(DOUBLE, &final_counter, &double_slot);
    }
    ctx.exact_safe_integer_locals
        .insert(closed_form.counter_local_id, closed_form.final_counter);
    ctx.nonnegative_integer_locals
        .insert(closed_form.counter_local_id);
}

fn classify_direct_field_accumulator_closed_form(
    ctx: &FnCtx<'_>,
    local_bound_classification: Option<(u32, u32, perry_hir::CompareOp)>,
    update: Option<&perry_hir::Expr>,
    body: &[Stmt],
) -> Option<DirectFieldAccumulatorClosedForm> {
    let (counter_id, bound_id, op) = local_bound_classification?;
    if !matches!(op, perry_hir::CompareOp::Lt)
        || !ctx.i32_counter_slots.contains_key(&counter_id)
        || !loop_counter_bounds_are_safe(ctx, counter_id, update, body)
    {
        return None;
    }

    let [Stmt::Expr(perry_hir::Expr::PropertySet {
        object,
        property,
        value,
    })] = body
    else {
        return None;
    };
    let perry_hir::Expr::LocalGet(object_local_id) = object.as_ref() else {
        return None;
    };
    if *object_local_id == counter_id {
        return None;
    }
    let class_name = ctx.const_new_class_locals.get(object_local_id)?;
    if !ctx
        .direct_field_new_locals
        .get(object_local_id)
        .is_some_and(|fields| fields.contains(property))
    {
        return None;
    }
    if !crate::type_analysis::class_field_declared_type(ctx, class_name, property)
        .as_ref()
        .is_some_and(crate::typed_shape::type_is_raw_f64_candidate)
    {
        return None;
    }
    let field_index = crate::type_analysis::class_field_global_index(ctx, class_name, property)?;
    let initial_field = ctx
        .exact_safe_integer_class_fields
        .get(&(*object_local_id, property.clone()))
        .copied()?;
    let addend = affine_accumulator_addend(
        counter_id,
        self_add_class_field_addend(*object_local_id, property, value.as_ref())?,
    )?;
    let start = *ctx.exact_safe_integer_locals.get(&counter_id)?;
    let bound = *ctx.exact_safe_integer_locals.get(&bound_id)?;
    if start < 0 || bound < 0 || initial_field < 0 {
        return None;
    }
    let final_counter = if start < bound { bound } else { start };
    i32::try_from(final_counter).ok()?;

    let iterations = if start < bound {
        (bound as i128).checked_sub(start as i128)?
    } else {
        0
    };
    let counter_sum = arithmetic_series_sum(start as i128, final_counter as i128)?;
    let delta = addend
        .coeff
        .checked_mul(counter_sum)?
        .checked_add(addend.constant.checked_mul(iterations)?)?;
    let final_field = (initial_field as i128).checked_add(delta)?;
    if final_field < 0 || final_field > MAX_SAFE_INTEGER_I64 as i128 {
        return None;
    }

    Some(DirectFieldAccumulatorClosedForm {
        object_local_id: *object_local_id,
        field: property.clone(),
        field_index,
        counter_local_id: counter_id,
        final_counter,
        final_field: final_field as i64,
    })
}

fn self_add_class_field_addend<'a>(
    object_local_id: u32,
    field: &str,
    value: &'a perry_hir::Expr,
) -> Option<&'a perry_hir::Expr> {
    let perry_hir::Expr::Binary {
        op: perry_hir::BinaryOp::Add,
        left,
        right,
    } = value
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (perry_hir::Expr::PropertyGet { object, property }, addend)
            if property == field
                && matches!(object.as_ref(), perry_hir::Expr::LocalGet(id) if *id == object_local_id) =>
        {
            Some(addend)
        }
        (addend, perry_hir::Expr::PropertyGet { object, property })
            if property == field
                && matches!(object.as_ref(), perry_hir::Expr::LocalGet(id) if *id == object_local_id) =>
        {
            Some(addend)
        }
        _ => None,
    }
}

fn emit_direct_field_accumulator_closed_form(
    ctx: &mut FnCtx<'_>,
    closed_form: DirectFieldAccumulatorClosedForm,
) -> Result<()> {
    let object_expr = perry_hir::Expr::LocalGet(closed_form.object_local_id);
    let recv_box = lower_expr(ctx, &object_expr)?;
    let field_idx_str = closed_form.field_index.to_string();
    let final_field = crate::nanbox::double_literal(closed_form.final_field as f64);
    let blk = ctx.block();
    let obj_bits = blk.bitcast_double_to_i64(&recv_box);
    let obj_handle = blk.and(I64, &obj_bits, POINTER_MASK_I64);
    let obj_ptr = blk.inttoptr(I64, &obj_handle);
    let fields_base = blk.gep(I8, &obj_ptr, &[(I64, "24")]);
    let field_ptr = blk.gep(DOUBLE, &fields_base, &[(I64, &field_idx_str)]);
    blk.store(DOUBLE, &final_field, &field_ptr);

    ctx.exact_safe_integer_class_fields.insert(
        (closed_form.object_local_id, closed_form.field),
        closed_form.final_field,
    );

    if let Some(i32_slot) = ctx
        .i32_counter_slots
        .get(&closed_form.counter_local_id)
        .cloned()
    {
        ctx.block()
            .store(I32, &closed_form.final_counter.to_string(), &i32_slot);
    }
    if let Some(double_slot) = ctx.locals.get(&closed_form.counter_local_id).cloned() {
        let final_counter = crate::nanbox::double_literal(closed_form.final_counter as f64);
        ctx.block().store(DOUBLE, &final_counter, &double_slot);
    }
    ctx.exact_safe_integer_locals
        .insert(closed_form.counter_local_id, closed_form.final_counter);
    ctx.nonnegative_integer_locals
        .insert(closed_form.counter_local_id);
    Ok(())
}

pub(crate) fn clear_loop_body_shadow_slots(ctx: &mut FnCtx<'_>, body: &[Stmt]) {
    if ctx.block().is_terminated() || ctx.shadow_slot_map.is_empty() {
        return;
    }
    let slots =
        crate::collectors::collect_declared_shadow_slots_in_stmts(body, &ctx.shadow_slot_map);
    if slots.is_empty() {
        return;
    }
    emit_shadow_slot_clears(ctx, &slots);
}

fn emit_invariant_numeric_array_index_get_hoist(
    ctx: &mut FnCtx<'_>,
    hoist: InvariantArrayIndexGetHoist,
    len_i32_slot: &str,
) -> Result<String> {
    let arr_expr = perry_hir::Expr::LocalGet(hoist.array_local_id);
    let idx_expr = perry_hir::Expr::LocalGet(hoist.index_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let idx_i32 =
        if let Some(idx_i32_slot) = ctx.i32_counter_slots.get(&hoist.index_local_id).cloned() {
            ctx.block().load(I32, &idx_i32_slot)
        } else {
            let idx_double = lower_expr(ctx, &idx_expr)?;
            ctx.block().fptosi(DOUBLE, &idx_double, I32)
        };
    let inner_counter_slot = ctx
        .i32_counter_slots
        .get(&hoist.inner_counter_local_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("invariant array get hoist missing inner i32 counter"))?;
    let len_i32 = ctx.block().load(I32, len_i32_slot);
    let inner_i32 = ctx.block().load(I32, &inner_counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &len_i32, &inner_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);

    lower_guarded_array_index_get_trusted_i32(
        ctx,
        &arr_box,
        &idx_i32,
        "hoist.num",
        Some(&skipped_i64),
    )
}

fn emit_range_numeric_array_index_get_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: RangeNumericArrayIndexGetPreguard,
    len_i32_slot: &str,
) -> Result<PreguardedNumericArrayIndexGet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_slot = ctx
        .i32_counter_slots
        .get(&preguard.index_local_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("range array get preguard missing i32 counter"))?;
    let len_i32 = ctx.block().load(I32, len_i32_slot);
    let counter_i32 = ctx.block().load(I32, &counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &len_i32, &counter_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);
    let last_i32 = ctx.block().sub(I32, &len_i32, "1");
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]",
        TypedFeedbackContract::numeric_array_get_index(),
    );
    let guard_result = ctx.block().call(
        I32,
        "js_typed_feedback_numeric_array_index_get_guard_i32",
        &[
            (I64, &site_id),
            (DOUBLE, &arr_box),
            (I32, &last_i32),
            (I32, "1"),
        ],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);
    let guard_ok = ctx.block().icmp_ne(I32, &guard_result, "0");

    let fast_idx = ctx.new_block("range_preguard.fast");
    let done_idx = ctx.new_block("range_preguard.done");
    let fast_label = ctx.block_label(fast_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&guard_ok, &fast_label, &done_label);

    ctx.current_block = fast_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_array_guard_fast_passes",
        &[(I64, &site_id), (I64, &skipped_i64)],
    );
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(PreguardedNumericArrayIndexGet {
        site_id,
        guard_ok_slot,
    })
}

fn emit_range_numeric_array_index_set_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: RangeNumericArrayIndexSetPreguard,
    len_i32_slot: &str,
) -> Result<PreguardedNumericArrayIndexSet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_slot = ctx
        .i32_counter_slots
        .get(&preguard.index_local_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("range array set preguard missing i32 counter"))?;
    let len_i32 = ctx.block().load(I32, len_i32_slot);
    let counter_i32 = ctx.block().load(I32, &counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &len_i32, &counter_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);
    let last_i32 = ctx.block().sub(I32, &len_i32, "1");
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]=",
        TypedFeedbackContract::numeric_array_set_index(),
    );
    let guard_result = ctx.block().call(
        I32,
        "js_typed_feedback_numeric_array_index_set_guard",
        &[
            (I64, &site_id),
            (DOUBLE, &arr_box),
            (I32, &last_i32),
            (DOUBLE, "0.0"),
            (I32, "1"),
        ],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);
    let guard_ok = ctx.block().icmp_ne(I32, &guard_result, "0");

    let fast_idx = ctx.new_block("range_set_preguard.fast");
    let done_idx = ctx.new_block("range_set_preguard.done");
    let fast_label = ctx.block_label(fast_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&guard_ok, &fast_label, &done_label);

    ctx.current_block = fast_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_array_guard_fast_passes",
        &[(I64, &site_id), (I64, &skipped_i64)],
    );
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(PreguardedNumericArrayIndexSet {
        site_id,
        guard_ok_slot,
    })
}

fn emit_range_plain_array_index_set_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: RangePlainArrayIndexSetPreguard,
    len_i32_slot: Option<&str>,
    len_dbl_slot: Option<&str>,
) -> Result<PreguardedPlainArrayIndexSet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_expr = perry_hir::Expr::LocalGet(preguard.index_local_id);
    let counter_i32 =
        if let Some(counter_slot) = ctx.i32_counter_slots.get(&preguard.index_local_id).cloned() {
            ctx.block().load(I32, &counter_slot)
        } else {
            let counter_box = lower_expr(ctx, &counter_expr)?;
            ctx.block().fptosi(DOUBLE, &counter_box, I32)
        };
    let max_i32 = match preguard.max_index {
        PlainArraySetMaxIndex::ArrayLength => {
            let len_i32 = if let Some(slot) = len_i32_slot {
                ctx.block().load(I32, slot)
            } else {
                let len_dbl_slot = len_dbl_slot.ok_or_else(|| {
                    anyhow::anyhow!("plain array set preguard missing hoisted length")
                })?;
                let len_dbl = ctx.block().load(DOUBLE, len_dbl_slot);
                ctx.block().fptosi(DOUBLE, &len_dbl, I32)
            };
            ctx.block().sub(I32, &len_i32, "1")
        }
        PlainArraySetMaxIndex::BoundExpr(expr) => {
            let bound_box = lower_expr(ctx, &expr)?;
            let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_box, I32);
            ctx.block().sub(I32, &bound_i32, "1")
        }
    };
    let guard_result = ctx.block().call(
        I32,
        "js_plain_array_inbounds_range_guard",
        &[(DOUBLE, &arr_box), (I32, &counter_i32), (I32, &max_i32)],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);

    let pointer_free_result = ctx.block().call(
        I32,
        "js_plain_array_inbounds_pointer_free_range_guard",
        &[(DOUBLE, &arr_box), (I32, &counter_i32), (I32, &max_i32)],
    );
    let pointer_free_range_slot = ctx.func.alloca_entry(I32);
    ctx.block()
        .store(I32, &pointer_free_result, &pointer_free_range_slot);

    let f64_numeric_range_slot = if preguard.f64_numeric_update {
        let numeric_result = ctx.block().call(
            I32,
            "js_plain_array_f64_number_range_guard",
            &[(DOUBLE, &arr_box), (I32, &counter_i32), (I32, &max_i32)],
        );
        let slot = ctx.func.alloca_entry(I32);
        ctx.block().store(I32, &numeric_result, &slot);
        Some(slot)
    } else {
        None
    };

    Ok(PreguardedPlainArrayIndexSet {
        guard_ok_slot,
        pointer_free_range_slot: Some(pointer_free_range_slot),
        f64_numeric_range_slot,
    })
}

fn emit_affine_numeric_array_index_get_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: AffineNumericArrayIndexGetPreguard,
    counter_id: u32,
    bound_i32_slot: &str,
) -> Result<PreguardedNumericArrayAffineIndexGet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_slot = ctx
        .i32_counter_slots
        .get(&counter_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("affine array get preguard missing i32 counter"))?;
    let bound_i32 = ctx.block().load(I32, bound_i32_slot);
    let counter_i32 = ctx.block().load(I32, &counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &bound_i32, &counter_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);
    let last_counter_i32 = ctx.block().sub(I32, &bound_i32, "1");

    let max_i32 = match &preguard.index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
            mul_local_id,
            bound_local_id,
            ..
        } => {
            let mul_slot = ctx
                .i32_counter_slots
                .get(mul_local_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("affine array get preguard missing i32 multiplier")
                })?;
            let bound_slot = ctx
                .i32_counter_slots
                .get(bound_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array get preguard missing i32 bound"))?;
            let mul = ctx.block().load(I32, &mul_slot);
            let bound = ctx.block().load(I32, &bound_slot);
            let base = ctx.block().mul(I32, &mul, &bound);
            ctx.block().add(I32, &base, &last_counter_i32)
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
            bound_local_id,
            add_local_id,
            ..
        } => {
            let bound_slot = ctx
                .i32_counter_slots
                .get(bound_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array get preguard missing i32 bound"))?;
            let add_slot = ctx
                .i32_counter_slots
                .get(add_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array get preguard missing i32 addend"))?;
            let bound = ctx.block().load(I32, &bound_slot);
            let add = ctx.block().load(I32, &add_slot);
            let base = ctx.block().mul(I32, &last_counter_i32, &bound);
            ctx.block().add(I32, &base, &add)
        }
    };
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]",
        TypedFeedbackContract::numeric_array_get_index(),
    );
    let guard_result = ctx.block().call(
        I32,
        "js_typed_feedback_numeric_array_index_get_guard_i32",
        &[
            (I64, &site_id),
            (DOUBLE, &arr_box),
            (I32, &max_i32),
            (I32, "1"),
        ],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);
    let guard_ok = ctx.block().icmp_ne(I32, &guard_result, "0");

    let fast_idx = ctx.new_block("affine_preguard.fast");
    let done_idx = ctx.new_block("affine_preguard.done");
    let fast_label = ctx.block_label(fast_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&guard_ok, &fast_label, &done_label);

    ctx.current_block = fast_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_array_guard_fast_passes",
        &[(I64, &site_id), (I64, &skipped_i64)],
    );
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(PreguardedNumericArrayAffineIndexGet {
        array_local_id: preguard.array_local_id,
        index: preguard.index,
        site_id,
        guard_ok_slot,
    })
}

fn emit_modulo_numeric_array_index_get_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: ModuloNumericArrayIndexGetPreguard,
    counter_id: u32,
    bound_i32_slot: &str,
) -> Result<PreguardedNumericArrayModuloIndexGet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_slot = ctx
        .i32_counter_slots
        .get(&counter_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("modulo array get preguard missing i32 counter"))?;
    let bound_i32 = ctx.block().load(I32, bound_i32_slot);
    let counter_i32 = ctx.block().load(I32, &counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &bound_i32, &counter_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);

    let modulus_local_id = match &preguard.index {
        PreguardedModuloIndexExpr::LocalModuloLocal {
            modulus_local_id, ..
        }
        | PreguardedModuloIndexExpr::LocalTimesConstModuloLocal {
            modulus_local_id, ..
        } => *modulus_local_id,
    };
    let modulus_slot = ctx
        .i32_counter_slots
        .get(&modulus_local_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("modulo array get preguard missing i32 modulus"))?;
    let modulus_i32 = ctx.block().load(I32, &modulus_slot);
    let max_i32 = ctx.block().sub(I32, &modulus_i32, "1");

    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]",
        TypedFeedbackContract::numeric_array_get_index(),
    );
    let guard_result = ctx.block().call(
        I32,
        "js_typed_feedback_numeric_array_index_get_guard_i32",
        &[
            (I64, &site_id),
            (DOUBLE, &arr_box),
            (I32, &max_i32),
            (I32, "1"),
        ],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);
    let guard_ok = ctx.block().icmp_ne(I32, &guard_result, "0");

    let fast_idx = ctx.new_block("modulo_preguard.fast");
    let done_idx = ctx.new_block("modulo_preguard.done");
    let fast_label = ctx.block_label(fast_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&guard_ok, &fast_label, &done_label);

    ctx.current_block = fast_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_array_guard_fast_passes",
        &[(I64, &site_id), (I64, &skipped_i64)],
    );
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(PreguardedNumericArrayModuloIndexGet {
        array_local_id: preguard.array_local_id,
        index: preguard.index,
        site_id,
        guard_ok_slot,
    })
}

fn emit_affine_numeric_array_index_set_preguard(
    ctx: &mut FnCtx<'_>,
    preguard: AffineNumericArrayIndexSetPreguard,
    counter_id: u32,
    bound_i32_slot: &str,
) -> Result<PreguardedNumericArrayAffineIndexSet> {
    let arr_expr = perry_hir::Expr::LocalGet(preguard.array_local_id);
    let arr_box = lower_expr(ctx, &arr_expr)?;
    let counter_slot = ctx
        .i32_counter_slots
        .get(&counter_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("affine array set preguard missing i32 counter"))?;
    let bound_i32 = ctx.block().load(I32, bound_i32_slot);
    let counter_i32 = ctx.block().load(I32, &counter_slot);
    let remaining_i32 = ctx.block().sub(I32, &bound_i32, &counter_i32);
    let skipped_i32 = ctx.block().sub(I32, &remaining_i32, "1");
    let skipped_i64 = ctx.block().zext(I32, &skipped_i32, I64);
    let last_counter_i32 = ctx.block().sub(I32, &bound_i32, "1");

    let max_i32 = match &preguard.index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
            mul_local_id,
            bound_local_id,
            ..
        } => {
            let mul_slot = ctx
                .i32_counter_slots
                .get(mul_local_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("affine array set preguard missing i32 multiplier")
                })?;
            let bound_slot = ctx
                .i32_counter_slots
                .get(bound_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array set preguard missing i32 bound"))?;
            let mul = ctx.block().load(I32, &mul_slot);
            let bound = ctx.block().load(I32, &bound_slot);
            let base = ctx.block().mul(I32, &mul, &bound);
            ctx.block().add(I32, &base, &last_counter_i32)
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
            bound_local_id,
            add_local_id,
            ..
        } => {
            let bound_slot = ctx
                .i32_counter_slots
                .get(bound_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array set preguard missing i32 bound"))?;
            let add_slot = ctx
                .i32_counter_slots
                .get(add_local_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("affine array set preguard missing i32 addend"))?;
            let bound = ctx.block().load(I32, &bound_slot);
            let add = ctx.block().load(I32, &add_slot);
            let base = ctx.block().mul(I32, &last_counter_i32, &bound);
            ctx.block().add(I32, &base, &add)
        }
    };
    let site_id = emit_typed_feedback_register_site(
        ctx,
        TypedFeedbackKind::ArrayElement,
        "array[index]=",
        TypedFeedbackContract::numeric_array_set_index(),
    );
    let guard_result = ctx.block().call(
        I32,
        "js_typed_feedback_numeric_array_index_set_guard",
        &[
            (I64, &site_id),
            (DOUBLE, &arr_box),
            (I32, &max_i32),
            (DOUBLE, "0.0"),
            (I32, "1"),
        ],
    );
    let guard_ok_slot = ctx.func.alloca_entry(I32);
    ctx.block().store(I32, &guard_result, &guard_ok_slot);
    let guard_ok = ctx.block().icmp_ne(I32, &guard_result, "0");

    let fast_idx = ctx.new_block("affine_set_preguard.fast");
    let done_idx = ctx.new_block("affine_set_preguard.done");
    let fast_label = ctx.block_label(fast_idx);
    let done_label = ctx.block_label(done_idx);
    ctx.block().cond_br(&guard_ok, &fast_label, &done_label);

    ctx.current_block = fast_idx;
    ctx.block().call_void(
        "js_typed_feedback_record_array_guard_fast_passes",
        &[(I64, &site_id), (I64, &skipped_i64)],
    );
    ctx.block().br(&done_label);

    ctx.current_block = done_idx;
    Ok(PreguardedNumericArrayAffineIndexSet {
        array_local_id: preguard.array_local_id,
        index: preguard.index,
        site_id,
        guard_ok_slot,
    })
}

fn emit_i32_length_loop_condition(
    ctx: &mut FnCtx<'_>,
    counter_id: u32,
    op: perry_hir::CompareOp,
    len_i32_slot: &str,
    true_label: &str,
    false_label: &str,
) -> bool {
    let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() else {
        return false;
    };
    let ctr = ctx.block().load(I32, &ctr_i32_slot);
    let len = ctx.block().load(I32, len_i32_slot);
    let cmp = match op {
        perry_hir::CompareOp::Le => ctx.block().icmp_sle(I32, &ctr, &len),
        _ => ctx.block().icmp_slt(I32, &ctr, &len),
    };
    ctx.block().cond_br(&cmp, true_label, false_label);
    true
}

fn classify_invariant_numeric_array_index_get_hoist(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    inner_counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<InvariantArrayIndexGetHoist> {
    if !expr_has_numeric_pointer_free_array_layout(ctx, &perry_hir::Expr::LocalGet(arr_id)) {
        return None;
    }
    let [perry_hir::Stmt::Expr(expr)] = body else {
        return None;
    };
    let index_id = find_invariant_array_index_get_candidate(ctx, expr, arr_id, inner_counter_id)?;
    if update
        .is_some_and(|expr| expr_mutates_local(expr, arr_id) || expr_mutates_local(expr, index_id))
    {
        return None;
    }
    if expr_mutates_local(expr, arr_id) || expr_mutates_local(expr, index_id) {
        return None;
    }
    if !expr_preserves_invariant_array_read(expr, arr_id, index_id) {
        return None;
    }
    Some(InvariantArrayIndexGetHoist {
        array_local_id: arr_id,
        index_local_id: index_id,
        inner_counter_local_id: inner_counter_id,
    })
}

fn classify_range_numeric_array_index_get_preguard(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    counter_id: u32,
    allowed_hoisted_index_id: Option<u32>,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<RangeNumericArrayIndexGetPreguard> {
    if !expr_has_numeric_pointer_free_array_layout(ctx, &perry_hir::Expr::LocalGet(arr_id)) {
        return None;
    }
    let [perry_hir::Stmt::Expr(expr)] = body else {
        return None;
    };
    if update.is_some_and(|expr| expr_mutates_local(expr, arr_id)) {
        return None;
    }
    if expr_mutates_local(expr, arr_id) || expr_mutates_local(expr, counter_id) {
        return None;
    }
    let count =
        count_range_numeric_array_index_gets(expr, arr_id, counter_id, allowed_hoisted_index_id)?;
    (count == 1).then_some(RangeNumericArrayIndexGetPreguard {
        array_local_id: arr_id,
        index_local_id: counter_id,
    })
}

fn classify_range_numeric_array_index_set_preguard(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<RangeNumericArrayIndexSetPreguard> {
    if !expr_has_numeric_pointer_free_array_layout(ctx, &perry_hir::Expr::LocalGet(arr_id)) {
        return None;
    }
    let [perry_hir::Stmt::Expr(perry_hir::Expr::IndexSet {
        object,
        index,
        value,
    })] = body
    else {
        return None;
    };
    if !matches!(object.as_ref(), perry_hir::Expr::LocalGet(candidate_arr_id) if *candidate_arr_id == arr_id)
    {
        return None;
    }
    if !matches!(index.as_ref(), perry_hir::Expr::LocalGet(candidate_index_id) if *candidate_index_id == counter_id)
    {
        return None;
    }
    if update.is_some_and(|expr| expr_mutates_local(expr, arr_id)) {
        return None;
    }
    if expr_mutates_local(value, arr_id) || expr_mutates_local(value, counter_id) {
        return None;
    }
    if !is_numeric_expr(ctx, value) {
        return None;
    }
    if !expr_preserves_invariant_array_read(value, arr_id, counter_id) {
        return None;
    }
    Some(RangeNumericArrayIndexSetPreguard {
        array_local_id: arr_id,
        index_local_id: counter_id,
    })
}

fn classify_range_plain_array_index_set_preguard_for_bounds(
    ctx: &crate::expr::FnCtx<'_>,
    arr_id: u32,
    counter_id: u32,
    max_index: PlainArraySetMaxIndex,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<RangePlainArrayIndexSetPreguard> {
    let arr_expr = perry_hir::Expr::LocalGet(arr_id);
    if !is_array_expr(ctx, &arr_expr) || expr_has_numeric_pointer_free_array_layout(ctx, &arr_expr)
    {
        return None;
    }
    if !update_is_absent_or_counter_increment(update, counter_id)
        || update.is_some_and(|expr| expr_mutates_local(expr, arr_id))
    {
        return None;
    }
    let [perry_hir::Stmt::Expr(perry_hir::Expr::IndexSet {
        object,
        index,
        value,
    })] = body
    else {
        return None;
    };
    if !matches!(object.as_ref(), perry_hir::Expr::LocalGet(candidate_arr_id) if *candidate_arr_id == arr_id)
    {
        return None;
    }
    if !matches!(index.as_ref(), perry_hir::Expr::LocalGet(candidate_index_id) if *candidate_index_id == counter_id)
    {
        return None;
    }
    if expr_mutates_local(value, arr_id) || expr_mutates_local(value, counter_id) {
        return None;
    }
    if !expr_preserves_invariant_array_read(value, arr_id, counter_id) {
        return None;
    }
    let f64_numeric_update = plain_array_set_value_is_self_f64_add(value, arr_id, counter_id);
    Some(RangePlainArrayIndexSetPreguard {
        array_local_id: arr_id,
        index_local_id: counter_id,
        max_index,
        f64_numeric_update,
    })
}

fn plain_array_set_value_is_self_f64_add(
    value: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
) -> bool {
    use perry_hir::{BinaryOp, Expr};

    fn is_self_index_get(expr: &Expr, arr_id: u32, counter_id: u32) -> bool {
        matches!(
            expr,
            Expr::IndexGet { object, index }
                if matches!(object.as_ref(), Expr::LocalGet(candidate_arr) if *candidate_arr == arr_id)
                    && matches!(index.as_ref(), Expr::LocalGet(candidate_idx) if *candidate_idx == counter_id)
        )
    }

    fn is_numeric_literal(expr: &Expr) -> bool {
        matches!(expr, Expr::Integer(_) | Expr::Number(_))
    }

    let Expr::Binary { op, left, right } = value else {
        return false;
    };
    if !matches!(op, BinaryOp::Add) {
        return false;
    }
    (is_self_index_get(left, arr_id, counter_id) && is_numeric_literal(right))
        || (is_self_index_get(right, arr_id, counter_id) && is_numeric_literal(left))
}

fn classify_range_plain_array_index_set_preguard_for_condition(
    ctx: &crate::expr::FnCtx<'_>,
    cond: &perry_hir::Expr,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Option<RangePlainArrayIndexSetPreguard> {
    use perry_hir::{CompareOp, Expr};
    let Expr::Compare { op, left, right } = cond else {
        return None;
    };
    if !matches!(op, CompareOp::Lt) {
        return None;
    }
    let Expr::LocalGet(counter_id) = left.as_ref() else {
        return None;
    };
    let [perry_hir::Stmt::Expr(perry_hir::Expr::IndexSet { object, .. })] = body else {
        return None;
    };
    let Expr::LocalGet(arr_id) = object.as_ref() else {
        return None;
    };
    if !plain_array_set_bound_expr_is_invariant(ctx, right, *arr_id, *counter_id, update, body) {
        return None;
    }
    classify_range_plain_array_index_set_preguard_for_bounds(
        ctx,
        *arr_id,
        *counter_id,
        PlainArraySetMaxIndex::BoundExpr((**right).clone()),
        update,
        body,
    )
}

fn plain_array_set_bound_expr_is_invariant(
    ctx: &crate::expr::FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> bool {
    use perry_hir::{BinaryOp, Expr};
    match expr {
        Expr::Integer(_) | Expr::Number(_) => true,
        Expr::LocalGet(id) => {
            *id != arr_id
                && *id != counter_id
                && (ctx.locals.contains_key(id)
                    || ctx.module_globals.contains_key(id)
                    || ctx.integer_locals.contains(id))
                && !update.is_some_and(|expr| expr_mutates_local(expr, *id))
                && !stmts_mutate_local(body, *id)
        }
        Expr::Binary { op, left, right } if matches!(op, BinaryOp::Add | BinaryOp::Sub) => {
            plain_array_set_bound_expr_is_invariant(ctx, left, arr_id, counter_id, update, body)
                && plain_array_set_bound_expr_is_invariant(
                    ctx, right, arr_id, counter_id, update, body,
                )
        }
        _ => false,
    }
}

fn classify_affine_numeric_array_index_get_preguards(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    bound_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Vec<AffineNumericArrayIndexGetPreguard> {
    let [perry_hir::Stmt::Expr(expr)] = body else {
        return Vec::new();
    };
    let Some(mut candidates) = collect_affine_numeric_array_index_gets(expr, counter_id, bound_id)
    else {
        return Vec::new();
    };
    if candidates.is_empty() || candidates.len() > 4 {
        return Vec::new();
    }

    let mut seen = std::collections::HashSet::new();
    candidates.retain(|candidate| seen.insert((candidate.array_local_id, candidate.index.clone())));

    for candidate in &candidates {
        if !expr_has_numeric_pointer_free_array_layout(
            ctx,
            &perry_hir::Expr::LocalGet(candidate.array_local_id),
        ) {
            return Vec::new();
        }

        let protected = affine_index_protected_locals(candidate);
        if protected.iter().any(|id| expr_mutates_local(expr, *id)) {
            return Vec::new();
        }
        if update.is_some_and(|expr| {
            protected
                .iter()
                .any(|id| *id != counter_id && expr_mutates_local(expr, *id))
        }) {
            return Vec::new();
        }
        if !affine_index_i32_slots_are_available(ctx, &candidate.index) {
            return Vec::new();
        }
        if !affine_index_nonnegative_inputs_are_safe(ctx, &candidate.index) {
            return Vec::new();
        }
    }

    candidates
}

fn classify_modulo_numeric_array_index_get_preguards(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Vec<ModuloNumericArrayIndexGetPreguard> {
    let [perry_hir::Stmt::Expr(expr)] = body else {
        return Vec::new();
    };
    let Some(mut candidates) = collect_modulo_numeric_array_index_gets(expr, counter_id) else {
        return Vec::new();
    };
    if candidates.is_empty() || candidates.len() > 4 {
        return Vec::new();
    }

    let mut seen = std::collections::HashSet::new();
    candidates.retain(|candidate| seen.insert((candidate.array_local_id, candidate.index.clone())));

    for candidate in &candidates {
        if !expr_has_numeric_pointer_free_array_layout(
            ctx,
            &perry_hir::Expr::LocalGet(candidate.array_local_id),
        ) {
            return Vec::new();
        }

        let protected = modulo_index_protected_locals(candidate);
        if protected.iter().any(|id| expr_mutates_local(expr, *id)) {
            return Vec::new();
        }
        if update.is_some_and(|expr| {
            protected
                .iter()
                .any(|id| *id != counter_id && expr_mutates_local(expr, *id))
        }) {
            return Vec::new();
        }
        if !modulo_index_i32_slots_are_available(ctx, &candidate.index) {
            return Vec::new();
        }
        if !modulo_index_inputs_are_safe(ctx, &candidate.index) {
            return Vec::new();
        }
    }

    candidates
}

fn classify_affine_numeric_array_index_set_preguards(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    bound_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> Vec<AffineNumericArrayIndexSetPreguard> {
    let Some(mut candidates) =
        collect_affine_numeric_array_index_sets(ctx, body, counter_id, bound_id)
    else {
        return Vec::new();
    };
    if candidates.is_empty() || candidates.len() > 2 {
        return Vec::new();
    }

    let mut seen = std::collections::HashSet::new();
    candidates.retain(|candidate| seen.insert((candidate.array_local_id, candidate.index.clone())));

    for candidate in &candidates {
        if !expr_has_numeric_pointer_free_array_layout(
            ctx,
            &perry_hir::Expr::LocalGet(candidate.array_local_id),
        ) {
            return Vec::new();
        }

        let protected = affine_set_index_protected_locals(candidate);
        if protected.iter().any(|id| stmts_mutate_local(body, *id)) {
            return Vec::new();
        }
        if update.is_some_and(|expr| {
            protected
                .iter()
                .any(|id| *id != counter_id && expr_mutates_local(expr, *id))
        }) {
            return Vec::new();
        }
        if !affine_index_i32_slots_are_available(ctx, &candidate.index) {
            return Vec::new();
        }
        if !affine_index_nonnegative_inputs_are_safe(ctx, &candidate.index) {
            return Vec::new();
        }
    }

    candidates
}

fn affine_index_protected_locals(candidate: &AffineNumericArrayIndexGetPreguard) -> Vec<u32> {
    let mut locals = vec![candidate.array_local_id];
    match &candidate.index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
            mul_local_id,
            bound_local_id,
            counter_local_id,
        } => {
            locals.push(*mul_local_id);
            locals.push(*bound_local_id);
            locals.push(*counter_local_id);
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
            counter_local_id,
            bound_local_id,
            add_local_id,
        } => {
            locals.push(*counter_local_id);
            locals.push(*bound_local_id);
            locals.push(*add_local_id);
        }
    }
    locals.sort_unstable();
    locals.dedup();
    locals
}

fn modulo_index_protected_locals(candidate: &ModuloNumericArrayIndexGetPreguard) -> Vec<u32> {
    let mut locals = vec![candidate.array_local_id];
    match &candidate.index {
        PreguardedModuloIndexExpr::LocalModuloLocal {
            value_local_id,
            modulus_local_id,
        }
        | PreguardedModuloIndexExpr::LocalTimesConstModuloLocal {
            value_local_id,
            modulus_local_id,
            ..
        } => {
            locals.push(*value_local_id);
            locals.push(*modulus_local_id);
        }
    }
    locals.sort_unstable();
    locals.dedup();
    locals
}

fn affine_set_index_protected_locals(candidate: &AffineNumericArrayIndexSetPreguard) -> Vec<u32> {
    let mut locals = vec![candidate.array_local_id];
    match &candidate.index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
            mul_local_id,
            bound_local_id,
            counter_local_id,
        } => {
            locals.push(*mul_local_id);
            locals.push(*bound_local_id);
            locals.push(*counter_local_id);
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
            counter_local_id,
            bound_local_id,
            add_local_id,
        } => {
            locals.push(*counter_local_id);
            locals.push(*bound_local_id);
            locals.push(*add_local_id);
        }
    }
    locals.sort_unstable();
    locals.dedup();
    locals
}

fn affine_index_i32_slots_are_available(
    ctx: &crate::expr::FnCtx<'_>,
    index: &PreguardedAffineIndexExpr,
) -> bool {
    match index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
            mul_local_id,
            bound_local_id,
            counter_local_id,
        } => {
            ctx.i32_counter_slots.contains_key(mul_local_id)
                && ctx.i32_counter_slots.contains_key(bound_local_id)
                && ctx.i32_counter_slots.contains_key(counter_local_id)
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
            counter_local_id,
            bound_local_id,
            add_local_id,
        } => {
            ctx.i32_counter_slots.contains_key(counter_local_id)
                && ctx.i32_counter_slots.contains_key(bound_local_id)
                && ctx.i32_counter_slots.contains_key(add_local_id)
        }
    }
}

fn modulo_index_i32_slots_are_available(
    ctx: &crate::expr::FnCtx<'_>,
    index: &PreguardedModuloIndexExpr,
) -> bool {
    match index {
        PreguardedModuloIndexExpr::LocalModuloLocal {
            value_local_id,
            modulus_local_id,
        }
        | PreguardedModuloIndexExpr::LocalTimesConstModuloLocal {
            value_local_id,
            modulus_local_id,
            ..
        } => {
            ctx.i32_counter_slots.contains_key(value_local_id)
                && ctx.i32_counter_slots.contains_key(modulus_local_id)
        }
    }
}

fn modulo_index_inputs_are_safe(
    ctx: &crate::expr::FnCtx<'_>,
    index: &PreguardedModuloIndexExpr,
) -> bool {
    let (value_local_id, modulus_local_id, multiplier) = match index {
        PreguardedModuloIndexExpr::LocalModuloLocal {
            value_local_id,
            modulus_local_id,
        } => (*value_local_id, *modulus_local_id, 1),
        PreguardedModuloIndexExpr::LocalTimesConstModuloLocal {
            value_local_id,
            multiplier,
            modulus_local_id,
        } => (*value_local_id, *modulus_local_id, *multiplier),
    };
    if multiplier <= 0 || !loop_counter_is_nonnegative_at_entry(ctx, value_local_id) {
        return false;
    }
    let Some(value_range) =
        crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(value_local_id))
    else {
        return false;
    };
    if value_range.min < 0
        || value_range
            .max
            .checked_mul(i64::from(multiplier))
            .is_none_or(|max| max > i64::from(i32::MAX))
    {
        return false;
    }
    crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(modulus_local_id))
        .is_some_and(|range| range.min > 0 && range.max <= i64::from(i32::MAX))
}

fn affine_index_nonnegative_inputs_are_safe(
    ctx: &crate::expr::FnCtx<'_>,
    index: &PreguardedAffineIndexExpr,
) -> bool {
    match index {
        PreguardedAffineIndexExpr::MulLocalBoundPlusCounter { mul_local_id, .. } => {
            loop_counter_is_nonnegative_at_entry(ctx, *mul_local_id)
        }
        PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal { add_local_id, .. } => {
            loop_counter_is_nonnegative_at_entry(ctx, *add_local_id)
        }
    }
}

fn collect_affine_numeric_array_index_gets(
    expr: &perry_hir::Expr,
    counter_id: u32,
    bound_id: u32,
) -> Option<Vec<AffineNumericArrayIndexGetPreguard>> {
    use perry_hir::{ArrayElement, Expr};

    let collect = |expr: &Expr| collect_affine_numeric_array_index_gets(expr, counter_id, bound_id);
    let collect_pair = |left: &Expr, right: &Expr| {
        let mut out = collect(left)?;
        out.extend(collect(right)?);
        Some(out)
    };

    match expr {
        Expr::IndexGet { object, index } => match object.as_ref() {
            Expr::LocalGet(array_local_id) => {
                let index = classify_affine_index_expr(index.as_ref(), counter_id, bound_id)?;
                Some(vec![AffineNumericArrayIndexGetPreguard {
                    array_local_id: *array_local_id,
                    index,
                }])
            }
            _ => None,
        },
        Expr::LocalSet(id, value) => {
            if *id == counter_id || *id == bound_id {
                None
            } else {
                collect(value)
            }
        }
        Expr::Update { id, .. } => (*id != counter_id && *id != bound_id).then_some(Vec::new()),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            collect_pair(left.as_ref(), right.as_ref())
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => collect(operand),
        Expr::Array(elements) => elements.iter().try_fold(Vec::new(), |mut acc, expr| {
            acc.extend(collect(expr)?);
            Some(acc)
        }),
        Expr::ArraySpread(elements) => elements.iter().try_fold(Vec::new(), |mut acc, element| {
            match element {
                ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                    acc.extend(collect(expr)?);
                }
            }
            Some(acc)
        }),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            collect_pair(left.as_ref(), right.as_ref())
        }
        Expr::MathMin(elements) | Expr::MathMax(elements) => {
            elements.iter().try_fold(Vec::new(), |mut acc, expr| {
                acc.extend(collect(expr)?);
                Some(acc)
            })
        }
        Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathF16round(expr) => collect(expr),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => Some(Vec::new()),
        // Avoid preguarding across runtime calls, short-circuit paths, or
        // HIR variants whose evaluation order/mutation behavior is not
        // modeled by this narrow classifier.
        _ => None,
    }
}

fn collect_modulo_numeric_array_index_gets(
    expr: &perry_hir::Expr,
    counter_id: u32,
) -> Option<Vec<ModuloNumericArrayIndexGetPreguard>> {
    use perry_hir::{ArrayElement, Expr};

    let collect = |expr: &Expr| collect_modulo_numeric_array_index_gets(expr, counter_id);
    let collect_pair = |left: &Expr, right: &Expr| {
        let mut out = collect(left)?;
        out.extend(collect(right)?);
        Some(out)
    };

    match expr {
        Expr::IndexGet { object, index } => match object.as_ref() {
            Expr::LocalGet(array_local_id) => {
                let index = classify_modulo_index_expr(index.as_ref(), counter_id)?;
                Some(vec![ModuloNumericArrayIndexGetPreguard {
                    array_local_id: *array_local_id,
                    index,
                }])
            }
            _ => None,
        },
        Expr::LocalSet(id, value) => {
            if *id == counter_id {
                None
            } else {
                collect(value)
            }
        }
        Expr::Update { id, .. } => (*id != counter_id).then_some(Vec::new()),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            collect_pair(left.as_ref(), right.as_ref())
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => collect(operand),
        Expr::Array(elements) => elements.iter().try_fold(Vec::new(), |mut acc, expr| {
            acc.extend(collect(expr)?);
            Some(acc)
        }),
        Expr::ArraySpread(elements) => elements.iter().try_fold(Vec::new(), |mut acc, element| {
            match element {
                ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                    acc.extend(collect(expr)?);
                }
            }
            Some(acc)
        }),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            collect_pair(left.as_ref(), right.as_ref())
        }
        Expr::MathMin(elements) | Expr::MathMax(elements) => {
            elements.iter().try_fold(Vec::new(), |mut acc, expr| {
                acc.extend(collect(expr)?);
                Some(acc)
            })
        }
        Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathF16round(expr) => collect(expr),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => Some(Vec::new()),
        _ => None,
    }
}

fn collect_affine_numeric_array_index_sets(
    ctx: &crate::expr::FnCtx<'_>,
    body: &[perry_hir::Stmt],
    counter_id: u32,
    bound_id: u32,
) -> Option<Vec<AffineNumericArrayIndexSetPreguard>> {
    use perry_hir::{Expr, Stmt};

    let mut out = Vec::new();
    let mut body_numeric_locals = std::collections::HashSet::new();
    for stmt in body {
        match stmt {
            Stmt::Expr(Expr::IndexSet {
                object,
                index,
                value,
            }) => {
                let Expr::LocalGet(array_local_id) = object.as_ref() else {
                    return None;
                };
                if !affine_set_value_is_numeric(ctx, value, &body_numeric_locals)
                    || !affine_set_preguard_transparent_expr(value.as_ref())
                {
                    return None;
                }
                let index = classify_affine_index_expr(index.as_ref(), counter_id, bound_id)?;
                out.push(AffineNumericArrayIndexSetPreguard {
                    array_local_id: *array_local_id,
                    index,
                });
            }
            Stmt::Expr(expr) => {
                if !affine_set_preguard_transparent_expr(expr) {
                    return None;
                }
            }
            Stmt::Let { id, ty, init, .. } => {
                if let Some(init) = init {
                    if !affine_set_preguard_transparent_expr(init) {
                        return None;
                    }
                    if matches!(ty, perry_types::Type::Number | perry_types::Type::Int32)
                        && affine_set_value_is_numeric(ctx, init, &body_numeric_locals)
                    {
                        body_numeric_locals.insert(*id);
                    } else {
                        body_numeric_locals.remove(id);
                    }
                }
            }
            Stmt::For {
                init,
                condition,
                update,
                body,
            } => {
                if init
                    .as_ref()
                    .is_some_and(|stmt| !affine_set_preguard_transparent_stmt(stmt.as_ref()))
                    || condition
                        .as_ref()
                        .is_some_and(|expr| !affine_set_preguard_transparent_expr(expr))
                    || update
                        .as_ref()
                        .is_some_and(|expr| !affine_set_preguard_transparent_expr(expr))
                    || body
                        .iter()
                        .any(|stmt| !affine_set_preguard_transparent_stmt(stmt))
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn affine_set_value_is_numeric(
    ctx: &crate::expr::FnCtx<'_>,
    expr: &perry_hir::Expr,
    body_numeric_locals: &std::collections::HashSet<u32>,
) -> bool {
    use perry_hir::{BinaryOp, Expr};

    if is_numeric_expr(ctx, expr) {
        return true;
    }
    match expr {
        Expr::LocalGet(id) => body_numeric_locals.contains(id),
        Expr::Binary {
            op: BinaryOp::Add,
            left,
            right,
        } => {
            affine_set_value_is_numeric(ctx, left, body_numeric_locals)
                && affine_set_value_is_numeric(ctx, right, body_numeric_locals)
        }
        Expr::Binary { .. } | Expr::Update { .. } => true,
        _ => false,
    }
}

fn affine_set_preguard_transparent_stmt(stmt: &perry_hir::Stmt) -> bool {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_none_or(|expr| affine_set_preguard_transparent_expr(expr)),
        Stmt::Expr(expr) => affine_set_preguard_transparent_expr(expr),
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_none_or(|stmt| affine_set_preguard_transparent_stmt(stmt.as_ref()))
                && condition
                    .as_ref()
                    .is_none_or(|expr| affine_set_preguard_transparent_expr(expr))
                && update
                    .as_ref()
                    .is_none_or(|expr| affine_set_preguard_transparent_expr(expr))
                && body
                    .iter()
                    .all(|stmt| affine_set_preguard_transparent_stmt(stmt))
        }
        _ => false,
    }
}

fn affine_set_preguard_transparent_expr(expr: &perry_hir::Expr) -> bool {
    use perry_hir::{ArrayElement, Expr};
    match expr {
        Expr::LocalSet(_, value)
        | Expr::Unary { operand: value, .. }
        | Expr::Void(value)
        | Expr::TypeOf(value)
        | Expr::StringCoerce(value)
        | Expr::ObjectCoerce(value)
        | Expr::BooleanCoerce(value)
        | Expr::NumberCoerce(value)
        | Expr::MathAbs(value)
        | Expr::MathSqrt(value)
        | Expr::MathFloor(value)
        | Expr::MathCeil(value)
        | Expr::MathRound(value)
        | Expr::MathF16round(value) => affine_set_preguard_transparent_expr(value),
        Expr::Update { .. } => true,
        Expr::IndexGet { object, index } => {
            affine_set_preguard_transparent_expr(object)
                && affine_set_preguard_transparent_expr(index)
        }
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::MathImul(left, right)
        | Expr::MathPow(left, right) => {
            affine_set_preguard_transparent_expr(left)
                && affine_set_preguard_transparent_expr(right)
        }
        Expr::Array(elements) => elements
            .iter()
            .all(|expr| affine_set_preguard_transparent_expr(expr)),
        Expr::ArraySpread(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => {
                affine_set_preguard_transparent_expr(expr)
            }
        }),
        Expr::MathMin(elements) | Expr::MathMax(elements) => elements
            .iter()
            .all(|expr| affine_set_preguard_transparent_expr(expr)),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        _ => false,
    }
}

fn classify_affine_index_expr(
    expr: &perry_hir::Expr,
    counter_id: u32,
    bound_id: u32,
) -> Option<PreguardedAffineIndexExpr> {
    use perry_hir::{BinaryOp, Expr};

    fn local(expr: &Expr) -> Option<u32> {
        match expr {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        }
    }

    fn mul_pair(expr: &Expr) -> Option<(u32, u32)> {
        match expr {
            Expr::Binary {
                op: BinaryOp::Mul,
                left,
                right,
            } => Some((local(left.as_ref())?, local(right.as_ref())?)),
            _ => None,
        }
    }

    fn other_mul_local(pair: (u32, u32), known: u32) -> Option<u32> {
        if pair.0 == known {
            Some(pair.1)
        } else if pair.1 == known {
            Some(pair.0)
        } else {
            None
        }
    }

    let Expr::Binary {
        op: BinaryOp::Add,
        left,
        right,
    } = expr
    else {
        return None;
    };

    for (mul, add) in [
        (mul_pair(left.as_ref()), local(right.as_ref())),
        (mul_pair(right.as_ref()), local(left.as_ref())),
    ] {
        let (Some(pair), Some(add_id)) = (mul, add) else {
            continue;
        };
        if add_id == counter_id {
            if let Some(mul_local_id) = other_mul_local(pair, bound_id) {
                if mul_local_id != counter_id {
                    return Some(PreguardedAffineIndexExpr::MulLocalBoundPlusCounter {
                        mul_local_id,
                        bound_local_id: bound_id,
                        counter_local_id: counter_id,
                    });
                }
            }
        } else if add_id != counter_id {
            if other_mul_local(pair, counter_id) == Some(bound_id) {
                return Some(PreguardedAffineIndexExpr::CounterTimesBoundPlusLocal {
                    counter_local_id: counter_id,
                    bound_local_id: bound_id,
                    add_local_id: add_id,
                });
            }
        }
    }

    None
}

fn classify_modulo_index_expr(
    expr: &perry_hir::Expr,
    counter_id: u32,
) -> Option<PreguardedModuloIndexExpr> {
    use perry_hir::{BinaryOp, Expr};

    fn local(expr: &Expr) -> Option<u32> {
        match expr {
            Expr::LocalGet(id) => Some(*id),
            _ => None,
        }
    }

    fn counter_times_const(expr: &Expr, counter_id: u32) -> Option<i32> {
        match expr {
            Expr::LocalGet(id) if *id == counter_id => Some(1),
            Expr::Binary {
                op: BinaryOp::Mul,
                left,
                right,
            } => match (left.as_ref(), right.as_ref()) {
                (Expr::LocalGet(id), Expr::Integer(multiplier))
                | (Expr::Integer(multiplier), Expr::LocalGet(id))
                    if *id == counter_id =>
                {
                    i32::try_from(*multiplier).ok().filter(|value| *value > 0)
                }
                _ => None,
            },
            _ => None,
        }
    }

    let Expr::Binary {
        op: BinaryOp::Mod,
        left,
        right,
    } = expr
    else {
        return None;
    };
    let modulus_local_id = local(right.as_ref())?;
    if modulus_local_id == counter_id {
        return None;
    }
    let multiplier = counter_times_const(left.as_ref(), counter_id)?;
    if multiplier == 1 {
        Some(PreguardedModuloIndexExpr::LocalModuloLocal {
            value_local_id: counter_id,
            modulus_local_id,
        })
    } else {
        Some(PreguardedModuloIndexExpr::LocalTimesConstModuloLocal {
            value_local_id: counter_id,
            multiplier,
            modulus_local_id,
        })
    }
}

fn count_range_numeric_array_index_gets(
    expr: &perry_hir::Expr,
    arr_id: u32,
    counter_id: u32,
    allowed_hoisted_index_id: Option<u32>,
) -> Option<usize> {
    use perry_hir::{ArrayElement, Expr};

    let count = |expr: &Expr| {
        count_range_numeric_array_index_gets(expr, arr_id, counter_id, allowed_hoisted_index_id)
    };
    let count_pair = |left: &Expr, right: &Expr| Some(count(left)? + count(right)?);

    match expr {
        Expr::IndexGet { object, index } => match (object.as_ref(), index.as_ref()) {
            (Expr::LocalGet(candidate_arr_id), Expr::LocalGet(candidate_index_id))
                if *candidate_arr_id == arr_id && *candidate_index_id == counter_id =>
            {
                Some(1)
            }
            (Expr::LocalGet(candidate_arr_id), Expr::LocalGet(candidate_index_id))
                if *candidate_arr_id == arr_id
                    && Some(*candidate_index_id) == allowed_hoisted_index_id =>
            {
                Some(0)
            }
            _ => None,
        },
        Expr::LocalSet(id, value) => {
            if *id == arr_id || *id == counter_id {
                None
            } else {
                count(value)
            }
        }
        Expr::Update { id, .. } => (*id != arr_id && *id != counter_id).then_some(0),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            count_pair(left.as_ref(), right.as_ref())
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => count(operand),
        Expr::Array(elements) => elements
            .iter()
            .try_fold(0, |acc, expr| Some(acc + count(expr)?)),
        Expr::ArraySpread(elements) => elements.iter().try_fold(0, |acc, element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => Some(acc + count(expr)?),
        }),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            count_pair(left.as_ref(), right.as_ref())
        }
        Expr::MathMin(elements) | Expr::MathMax(elements) => elements
            .iter()
            .try_fold(0, |acc, expr| Some(acc + count(expr)?)),
        Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathF16round(expr) => count(expr),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => Some(0),
        // Avoid cached raw loads across runtime calls or branch-only reads.
        Expr::Call { .. }
        | Expr::CallSpread { .. }
        | Expr::Conditional { .. }
        | Expr::Logical { .. } => None,
        _ => None,
    }
}

fn find_invariant_array_index_get_candidate(
    ctx: &crate::expr::FnCtx<'_>,
    expr: &perry_hir::Expr,
    arr_id: u32,
    inner_counter_id: u32,
) -> Option<u32> {
    use perry_hir::{ArrayElement, Expr};

    let find =
        |expr: &Expr| find_invariant_array_index_get_candidate(ctx, expr, arr_id, inner_counter_id);
    let find_in_order = |exprs: &[&Expr]| exprs.iter().find_map(|expr| find(expr));

    match expr {
        Expr::IndexGet { object, index } => {
            if let (Expr::LocalGet(candidate_arr_id), Expr::LocalGet(candidate_index_id)) =
                (object.as_ref(), index.as_ref())
            {
                if *candidate_arr_id == arr_id
                    && *candidate_index_id != inner_counter_id
                    && ctx.bounded_index_pairs.iter().any(|fact| {
                        fact.array_local_id == arr_id && fact.index_local_id == *candidate_index_id
                    })
                {
                    return Some(*candidate_index_id);
                }
            }
            find_in_order(&[object.as_ref(), index.as_ref()])
        }
        Expr::LocalSet(_, value) => find(value),
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } => {
            find_in_order(&[left.as_ref(), right.as_ref()])
        }
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => find(operand),
        Expr::Array(elements) => elements.iter().find_map(find),
        Expr::ArraySpread(elements) => elements.iter().find_map(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => find(expr),
        }),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => {
            find_in_order(&[left.as_ref(), right.as_ref()])
        }
        Expr::MathMin(elements) | Expr::MathMax(elements) => elements.iter().find_map(find),
        Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathF16round(expr) => find(expr),
        // Avoid changing semantics for short-circuited or branch-only reads.
        Expr::Conditional { .. } | Expr::Logical { .. } => None,
        _ => None,
    }
}

fn expr_preserves_invariant_array_read(expr: &perry_hir::Expr, arr_id: u32, index_id: u32) -> bool {
    use perry_hir::{ArrayElement, Expr};
    let walk = |expr: &Expr| expr_preserves_invariant_array_read(expr, arr_id, index_id);
    match expr {
        Expr::LocalSet(id, value) => *id != arr_id && *id != index_id && walk(value),
        Expr::Update { id, .. } => *id != arr_id && *id != index_id,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => walk(left) && walk(right),
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk(operand),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => walk(condition) && walk(then_expr) && walk(else_expr),
        Expr::IndexGet { object, index } => walk(object) && walk(index),
        Expr::Array(elements) => elements.iter().all(&walk),
        Expr::ArraySpread(elements) => elements.iter().all(|element| match element {
            ArrayElement::Expr(expr) | ArrayElement::Spread(expr) => walk(expr),
        }),
        Expr::MathImul(left, right) | Expr::MathPow(left, right) => walk(left) && walk(right),
        Expr::MathMin(elements) | Expr::MathMax(elements) => elements.iter().all(&walk),
        Expr::MathAbs(expr)
        | Expr::MathSqrt(expr)
        | Expr::MathFloor(expr)
        | Expr::MathCeil(expr)
        | Expr::MathRound(expr)
        | Expr::MathF16round(expr) => walk(expr),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        _ => false,
    }
}

/// Inspect a `for` loop's condition expression and body, and return
/// `Some((arr_local_id, counter_local_id, op))` if the loop is the
/// well-known shape `for (let i = ...; i < <arr>.length; ...) { body }`
/// (or `<=`) AND the body is provably free of operations that can change
/// `arr.length`.
///
/// The walker also accepts `arr[i] = expr` IndexSets where `i` is the
/// loop counter from a strict `<` condition — those are guaranteed
/// inbounds and therefore can't trigger the realloc slow path that would
/// extend `arr.length`. Under `<=`, `i == arr.length` is reachable, so
/// array writes must go through the normal extension-capable path.
pub(crate) fn classify_for_length_hoist(
    cond: &perry_hir::Expr,
    body: &[perry_hir::Stmt],
) -> Option<(u32, u32, perry_hir::CompareOp)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let arr_id = match right {
        Expr::PropertyGet { object, property } if property == "length" => match object.as_ref() {
            Expr::LocalGet(id) => *id,
            _ => return None,
        },
        _ => return None,
    };
    let bounded_idx_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let has_strict_bound = matches!(op, CompareOp::Lt);
    if !body
        .iter()
        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound))
    {
        return None;
    }
    Some((arr_id, bounded_idx_id, op))
}

/// Inspect a `for` loop's condition and return `Some((counter_id, bound_id,
/// op))` if the condition is the shape `counter < bound` (or `<=`) where
/// both sides are `LocalGet` ids, the counter is in `integer_locals`, and
/// the bound is either (a) provably integer-valued (`integer_locals`) or
/// (b) a number-typed local / parameter whose slot is accessible directly
/// (i.e. not boxed and not a module global).
///
/// Case (b) relies on Perry's trust-types philosophy: a `number`-typed local
/// used as a for-loop bound is expected to hold a whole-number value at
/// runtime.  Callers that pass non-integer floats as loop bounds would
/// observe at most one iteration difference — a trade-off that is within
/// Perry's existing trust-types contract.
///
/// Used by `lower_for` to enable the same i32 counter specialization as
/// the `i < arr.length` peephole (`classify_for_length_hoist`) on the
/// common case where the loop bound comes from a function parameter or a
/// number-typed local variable.
pub(crate) fn classify_for_local_bound(
    cond: &perry_hir::Expr,
    ctx: &crate::expr::FnCtx<'_>,
) -> Option<(u32, u32, perry_hir::CompareOp)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (*op, left.as_ref(), right.as_ref()),
        _ => return None,
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    let counter_id = match left {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    let bound_id = match right {
        Expr::LocalGet(id) => *id,
        _ => return None,
    };
    // Counter must be provably integer-valued (initialized from integer
    // literal, only mutated by Update ++/--).
    if !ctx.integer_locals.contains(&counter_id) {
        return None;
    }
    // Bound is safe to fptosi when provably integer-valued, OR when it is a
    // number-typed slot that is accessible without boxing (params and simple
    // `let` locals).  Module globals and boxed (closure-captured) variables
    // go through different load paths so we skip those.
    let bound_is_integer_safe = ctx.integer_locals.contains(&bound_id)
        || (ctx.locals.contains_key(&bound_id)
            && !ctx.boxed_vars.contains(&bound_id)
            && !ctx.module_globals.contains_key(&bound_id)
            && matches!(
                ctx.local_types.get(&bound_id),
                Some(perry_types::Type::Number | perry_types::Type::Int32)
            ));
    if !bound_is_integer_safe {
        return None;
    }
    Some((counter_id, bound_id, op))
}

fn loop_counter_bounds_are_safe(
    ctx: &crate::expr::FnCtx<'_>,
    counter_id: u32,
    update: Option<&perry_hir::Expr>,
    body: &[perry_hir::Stmt],
) -> bool {
    loop_counter_is_nonnegative_at_entry(ctx, counter_id)
        && update_is_absent_or_counter_increment(update, counter_id)
        && !stmts_mutate_local(body, counter_id)
}

fn loop_counter_is_nonnegative_at_entry(ctx: &crate::expr::FnCtx<'_>, counter_id: u32) -> bool {
    ctx.nonnegative_integer_locals.contains(&counter_id)
        || crate::expr::int_range_expr(ctx, &perry_hir::Expr::LocalGet(counter_id))
            .is_some_and(|range| range.min >= 0)
}

fn update_is_absent_or_counter_increment(
    update: Option<&perry_hir::Expr>,
    counter_id: u32,
) -> bool {
    use perry_hir::{Expr, UpdateOp};
    update.is_none_or(|expr| {
        matches!(
            expr,
            Expr::Update {
                id,
                op: UpdateOp::Increment,
                ..
            } if *id == counter_id
        )
    })
}

fn stmts_mutate_local(stmts: &[perry_hir::Stmt], local_id: u32) -> bool {
    stmts.iter().any(|stmt| stmt_mutates_local(stmt, local_id))
}

fn stmt_mutates_local(stmt: &perry_hir::Stmt, local_id: u32) -> bool {
    use perry_hir::Stmt;
    match stmt {
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_mutates_local(expr, local_id)),
        Stmt::Expr(expr) | Stmt::Return(Some(expr)) | Stmt::Throw(expr) => {
            expr_mutates_local(expr, local_id)
        }
        Stmt::Return(None)
        | Stmt::Break
        | Stmt::Continue
        | Stmt::LabeledBreak(_)
        | Stmt::LabeledContinue(_)
        | Stmt::PreallocateBoxes(_) => false,
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_mutates_local(condition, local_id)
                || stmts_mutate_local(then_branch, local_id)
                || else_branch
                    .as_ref()
                    .is_some_and(|body| stmts_mutate_local(body, local_id))
        }
        Stmt::While { condition, body } => {
            expr_mutates_local(condition, local_id) || stmts_mutate_local(body, local_id)
        }
        Stmt::DoWhile { body, condition } => {
            stmts_mutate_local(body, local_id) || expr_mutates_local(condition, local_id)
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_mutates_local(stmt.as_ref(), local_id))
                || condition
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
                || update
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
                || stmts_mutate_local(body, local_id)
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            stmts_mutate_local(body, local_id)
                || catch
                    .as_ref()
                    .is_some_and(|catch| stmts_mutate_local(&catch.body, local_id))
                || finally
                    .as_ref()
                    .is_some_and(|body| stmts_mutate_local(body, local_id))
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_mutates_local(discriminant, local_id)
                || cases.iter().any(|case| {
                    case.test
                        .as_ref()
                        .is_some_and(|expr| expr_mutates_local(expr, local_id))
                        || stmts_mutate_local(&case.body, local_id)
                })
        }
        Stmt::Labeled { body, .. } => stmt_mutates_local(body.as_ref(), local_id),
    }
}

fn expr_mutates_local(expr: &perry_hir::Expr, local_id: u32) -> bool {
    use perry_hir::Expr;
    match expr {
        Expr::LocalSet(id, value) => *id == local_id || expr_mutates_local(value, local_id),
        Expr::Update { id, .. } => *id == local_id,
        Expr::Closure { params, body, .. } => {
            params.iter().any(|param| {
                param
                    .default
                    .as_ref()
                    .is_some_and(|expr| expr_mutates_local(expr, local_id))
            }) || stmts_mutate_local(body, local_id)
        }
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(expr, &mut |child| {
                if !found && expr_mutates_local(child, local_id) {
                    found = true;
                }
            });
            found
        }
    }
}

fn classify_for_counter_range(
    init: Option<&perry_hir::Stmt>,
    cond: Option<&perry_hir::Expr>,
    update: Option<&perry_hir::Expr>,
    ctx: &crate::expr::FnCtx<'_>,
    scope_id: u32,
) -> Option<IntRangeFact> {
    use perry_hir::{CompareOp, Expr, Stmt, UpdateOp};
    let (counter_id, start) = match init? {
        Stmt::Let {
            id,
            init: Some(Expr::Integer(start)),
            ..
        } => (*id, *start),
        _ => return None,
    };
    let Expr::Compare { op, left, right } = cond? else {
        return None;
    };
    if !matches!(op, CompareOp::Lt | CompareOp::Le) {
        return None;
    }
    if !matches!(left.as_ref(), Expr::LocalGet(id) if *id == counter_id) {
        return None;
    }
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
    let bound_range = crate::expr::int_range_expr(ctx, right)?;
    if bound_range.min != bound_range.max {
        return None;
    }
    let upper = bound_range
        .max
        .checked_sub(if matches!(op, CompareOp::Lt) { 1 } else { 0 })?;
    if start <= upper {
        Some(IntRangeFact {
            local_id: counter_id,
            scope_id,
            range: crate::expr::IntRange {
                min: start,
                max: upper,
            },
        })
    } else {
        None
    }
}

pub(crate) fn stmt_preserves_array_length(
    s: &perry_hir::Stmt,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
) -> bool {
    use perry_hir::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => {
            expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
        }
        Stmt::Return(opt) => opt.as_ref().is_none_or(|e| {
            expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
        }),
        Stmt::Let { init, .. } => init.as_ref().is_none_or(|e| {
            expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
        }),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id, has_strict_bound)
                && then_branch.iter().all(|s| {
                    stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                })
                && else_branch.as_ref().is_none_or(|b| {
                    b.iter().all(|s| {
                        stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                    })
                })
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id, has_strict_bound)
                && body.iter().all(|s| {
                    stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                })
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref().is_none_or(|s| {
                stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
            }) && condition.as_ref().is_none_or(|e| {
                expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
            }) && update.as_ref().is_none_or(|e| {
                expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
            }) && body
                .iter()
                .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter()
                .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound))
                && catch.as_ref().is_none_or(|c| {
                    c.body.iter().all(|s| {
                        stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                    })
                })
                && finally.as_ref().is_none_or(|b| {
                    b.iter().all(|s| {
                        stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                    })
                })
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_preserves_array_length(discriminant, arr_id, bounded_idx_id, has_strict_bound)
                && cases.iter().all(|c| {
                    c.test.as_ref().is_none_or(|e| {
                        expr_preserves_array_length(e, arr_id, bounded_idx_id, has_strict_bound)
                    }) && c.body.iter().all(|s| {
                        stmt_preserves_array_length(s, arr_id, bounded_idx_id, has_strict_bound)
                    })
                })
        }
        Stmt::Labeled { body, .. } => {
            stmt_preserves_array_length(body.as_ref(), arr_id, bounded_idx_id, has_strict_bound)
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::PreallocateBoxes(_) => true,
    }
}

pub(crate) fn expr_preserves_array_length(
    e: &perry_hir::Expr,
    arr_id: u32,
    bounded_idx_id: u32,
    has_strict_bound: bool,
) -> bool {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let walk =
        |sub: &Expr| expr_preserves_array_length(sub, arr_id, bounded_idx_id, has_strict_bound);
    match e {
        Expr::ArrayPush { array_id, value } => *array_id != arr_id && walk(value),
        Expr::ArrayPop(id) | Expr::ArrayShift(id) => *id != arr_id,
        Expr::ArraySplice {
            array_id,
            start,
            delete_count,
            items,
        } => {
            *array_id != arr_id
                && walk(start)
                && delete_count.as_ref().is_none_or(|e| walk(e))
                && items.iter().all(&walk)
        }
        Expr::IndexSet {
            object,
            index,
            value,
        } => {
            // `arr[bounded_i] = expr` is the only IndexSet on `arr`
            // we accept, and only under a strict `i < arr.length`
            // guard. With `i <= arr.length`, `i == length` can extend
            // the array and invalidate a hoisted length.
            if let Expr::LocalGet(id) = object.as_ref() {
                if *id == arr_id {
                    if has_strict_bound {
                        if let Expr::LocalGet(idx_id) = index.as_ref() {
                            if *idx_id == bounded_idx_id {
                                return walk(value);
                            }
                        }
                    }
                    return false;
                }
            }
            walk(object) && walk(index) && walk(value)
        }
        // Reassigning the bounded index would invalidate the bound.
        // Reassigning the array variable would also invalidate (we'd
        // be tracking the wrong array).
        Expr::LocalSet(id, value) => *id != arr_id && *id != bounded_idx_id && walk(value),
        // Mutating either the array binding or the bounded index invalidates
        // the loop-local inbounds proof. The normal `for` update expression is
        // outside the body and is checked separately before facts are emitted.
        Expr::Update { id, .. } => *id != arr_id && *id != bounded_idx_id,
        Expr::NativeMethodCall { object, args, .. } => {
            if let Some(o) = object {
                if let Expr::LocalGet(id) = o.as_ref() {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(o) {
                    return false;
                }
            }
            args.iter().all(&walk)
        }
        Expr::Call { callee, args, .. } => {
            if !walk(callee) {
                return false;
            }
            for a in args {
                if let Expr::LocalGet(id) = a {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(a) {
                    return false;
                }
            }
            true
        }
        Expr::CallSpread { callee, args, .. } => {
            if !walk(callee) {
                return false;
            }
            for a in args {
                let inner = match a {
                    CallArg::Expr(e) | CallArg::Spread(e) => e,
                };
                if let Expr::LocalGet(id) = inner {
                    if *id == arr_id {
                        return false;
                    }
                }
                if !walk(inner) {
                    return false;
                }
            }
            true
        }
        Expr::Closure { .. } => false,
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => walk(left) && walk(right),
        Expr::Unary { operand, .. }
        | Expr::Void(operand)
        | Expr::TypeOf(operand)
        | Expr::Await(operand)
        | Expr::Delete(operand)
        | Expr::StringCoerce(operand)
        | Expr::ObjectCoerce(operand)
        | Expr::BooleanCoerce(operand)
        | Expr::NumberCoerce(operand) => walk(operand),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
        } => walk(condition) && walk(then_expr) && walk(else_expr),
        Expr::PropertyGet { object, .. } => walk(object),
        Expr::PropertySet { object, value, .. } => walk(object) && walk(value),
        Expr::IndexGet { object, index } => walk(object) && walk(index),
        // Buffer / Uint8Array reads + writes preserve the underlying array
        // length — Buffer.alloc allocates a fixed-capacity blob, and the
        // GEP-based fast path (`Expr::Uint8ArrayGet`/`Set`,
        // `Expr::BufferIndexGet`/`Set`) doesn't extend it. Without these
        // arms the default `_ => false` arm rejects bodies that touch
        // a Buffer, blocking the `i < dst.length` peephole on
        // `for (let i = 0; i < dst.length; i++) dst[i]` patterns —
        // image_convolution's FNV-1a checksum loop is the canonical
        // example, ~24M iterations through `fcmp olt double` instead of
        // `icmp slt i32`.
        Expr::Uint8ArrayGet { array, index } => walk(array) && walk(index),
        Expr::Uint8ArraySet {
            array,
            index,
            value,
        } => walk(array) && walk(index) && walk(value),
        Expr::BufferIndexGet { buffer, index } => walk(buffer) && walk(index),
        Expr::BufferIndexSet {
            buffer,
            index,
            value,
        } => walk(buffer) && walk(index) && walk(value),
        // Pure arithmetic intrinsics — `Math.imul(a, b)` lowers to
        // `Expr::MathImul`, `Math.abs/sqrt/pow/floor/ceil/round` etc. all
        // bottom out as numeric ops with no side effects on the bounded
        // array. image_conv's FNV-1a body uses Math.imul and was rejecting
        // the peephole until this arm landed.
        Expr::MathImul(a, b) | Expr::MathPow(a, b) => walk(a) && walk(b),
        Expr::MathMin(elems) | Expr::MathMax(elems) => elems.iter().all(&walk),
        Expr::MathAbs(a)
        | Expr::MathSqrt(a)
        | Expr::MathFloor(a)
        | Expr::MathCeil(a)
        | Expr::MathRound(a)
        | Expr::MathF16round(a) => walk(a),
        Expr::Array(elements) => elements.iter().all(&walk),
        Expr::ArraySpread(elements) => elements.iter().all(|el| match el {
            ArrayElement::Expr(e) | ArrayElement::Spread(e) => walk(e),
        }),
        Expr::Object(fields) => fields.iter().all(|(_, v)| walk(v)),
        Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::FuncRef(_)
        | Expr::Number(_)
        | Expr::Integer(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Undefined
        | Expr::String(_)
        | Expr::WtfString(_) => true,
        // Default: conservative reject for HIR variants we haven't
        // analyzed. Better to lose the optimization than to silently
        // hoist past a body that mutates the array.
        _ => false,
    }
}

/// `while (cond) { body }` — classic 3-block CFG (cond / body / exit).
///
/// ```text
///   <current>:
///     br cond
///   while.cond:
///     <condition>
///     truthy → body, falsey → exit
///   while.body:
///     <body>
///     br cond                 ; if not already terminated
///   while.exit:
///     <continues here>
/// ```
///
/// No break/continue support yet — body must fall through to the next
/// loop iteration. Same limitation as `for`.
pub(crate) fn lower_while(
    ctx: &mut FnCtx<'_>,
    condition: &perry_hir::Expr,
    body: &[Stmt],
) -> Result<()> {
    let cond_idx = ctx.new_block("while.cond");
    let body_idx = ctx.new_block("while.body");
    let exit_idx = ctx.new_block("while.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&cond_label);

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);

    // For while-loops, continue jumps back to the cond block.
    ctx.loop_targets
        .push((cond_label.clone(), exit_label.clone()));
    let loop_proof_scope_id = ctx.next_loop_proof_scope_id();

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_label = ctx.pending_label.take();
    let previous_region_id = ctx.active_region_id.clone();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    if let Some(fact) = crate::expr::while_condition_range_fact(ctx, condition, loop_proof_scope_id)
    {
        ctx.int_range_facts.push(fact);
    }
    let mut guarded =
        crate::expr::guarded_buffer_indices_for_condition(ctx, condition, loop_proof_scope_id);
    guarded.retain(|fact| !stmts_mutate_local(body, fact.index_local_id));
    ctx.guarded_buffer_index_pairs.extend(guarded);

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();
    ctx.guarded_buffer_index_pairs
        .retain(|fact| fact.scope_id != loop_proof_scope_id);
    ctx.int_range_facts
        .retain(|fact| fact.scope_id != loop_proof_scope_id);

    ctx.current_block = exit_idx;
    Ok(())
}

/// `do { body } while (cond)` — body runs at least once. Same blocks as
/// `while`, but the initial branch goes to body, not cond.
pub(crate) fn lower_do_while(
    ctx: &mut FnCtx<'_>,
    body: &[Stmt],
    condition: &perry_hir::Expr,
) -> Result<()> {
    let body_idx = ctx.new_block("dowhile.body");
    let cond_idx = ctx.new_block("dowhile.cond");
    let exit_idx = ctx.new_block("dowhile.exit");

    let body_label = ctx.block_label(body_idx);
    let cond_label = ctx.block_label(cond_idx);
    let exit_label = ctx.block_label(exit_idx);

    ctx.block().br(&body_label);

    // Push break/continue targets BEFORE compiling the body so nested
    // break/continue see them.
    ctx.loop_targets
        .push((cond_label.clone(), exit_label.clone()));

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_label = ctx.pending_label.take();
    let previous_region_id = ctx.active_region_id.clone();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
        ctx.active_region_id = Some(ctx.region_id_for_label(lbl));
    }

    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: see lower_for for rationale.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.current_block = cond_idx;
    let cv = lower_expr(ctx, condition)?;
    let i1 = lower_truthy(ctx, &cv, condition);
    ctx.block().cond_br(&i1, &body_label, &exit_label);
    ctx.active_region_id = previous_region_id;

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}
