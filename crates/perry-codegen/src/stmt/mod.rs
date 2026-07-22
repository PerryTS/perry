//! Statement codegen — Phase 2.
//!
//! Supports: Expr, Return(Some|None), If (with/without else), Let. Enough
//! for a recursive fibonacci function plus `console.log(fibonacci(N))` at
//! top level. Loops and Date.now land in Phase 2.1.

use anyhow::{anyhow, bail, Result};
use perry_hir::Stmt;

use crate::expr::{lower_expr, lower_expr_value, materialize_js_value, FnCtx};
use crate::native_value::{LoweredValue, MaterializationReason};
use crate::types::DOUBLE;

mod if_stmt;
mod let_stmt;
mod loops;
mod masked_window_region;
mod switch_stmt;
mod try_stmt;
mod unused_expr;

pub(crate) use if_stmt::lower_if;
pub(crate) use let_stmt::lower_let;
pub(crate) use loops::{lower_do_while, lower_for, lower_while};
pub(crate) use switch_stmt::lower_switch;
pub(crate) use try_stmt::lower_try;

pub(crate) fn record_boxed_slot_js_value_bits(
    ctx: &mut FnCtx<'_>,
    local_id: u32,
    box_ptr: &str,
    consumer: &'static str,
) {
    let lowered = LoweredValue::js_value_bits(box_ptr);
    ctx.record_lowered_value(
        "BoxedLocalSlot",
        Some(local_id),
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec!["raw_box_pointer_carried_as_i64".to_string()],
    );
}

/// Lower a sequence of statements into the current block of `ctx`. If any
/// statement splits control flow, `ctx.current_block` is updated to the
/// "fall-through" block after the split.
pub(crate) fn lower_stmts(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<()> {
    lower_stmts_inner(ctx, stmts, false)
}

/// Lower a user function's top-level statement list and apply the conservative
/// shadow-slot clear plan computed for that exact list. Nested statement lists
/// use `lower_stmts`, so this slice never clears inside loop/branch bodies.
pub(crate) fn lower_top_level_stmts(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<()> {
    lower_stmts_inner(ctx, stmts, true)
}

pub(crate) fn lower_async_rejecting_stmts(ctx: &mut FnCtx<'_>, stmts: &[Stmt]) -> Result<()> {
    lower_async_rejecting_stmts_inner(ctx, stmts, false)
}

pub(crate) fn lower_async_rejecting_top_level_stmts(
    ctx: &mut FnCtx<'_>,
    stmts: &[Stmt],
) -> Result<()> {
    lower_async_rejecting_stmts_inner(ctx, stmts, true)
}

fn lower_async_rejecting_stmts_inner(
    ctx: &mut FnCtx<'_>,
    stmts: &[Stmt],
    emit_shadow_clears: bool,
) -> Result<()> {
    use crate::types::I64;

    // Direct async functions that were not rewritten into generator state
    // machines still need the ECMAScript async boundary: any abrupt
    // completion before the first await rejects the returned Promise instead
    // of escaping as a host exception.
    ctx.func.has_try = true;

    let body_idx = ctx.new_block("async.body");
    let catch_idx = ctx.new_block("async.catch");
    let merge_idx = ctx.new_block("async.merge");

    let body_label = ctx.block_label(body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let merge_label = ctx.block_label(merge_idx);

    // js_try_push + target-ABI setjmp + branch — shared with `lower_try` so
    // the setjmp variant (chosen from `ctx.target_triple`, see
    // `crate::setjmp_abi`) is decided in exactly one place.
    try_stmt::emit_setjmp_dispatch(ctx, &catch_label, &body_label);

    ctx.current_block = body_idx;
    ctx.try_depth += 1;
    // The whole async body runs between the setjmp above and a possible
    // longjmp into `async.catch`. `async.catch` itself only touches runtime
    // state (get/clear exception, reject the promise) and never reads a
    // local, so in principle no alloca needs to survive that longjmp — but we
    // open the region anyway rather than special-case it. The uniform rule
    // ("every alloca stored inside a setjmp-protected region is volatile") is
    // the one that is trivially sound, and this is still strictly better than
    // the `optnone` it replaces: the arithmetic, compares and branches in an
    // async body now optimize even though its locals stay frame-resident.
    ctx.func.enter_try_region();
    lower_stmts_inner(ctx, stmts, emit_shadow_clears)?;
    ctx.func.exit_try_region();
    ctx.try_depth -= 1;
    if !ctx.block().is_terminated() {
        ctx.block().call_void("js_try_end", &[]);
        ctx.block().br(&merge_label);
    }

    ctx.current_block = catch_idx;
    ctx.block().call_void("js_try_end", &[]);
    let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
    let handle = ctx
        .block()
        .call(I64, "js_promise_rejected", &[(DOUBLE, &exc)]);
    ctx.block().call_void("js_clear_exception", &[]);
    let boxed = crate::expr::nanbox_pointer_inline_pub(ctx.block(), &handle);
    ctx.block().ret(DOUBLE, &boxed);

    ctx.current_block = merge_idx;
    Ok(())
}

fn lower_stmts_inner(ctx: &mut FnCtx<'_>, stmts: &[Stmt], emit_shadow_clears: bool) -> Result<()> {
    let mut i = 0;
    while i < stmts.len() {
        // Channel-reduction fusion: detect a length-3-or-4 sequence of
        // `acc[c] += arr[idx + c] * k` accumulator updates and emit a
        // single `<4 x i32>` SIMD multiply-add. The canonical hot shape
        // is image_convolution's blur kernel inner body. Detection is
        // narrow (consecutive integer offsets, identical array, identical
        // factor, distinct integer-stable accumulators) so the fusion
        // won't fire on shapes like `r += a[i]*k1; g += a[i]*k2;`
        // (different factors) or `acc += a[i]*k1; acc += a[i+1]*k2;`
        // (same accumulator).
        //
        // The fusion only fires when the array has a `buffer_data_slot`
        // entry — without the pre-computed data ptr we'd have to derive
        // it inline, which costs the same as the scalar Uint8ArrayGet
        // and gives up the win.
        // Skip the manual `<4 x i32>` channel reduction in functions whose
        // body was expanded by `perry_transform::unroll_static_loops`.
        // After the unroll, `KERNEL[ky+2][kx+2]` constant-folds to integer
        // literals and LLVM has enough info to (a) replace mul-by-1 with
        // no-op, (b) replace mul-by-power-of-2 with a shift, (c) choose
        // its own vectorization shape across the 25-chunk unrolled body.
        // Forcing `<4 x i32>` per chunk pre-commits to a vectorization
        // that fights all three. Image_convolution measured 350-360 ms
        // with manual SIMD vs 310-320 ms without (post-unroll) — a -50 ms
        // savings on the canonical workload.
        //
        // Pre-unroll (no constant-foldable k), the manual reduction is
        // still a 10 ms win (817c4b56) so we keep it as the default
        // fallback for non-unrolled functions.
        if !ctx.was_unrolled {
            if let Some(reduction) =
                crate::expr::try_match_channel_reduction(stmts, i, ctx.integer_locals)
            {
                if ctx.buffer_data_slots.contains_key(&reduction.array_id) {
                    crate::expr::lower_channel_reduction(ctx, &reduction)?;
                    let last_lowered_idx = i + reduction.acc_ids.len() - 1;
                    i += reduction.acc_ids.len();
                    if emit_shadow_clears && !ctx.block().is_terminated() {
                        emit_shadow_clears_after_stmt(ctx, last_lowered_idx);
                    }
                    if ctx.block().is_terminated() {
                        break;
                    }
                    continue;
                }
            }
        }
        // #6750 follow-up: masked-window versioning for straight-line runs
        // of scalar statements (bcryptjs ships `_encipher` fully unrolled —
        // ~130 consecutive `S[l >>> 24]`-shaped reads with no loop for the
        // range-loop tiers to version). Probes the accessed arrays once at
        // region entry and branches into a fast copy whose masked reads are
        // bare inline loads; consumes the whole region on a match.
        if let Some(region) = masked_window_region::try_match_masked_window_region(ctx, &stmts[i..])
        {
            let end = i + region.len;
            masked_window_region::lower_masked_window_region(
                ctx,
                &stmts[i..end],
                i,
                emit_shadow_clears,
                &region,
            )?;
            i = end;
            if ctx.block().is_terminated() {
                break;
            }
            continue;
        }
        lower_stmt(ctx, &stmts[i])?;
        // If an earlier statement already terminated the current block
        // (e.g. return in a straight-line sequence), any following statement
        // would emit dead code. Anvil silently drops these at the block
        // level; we do the same here to avoid tripping LLVM's verifier.
        if ctx.block().is_terminated() {
            break;
        }
        if emit_shadow_clears {
            emit_shadow_clears_after_stmt(ctx, i);
            if ctx.block().is_terminated() {
                break;
            }
        }
        i += 1;
    }
    Ok(())
}

pub(crate) fn emit_shadow_clears_after_stmt(ctx: &mut FnCtx<'_>, stmt_idx: usize) {
    let Some(slots) = ctx.shadow_slot_clears_after_stmt.get(&stmt_idx).cloned() else {
        return;
    };
    emit_shadow_slot_clears(ctx, &slots);
}

pub(crate) fn emit_shadow_slot_clears(ctx: &mut FnCtx<'_>, slots: &[u32]) {
    for &slot_idx in slots {
        crate::expr::emit_shadow_slot_clear(ctx, slot_idx);
    }
}

fn lower_return_expr(ctx: &mut FnCtx<'_>, expr: &perry_hir::Expr) -> Result<String> {
    if let Some(lowered) = lower_expr_value(ctx, expr)? {
        return Ok(materialize_js_value(
            ctx,
            lowered,
            MaterializationReason::ReturnAbi,
        ));
    }
    lower_expr(ctx, expr)
}

pub(crate) fn lower_stmt(ctx: &mut FnCtx<'_>, stmt: &Stmt) -> Result<()> {
    match stmt {
        Stmt::Expr(e) => {
            let prev_discard = ctx.discard_expr_value;
            ctx.discard_expr_value = true;
            let result = lower_expr(ctx, e);
            ctx.discard_expr_value = prev_discard;
            let _ = result?;
            Ok(())
        }

        Stmt::Return(Some(e)) => {
            // Inside an inlined constructor body, an explicit `return <value>`
            // applies spec return-override semantics and yields the `new`
            // expression's value — it must NOT emit a function-level `ret`
            // (that would terminate the ENCLOSING function, e.g. `main`).
            if let Some(target) = ctx.inline_ctor_return.last().cloned() {
                // Store the RAW returned value and branch to the construction
                // completion block. The spec return-override check (object? /
                // derived-primitive TypeError) is applied THERE, not here —
                // it must run as part of [[Construct]] completion, OUTSIDE any
                // `try` in the body, so `try { return 0; } catch {}` in a
                // derived ctor throws uncaught (the catch can't see it).
                let ret_val = lower_expr(ctx, e)?;
                ctx.block().store(DOUBLE, &ret_val, &target.result_slot);
                // Pop any open try frames before leaving the body (mirrors the
                // ordinary `return` path below).
                for _ in 0..ctx.try_depth {
                    ctx.block().call_void("js_try_end", &[]);
                }
                ctx.block().br(&target.after_label);
                return Ok(());
            }
            let v = lower_return_expr(ctx, e)?;
            // Phase E: async functions wrap their return value in
            // js_async_fn_result so callers can await the result. Unlike
            // js_promise_resolved (whose Promise.resolve(p) === p identity
            // is spec for Promise.resolve only), an async fn returning a
            // promise must produce a FRESH promise that adopts the inner
            // via the two-tick thenable job (V8 microtask-hop parity).
            let final_v = if ctx.is_async_fn {
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_async_fn_result", &[(DOUBLE, &v)]);
                crate::expr::nanbox_pointer_inline_pub(blk, &handle)
            } else {
                v
            };
            // Pop any currently-open try frames before returning so the
            // runtime's TRY_DEPTH counter stays balanced. Otherwise an
            // early `return` inside `try { ... }` leaks one frame per
            // call — at 128 the runtime panics with "Try block nesting
            // too deep".
            for _ in 0..ctx.try_depth {
                ctx.block().call_void("js_try_end", &[]);
            }
            ctx.block().ret(DOUBLE, &final_v);
            Ok(())
        }
        Stmt::Return(None) => {
            // Inside an inlined constructor body, a bare `return;` keeps the
            // implicit `this` (the result slot already holds it) and jumps to
            // the shared after-block — never a function-level `ret`.
            if let Some(target) = ctx.inline_ctor_return.last().cloned() {
                for _ in 0..ctx.try_depth {
                    ctx.block().call_void("js_try_end", &[]);
                }
                ctx.block().br(&target.after_label);
                return Ok(());
            }
            // Bare `return;` returns the NaN-boxed `undefined` value
            // (TAG_UNDEFINED). For async functions, wrap it in a
            // resolved promise.
            let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            if ctx.is_async_fn {
                let blk = ctx.block();
                let handle = blk.call(
                    crate::types::I64,
                    "js_promise_resolved",
                    &[(DOUBLE, &undef)],
                );
                let boxed = crate::expr::nanbox_pointer_inline_pub(blk, &handle);
                // Pop open try frames first (see above).
                for _ in 0..ctx.try_depth {
                    ctx.block().call_void("js_try_end", &[]);
                }
                ctx.block().ret(DOUBLE, &boxed);
            } else {
                // Pop open try frames first (see above).
                for _ in 0..ctx.try_depth {
                    ctx.block().call_void("js_try_end", &[]);
                }
                ctx.block().ret(DOUBLE, &undef);
            }
            Ok(())
        }

        Stmt::Let {
            id,
            name,
            init,
            ty,
            mutable,
            ..
        } => lower_let(ctx, *id, name, init.as_ref(), ty, *mutable),

        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => lower_if(ctx, condition, then_branch, else_branch.as_deref()),

        Stmt::For {
            init,
            condition,
            update,
            body,
        } => lower_for(
            ctx,
            init.as_deref(),
            condition.as_ref(),
            update.as_ref(),
            body,
        ),

        // `while (cond) { body }` — same CFG as for-loop without init/update.
        Stmt::While { condition, body } => lower_while(ctx, condition, body),

        // `do { body } while (cond)` — body runs at least once, then cond.
        Stmt::DoWhile { body, condition } => lower_do_while(ctx, body, condition),

        // `break;` — branch to the innermost loop's exit block. The
        // current block becomes terminated; subsequent statements in
        // the same scope are dead code and `lower_stmts` skips them.
        Stmt::Break => {
            let (break_label, target_depth) = ctx
                .loop_targets
                .last()
                .map(|(_c, b, d)| (b.clone(), *d))
                .ok_or_else(|| anyhow!("break statement outside any loop"))?;
            // Pop any `try` frames this break jumps OUT of so the runtime's
            // TRY_DEPTH stays balanced. The loop recorded the try_depth at
            // its entry; any frames opened since (the difference) are escaped
            // by the branch and must be closed first (mirrors Stmt::Return).
            for _ in target_depth..ctx.try_depth {
                ctx.block().call_void("js_try_end", &[]);
            }
            ctx.block().br(&break_label);
            Ok(())
        }

        // `continue;` — branch to the innermost loop's continue target
        // (which is the update block for `for`, the cond block for
        // `while`/`do-while`).
        Stmt::Continue => {
            // Scan outward past switch frames: a switch pushes a loop_targets
            // entry with an EMPTY cont slot (it is a break target only), so
            // `continue` inside a switch must resolve to the innermost real
            // LOOP, not the switch exit (#5989 — see switch_stmt.rs).
            let (cont_label, target_depth) = ctx
                .loop_targets
                .iter()
                .rev()
                .find(|(c, _b, _d)| !c.is_empty())
                .map(|(c, _b, d)| (c.clone(), *d))
                .ok_or_else(|| anyhow!("continue statement outside any loop"))?;
            // Pop try frames escaped by jumping back to the loop header
            // (see Stmt::Break / Stmt::Return for the balancing rationale).
            for _ in target_depth..ctx.try_depth {
                ctx.block().call_void("js_try_end", &[]);
            }
            ctx.block().br(&cont_label);
            Ok(())
        }

        // `switch (disc) { case A: ... case B: ... default: ... }` —
        // lowered as a tower of test/body blocks with explicit fall-through
        // (each body block falls into the next body block, not the next
        // test). `break` inside a case branches to the exit block (we
        // push a (exit, exit) entry onto loop_targets so `break` works
        // even though there's no continue target).
        //
        // Layout for `switch (d) { case A: ...; break; case B: ...; default: ... }`:
        //
        //   <pre>:
        //     %dv = <discriminant>
        //     br test_A
        //   test_A:
        //     %cmp = fcmp oeq %dv, A
        //     br i1 %cmp, body_A, test_B
        //   body_A:
        //     ...
        //     br exit            ; from `break`
        //   test_B:
        //     %cmp = fcmp oeq %dv, B
        //     br i1 %cmp, body_B, body_default
        //   body_B:
        //     ...
        //     br body_default    ; fall-through
        //   body_default:
        //     ...
        //     br exit
        //   exit:
        //
        // Default position is preserved (it goes wherever it appears in
        // source order) — falling-through into the default case from the
        // preceding case is valid JS.
        Stmt::Switch {
            discriminant,
            cases,
        } => lower_switch(ctx, discriminant, cases),

        // Labeled statement: set the pending label so the next loop
        // lowered (for/while/do-while) can register itself in
        // `label_targets` under this name.
        Stmt::Labeled { label, body } => {
            // Stack this label onto any pending ones from an enclosing
            // `Stmt::Labeled` so a chain (`outer: inner: for (...)`) hands the
            // loop *both* labels to register. The loop/switch consumes the
            // whole stack via `mem::take`.
            ctx.pending_labels.push(label.clone());
            lower_stmt(ctx, body)?;
            // If the body wasn't a loop/switch that consumed the pending
            // labels, clear them to avoid leaking into subsequent statements.
            ctx.pending_labels.clear();
            // Clean up the label target now that we've exited the labeled
            // statement's scope.
            ctx.label_targets.remove(label);
            Ok(())
        }
        Stmt::LabeledBreak(label) => {
            let (target, target_depth) =
                if let Some((_cont, brk, depth)) = ctx.label_targets.get(label).cloned() {
                    (brk, depth)
                } else {
                    // Fallback: use innermost loop (for unresolved labels).
                    ctx.loop_targets
                        .last()
                        .map(|(_c, b, d)| (b.clone(), *d))
                        .ok_or_else(|| anyhow!("labeled break '{}' outside any loop", label))?
                };
            // Pop any try frames escaped by this labeled break (the target
            // loop/label may sit outside one or more open `try` frames —
            // e.g. a state-machine suspend `break`s out of the dispatch
            // loop's real try). See Stmt::Break for the rationale.
            for _ in target_depth..ctx.try_depth {
                ctx.block().call_void("js_try_end", &[]);
            }
            ctx.block().br(&target);
            Ok(())
        }
        Stmt::LabeledContinue(label) => {
            let (target, target_depth) =
                if let Some((cont, _brk, depth)) = ctx.label_targets.get(label).cloned() {
                    (cont, depth)
                } else {
                    // Fallback: innermost real LOOP — skip switch frames, whose
                    // cont slot is the empty break-only sentinel (#5989).
                    ctx.loop_targets
                        .iter()
                        .rev()
                        .find(|(c, _b, _d)| !c.is_empty())
                        .map(|(c, _b, d)| (c.clone(), *d))
                        .ok_or_else(|| anyhow!("labeled continue '{}' outside any loop", label))?
                };
            for _ in target_depth..ctx.try_depth {
                ctx.block().call_void("js_try_end", &[]);
            }
            ctx.block().br(&target);
            Ok(())
        }

        // Phase G: real setjmp/longjmp-based exception handling.
        //
        // `throw expr` evaluates the expression, calls js_throw(value)
        // which longjmps to the most recent try block, and emits an
        // LLVM `unreachable` terminator (js_throw never returns).
        //
        // Spec-corner: inside an async function with no enclosing
        // `try { ... }` frame, a thrown value must reject the returned
        // promise instead of propagating as an uncaught exception. The
        // async-to-generator pre-pass bails out on functions whose body
        // contains a capturing closure (very common — any `.then(cb)`,
        // `forEach`, etc.), leaving them as `is_async: true` with no
        // state-machine wrapper. Without this guard, `async function f() {
        // throw new Error("x"); }` would terminate the process instead
        // of producing a rejected promise the caller can `.catch()`.
        Stmt::Throw(expr) => {
            let val = lower_expr(ctx, expr)?;
            if ctx.is_async_fn && ctx.try_depth == 0 {
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_rejected", &[(DOUBLE, &val)]);
                let boxed = crate::expr::nanbox_pointer_inline_pub(blk, &handle);
                blk.ret(DOUBLE, &boxed);
            } else {
                ctx.block().call_void("js_throw", &[(DOUBLE, &val)]);
                ctx.block().unreachable();
            }
            Ok(())
        }

        // Phase G: try/catch/finally via setjmp/longjmp.
        //
        // CFG shape:
        //   <current block>:
        //     %jmpbuf = call ptr @js_try_push()
        //     %sjr    = call i32 @setjmp(ptr %jmpbuf)
        //     %is_exc = icmp ne i32 %sjr, 0
        //     br i1 %is_exc, label %catch_entry, label %try_body
        //
        //   try_body:
        //     <lower try body stmts>
        //     call void @js_try_end()
        //     br label %finally_or_merge
        //
        //   catch_entry:
        //     call void @js_try_end()        ; pop try depth before catch body
        //     %exc = call double @js_get_exception()
        //     call void @js_clear_exception()
        //     <bind catch param to %exc if present>
        //     <lower catch body stmts>
        //     br label %finally_or_merge
        //
        //   finally_or_merge:
        //     <lower finally stmts if present>
        //     <continue>
        //
        // Local variable safety: all locals are alloca-backed (stack slots),
        // not SSA registers, so they survive longjmp without explicit
        // save/restore. This is the key advantage of the alloca+mem2reg
        // strategy used by our LLVM backend.
        Stmt::Try {
            body,
            catch,
            finally,
        } => lower_try(ctx, body, catch.as_ref(), finally.as_deref()),

        // Issue #569: pre-allocate slot+box for hoisted FnDecl ids and any
        // function-body let/const captured by a hoisted closure. Each id
        // gets an alloca'd entry-block slot whose value is a pointer to a
        // `js_box_alloc_bits(undefined_bits)` heap cell. Subsequent `Stmt::Let`s for
        // these ids skip the allocation and only `js_box_set` the init
        // value. `LocalGet` / `LocalSet` / `Update` already route through
        // the box because the id is in `ctx.boxed_vars`.
        Stmt::PreallocateBoxes(ids) => emit_preallocate_boxes(ctx, ids, false),

        // Temporal Dead Zone variant: identical to `PreallocateBoxes` but
        // seeds each JSValue box with the TAG_TDZ sentinel so a
        // read-before-declaration throws a spec ReferenceError. See the HIR
        // `Stmt::PreallocateTdzBoxes` doc and the runtime `js_box_get_bits`
        // choke point.
        Stmt::PreallocateTdzBoxes(ids) => emit_preallocate_boxes(ctx, ids, true),

        // #853: every current `perry_hir::Stmt` variant is matched above.
        // Keep this catch-all so HIR additions land as a clear compile-time
        // diagnostic instead of a silent codegen drop.
        #[allow(unreachable_patterns)]
        other => bail!(
            "perry-codegen Phase B.12: Stmt {} not yet supported",
            stmt_variant_name(other)
        ),
    }
}

fn emit_preallocate_boxes(ctx: &mut FnCtx<'_>, ids: &[u32], tdz: bool) -> Result<()> {
    for id in ids {
        if ctx.locals.contains_key(id) {
            // A previous PreallocateBoxes (or an unusual nesting)
            // already set this up -- skip to keep the existing slot.
            ctx.prealloc_boxes.insert(*id);
            ctx.boxed_vars.insert(*id);
            if tdz {
                ctx.tdz_boxes.insert(*id);
            }
            continue;
        }
        let is_i32_control = crate::expr::is_compiler_private_async_i32_control_local(ctx, *id);
        let is_i1_control = crate::expr::is_compiler_private_async_i1_control_local(ctx, *id);
        let blk = ctx.block();
        let (box_ptr, cell_note) = if is_i32_control {
            (
                blk.call(
                    crate::types::I64,
                    "js_i32_box_alloc",
                    &[(crate::types::I32, "0")],
                ),
                "primitive_i32_control_cell",
            )
        } else if is_i1_control {
            (
                blk.call(
                    crate::types::I64,
                    "js_bool_box_alloc",
                    &[(crate::types::I32, "0")],
                ),
                "primitive_i1_control_cell",
            )
        } else {
            // Seed the JSValue box with TAG_TDZ (Temporal Dead Zone) when
            // requested -- a read before the declaration runs throws a spec
            // ReferenceError via the runtime `js_box_get_bits` choke point.
            // Compiler-private i32/i1 control cells are never TDZ.
            let seed_bits = if tdz {
                crate::nanbox::TAG_TDZ_I64.to_string()
            } else {
                crate::nanbox::TAG_UNDEFINED_I64.to_string()
            };
            (
                blk.call(
                    crate::types::I64,
                    "js_box_alloc_bits",
                    &[(crate::types::I64, &seed_bits)],
                ),
                "jsvalue_box_cell",
            )
        };
        let slot = ctx.func.alloca_entry(crate::types::I64);
        // perry#4926: PreallocateBoxes can sit nested inside an If/Try/Labeled
        // body (e.g. the async state-machine wrapper), so this block's
        // box-pointer store doesn't necessarily dominate every load of the
        // slot. Entry-init the slot to TAG_UNDEFINED so paths that bypass this
        // statement read a defined sentinel instead of `undef` (see the boxed
        // `Stmt::Let` arm in let_stmt.rs). The slot holds a *box pointer*, not
        // the value, so it is TAG_UNDEFINED-initialized in both the TDZ and
        // non-TDZ cases -- the TAG_TDZ sentinel lives in the box cell, not the
        // slot.
        let undef_bits = crate::nanbox::TAG_UNDEFINED_I64.to_string();
        ctx.func
            .entry_allocas_push_store(crate::types::I64, &undef_bits, &slot);
        ctx.block().store(crate::types::I64, &box_ptr, &slot);
        record_boxed_slot_js_value_bits(ctx, *id, &box_ptr, "preallocate_boxes.box_ptr_slot");
        if cell_note != "jsvalue_box_cell" {
            let lowered = LoweredValue::js_value_bits(&box_ptr);
            ctx.record_lowered_value(
                "CompilerPrivateAsyncControlCell",
                Some(*id),
                cell_note,
                &lowered,
                None,
                None,
                None,
                false,
                false,
                Vec::new(),
            );
        }
        ctx.locals.insert(*id, slot);
        ctx.prealloc_boxes.insert(*id);
        ctx.boxed_vars.insert(*id);
        if tdz {
            ctx.tdz_boxes.insert(*id);
        }
        crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
    }
    Ok(())
}

fn stmt_variant_name(s: &Stmt) -> &'static str {
    match s {
        Stmt::Expr(_) => "Expr",
        Stmt::Let { .. } => "Let",
        Stmt::Return(_) => "Return",
        Stmt::If { .. } => "If",
        Stmt::While { .. } => "While",
        Stmt::DoWhile { .. } => "DoWhile",
        Stmt::For { .. } => "For",
        Stmt::Labeled { .. } => "Labeled",
        Stmt::Break => "Break",
        Stmt::Continue => "Continue",
        Stmt::LabeledBreak(_) => "LabeledBreak",
        Stmt::LabeledContinue(_) => "LabeledContinue",
        Stmt::Throw(_) => "Throw",
        Stmt::Try { .. } => "Try",
        Stmt::Switch { .. } => "Switch",
        Stmt::PreallocateBoxes(_) => "PreallocateBoxes",
        Stmt::PreallocateTdzBoxes(_) => "PreallocateTdzBoxes",
    }
}

// Silence the unused-import lint if lower_expr is not directly used here
// (it is used via the `use` above, but rustc's dead-code checker can be
// strict about helpers that only get called transitively).
#[allow(dead_code)]
fn _keep_anyhow_in_scope() -> anyhow::Error {
    anyhow!("")
}
