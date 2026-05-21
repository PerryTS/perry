//! `Stmt::For`, `Stmt::While`, `Stmt::DoWhile` lowering and supporting helpers.

use super::*;

use crate::loop_purity::body_needs_asm_barrier;
use crate::lower_conditional::lower_truthy;
use crate::types::I32;

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
    let hoist_classification: Option<(u32, u32)> =
        condition.and_then(|cond| classify_for_length_hoist(cond, body));
    let hoisted_length_arr_id: Option<u32> = hoist_classification.map(|(arr, _)| arr);
    let hoisted_length_slot: Option<String> =
        if let Some((arr_id, counter_id)) = hoist_classification {
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
            ctx.bounded_index_pairs.push((counter_id, arr_id));

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
    // length as i32 so the loop condition can use `icmp slt i32` instead
    // of `fcmp olt double`. This eliminates the float counter fadd +
    // fcmp per iteration — saves ~2 instructions on the inner loop of
    // nested_loops and similar patterns.
    let i32_length_slot: Option<String> = if let Some((_, counter_id)) = hoist_classification {
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
            // SCEV / LoopVectorizer to recognize the induction variable.
            if let Some(bound_slot) = ctx.locals.get(&bound_id).cloned() {
                let bound_dbl = ctx.block().load(DOUBLE, &bound_slot);
                let bound_i32 = ctx.block().fptosi(DOUBLE, &bound_dbl, I32);
                let slot = ctx.func.alloca_entry(I32);
                ctx.block().store(I32, &bound_i32, &slot);
                Some(slot)
            } else {
                None
            }
        } else {
            local_bound_counter_i32_was_fresh = false;
            None
        };

    let cond_idx = ctx.new_block("for.cond");
    let body_idx = ctx.new_block("for.body");
    let update_idx = ctx.new_block("for.update");
    let exit_idx = ctx.new_block("for.exit");

    let cond_label = ctx.block_label(cond_idx);
    let body_label = ctx.block_label(body_idx);
    let update_label = ctx.block_label(update_idx);
    let exit_label = ctx.block_label(exit_idx);

    // Branch from the block holding the init into the cond block.
    ctx.block().br(&cond_label);

    // Cond block — fast i32 path when both counter and length are i32.
    ctx.current_block = cond_idx;
    let used_i32_cond = if let (Some((_, counter_id)), Some(ref len_i32_slot)) =
        (hoist_classification, &i32_length_slot)
    {
        // Existing path: `i < arr.length` with hoisted i32 length.
        if let Some(ctr_i32_slot) = ctx.i32_counter_slots.get(&counter_id).cloned() {
            let ctr = ctx.block().load(I32, &ctr_i32_slot);
            let len = ctx.block().load(I32, len_i32_slot);
            let cmp = ctx.block().icmp_slt(I32, &ctr, &len);
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
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
            ctx.block().cond_br(&cmp, &body_label, &exit_label);
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
            ctx.block().cond_br(&i1, &body_label, &exit_label);
        } else {
            // `for (;;)` — unconditional jump into the body. May be an
            // infinite loop unless the body contains a `break`.
            ctx.block().br(&body_label);
        }
    }

    // Push break/continue targets so nested `break`/`continue` know where
    // to jump. For for-loops, continue runs the update step.
    ctx.loop_targets
        .push((update_label.clone(), exit_label.clone()));

    // If this for-loop has a pending label (from an enclosing Stmt::Labeled),
    // register it so `break label;` / `continue label;` resolve here.
    let consumed_label = ctx.pending_label.take();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (update_label.clone(), exit_label.clone()));
    }

    // Body block.
    ctx.current_block = body_idx;
    lower_stmts(ctx, body)?;
    clear_loop_body_shadow_slots(ctx, body);
    // Issue #74: insert an empty `asm sideeffect` in bodies whose
    // statements are all LLVM-pure (local-only arithmetic, no calls,
    // no heap mutation). Without this, clang -O3's loop-deletion
    // pass folds patterns like `for (let i=0;i<N;i++) sum+=1;` to
    // `sum=N` and eliminates the loop entirely — so two `Date.now()`
    // calls bracketing the loop end up adjacent in the binary and
    // report 0ms wall-clock. The barrier emits zero machine
    // instructions but is opaque to IndVarSimplify.
    if !ctx.block().is_terminated() && body_needs_asm_barrier(body) {
        ctx.block().asm_sideeffect_barrier();
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&update_label);
    }

    // Update block.
    ctx.current_block = update_idx;
    if let Some(update_expr) = update {
        let _ = lower_expr(ctx, update_expr)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&cond_label);
    }

    ctx.loop_targets.pop();

    // Pop the hoisted-length entry so nested loops or sibling loops
    // don't see a stale slot.
    if let Some((_, counter_id)) = hoist_classification {
        ctx.i32_counter_slots.remove(&counter_id);
    }
    if let Some(arr_id) = hoisted_length_arr_id {
        ctx.cached_lengths.remove(&arr_id);
        ctx.bounded_index_pairs.pop();
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
    let _ = i32_local_bound_slot;

    // Exit block — subsequent statements continue here.
    ctx.current_block = exit_idx;
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

/// Inspect a `for` loop's condition expression and body, and return
/// `Some(arr_local_id)` if the loop is the well-known shape
/// `for (let i = ...; i < <arr>.length; ...) { body }` AND the body
/// is provably free of operations that can change `arr.length`.
///
/// The walker also accepts `arr[i] = expr` IndexSets where `i` is the
/// loop counter from the condition — those are guaranteed inbounds
/// (since `i < arr.length`) and therefore can't trigger the realloc
/// slow path that would extend `arr.length`. Without that exception
/// we'd reject the very common `for (let i = 0; i < arr.length; i++)
/// arr[i] = expr` shape, which is the canonical write-array pattern.
pub(crate) fn classify_for_length_hoist(
    cond: &perry_hir::Expr,
    body: &[perry_hir::Stmt],
) -> Option<(u32, u32)> {
    use perry_hir::{CompareOp, Expr};
    let (op, left, right) = match cond {
        Expr::Compare { op, left, right } => (op, left.as_ref(), right.as_ref()),
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
    if !body
        .iter()
        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
    {
        return None;
    }
    Some((arr_id, bounded_idx_id))
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

pub(crate) fn stmt_preserves_array_length(
    s: &perry_hir::Stmt,
    arr_id: u32,
    bounded_idx_id: u32,
) -> bool {
    use perry_hir::Stmt;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => expr_preserves_array_length(e, arr_id, bounded_idx_id),
        Stmt::Return(opt) => opt
            .as_ref()
            .is_none_or(|e| expr_preserves_array_length(e, arr_id, bounded_idx_id)),
        Stmt::Let { init, .. } => init
            .as_ref()
            .is_none_or(|e| expr_preserves_array_length(e, arr_id, bounded_idx_id)),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id)
                && then_branch
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && else_branch.as_ref().is_none_or(|b| {
                    b.iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::While { condition, body } | Stmt::DoWhile { body, condition } => {
            expr_preserves_array_length(condition, arr_id, bounded_idx_id)
                && body
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
        }
        Stmt::For {
            init,
            condition,
            update,
            body,
        } => {
            init.as_ref()
                .is_none_or(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && condition
                    .as_ref()
                    .is_none_or(|e| expr_preserves_array_length(e, arr_id, bounded_idx_id))
                && update
                    .as_ref()
                    .is_none_or(|e| expr_preserves_array_length(e, arr_id, bounded_idx_id))
                && body
                    .iter()
                    .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
        }
        Stmt::Try {
            body,
            catch,
            finally,
        } => {
            body.iter()
                .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                && catch.as_ref().is_none_or(|c| {
                    c.body
                        .iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
                && finally.as_ref().is_none_or(|b| {
                    b.iter()
                        .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::Switch {
            discriminant,
            cases,
        } => {
            expr_preserves_array_length(discriminant, arr_id, bounded_idx_id)
                && cases.iter().all(|c| {
                    c.test
                        .as_ref()
                        .is_none_or(|e| expr_preserves_array_length(e, arr_id, bounded_idx_id))
                        && c.body
                            .iter()
                            .all(|s| stmt_preserves_array_length(s, arr_id, bounded_idx_id))
                })
        }
        Stmt::Labeled { body, .. } => {
            stmt_preserves_array_length(body.as_ref(), arr_id, bounded_idx_id)
        }
        Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => true,
        Stmt::PreallocateBoxes(_) => true,
    }
}

pub(crate) fn expr_preserves_array_length(
    e: &perry_hir::Expr,
    arr_id: u32,
    bounded_idx_id: u32,
) -> bool {
    use perry_hir::{ArrayElement, CallArg, Expr};
    let walk = |sub: &Expr| expr_preserves_array_length(sub, arr_id, bounded_idx_id);
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
            // we accept — it's guaranteed inbounds because the loop
            // condition `i < arr.length` is invariant in this body,
            // and the IndexSet inbounds path doesn't extend the array.
            if let Expr::LocalGet(id) = object.as_ref() {
                if *id == arr_id {
                    if let Expr::LocalGet(idx_id) = index.as_ref() {
                        if *idx_id == bounded_idx_id {
                            return walk(value);
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
        // `i++` / `i--` are fine — `i` stays integer-valued and the
        // for-loop's standard counter pattern depends on this. (We
        // don't try to verify the update direction; if `i--` ever
        // pushes `i` negative the IndexSet's runtime check still
        // catches it via the realloc fallback.)
        Expr::Update { id, .. } => *id != arr_id,
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
        | Expr::MathRound(a) => walk(a),
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

    // Consume pending label (from enclosing Stmt::Labeled).
    let consumed_label = ctx.pending_label.take();
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
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

    ctx.loop_targets.pop();

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
    if let Some(ref lbl) = consumed_label {
        ctx.label_targets
            .insert(lbl.clone(), (cond_label.clone(), exit_label.clone()));
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

    ctx.loop_targets.pop();

    ctx.current_block = exit_idx;
    Ok(())
}
