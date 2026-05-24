//! Statement codegen — Phase 2.
//!
//! Supports: Expr, Return(Some|None), If (with/without else), Let. Enough
//! for a recursive fibonacci function plus `console.log(fibonacci(N))` at
//! top level. Loops and Date.now land in Phase 2.1.

use anyhow::{anyhow, bail, Result};
use perry_hir::Stmt;

use crate::expr::{lower_expr, FnCtx};
use crate::types::DOUBLE;

mod if_stmt;
mod let_stmt;
mod loops;
mod switch_stmt;
mod try_stmt;

pub(crate) use if_stmt::lower_if;
pub(crate) use let_stmt::lower_let;
pub(crate) use loops::{lower_do_while, lower_for, lower_while};
pub(crate) use switch_stmt::lower_switch;
pub(crate) use try_stmt::lower_try;

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
            let v = lower_expr(ctx, e)?;
            // Phase E: async functions wrap their return value in
            // js_promise_resolved so callers can await the result.
            // If the value is already a promise (e.g. `return
            // Promise.resolve(x)`), js_promise_resolved is a no-op
            // wrap that the caller's await loop unwraps anyway.
            let final_v = if ctx.is_async_fn {
                let blk = ctx.block();
                let handle = blk.call(crate::types::I64, "js_promise_resolved", &[(DOUBLE, &v)]);
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
            let break_label = ctx
                .loop_targets
                .last()
                .map(|(_c, b)| b.clone())
                .ok_or_else(|| anyhow!("break statement outside any loop"))?;
            ctx.block().br(&break_label);
            Ok(())
        }

        // `continue;` — branch to the innermost loop's continue target
        // (which is the update block for `for`, the cond block for
        // `while`/`do-while`).
        Stmt::Continue => {
            let cont_label = ctx
                .loop_targets
                .last()
                .map(|(c, _b)| c.clone())
                .ok_or_else(|| anyhow!("continue statement outside any loop"))?;
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
            ctx.pending_label = Some(label.clone());
            lower_stmt(ctx, body)?;
            // If the body wasn't a loop that consumed the pending label,
            // clear it to avoid leaking into subsequent statements.
            ctx.pending_label = None;
            // Clean up the label target now that we've exited the labeled
            // statement's scope.
            ctx.label_targets.remove(label);
            Ok(())
        }
        Stmt::LabeledBreak(label) => {
            if let Some((_cont, brk)) = ctx.label_targets.get(label).cloned() {
                ctx.block().br(&brk);
            } else {
                // Fallback: use innermost loop (for unresolved labels).
                let target = ctx
                    .loop_targets
                    .last()
                    .map(|(_c, b)| b.clone())
                    .ok_or_else(|| anyhow!("labeled break '{}' outside any loop", label))?;
                ctx.block().br(&target);
            }
            Ok(())
        }
        Stmt::LabeledContinue(label) => {
            if let Some((cont, _brk)) = ctx.label_targets.get(label).cloned() {
                ctx.block().br(&cont);
            } else {
                // Fallback: use innermost loop.
                let target = ctx
                    .loop_targets
                    .last()
                    .map(|(c, _b)| c.clone())
                    .ok_or_else(|| anyhow!("labeled continue '{}' outside any loop", label))?;
                ctx.block().br(&target);
            }
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
        // `js_box_alloc(undefined)` heap cell. Subsequent `Stmt::Let`s for
        // these ids skip the allocation and only `js_box_set` the init
        // value. `LocalGet` / `LocalSet` / `Update` already route through
        // the box because the id is in `ctx.boxed_vars`.
        Stmt::PreallocateBoxes(ids) => {
            for id in ids {
                if ctx.locals.contains_key(id) {
                    // A previous PreallocateBoxes (or an unusual nesting)
                    // already set this up — skip to keep the existing slot.
                    ctx.prealloc_boxes.insert(*id);
                    ctx.boxed_vars.insert(*id);
                    continue;
                }
                let undef =
                    crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
                let blk = ctx.block();
                let box_ptr = blk.call(crate::types::I64, "js_box_alloc", &[(DOUBLE, &undef)]);
                let slot = ctx.func.alloca_entry(DOUBLE);
                let box_as_double = ctx.block().bitcast_i64_to_double(&box_ptr);
                ctx.block().store(DOUBLE, &box_as_double, &slot);
                ctx.locals.insert(*id, slot);
                ctx.prealloc_boxes.insert(*id);
                ctx.boxed_vars.insert(*id);
                crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
            }
            Ok(())
        }

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
    }
}

// Silence the unused-import lint if lower_expr is not directly used here
// (it is used via the `use` above, but rustc's dead-code checker can be
// strict about helpers that only get called transitively).
#[allow(dead_code)]
fn _keep_anyhow_in_scope() -> anyhow::Error {
    anyhow!("")
}
