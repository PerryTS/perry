//! `Stmt::Try` lowering — setjmp/longjmp-based exception handling.

use super::*;

/// Try/catch/finally via setjmp/longjmp.
///
/// The CFG pattern:
///   1. Call js_try_push() to get a jmp_buf pointer
///   2. Call setjmp(jmpbuf) — returns 0 on first call, non-0 after longjmp
///   3. Branch: 0 → try_body, non-0 → catch_entry
///   4. try_body runs, calls js_try_end(), branches to finally
///   5. catch_entry calls js_try_end(). With a user `catch`: reads the
///      exception, runs catch, branches to finally. WITHOUT a `catch`
///      (a `try/finally` with no handler): captures the exception, runs
///      a dedicated copy of the finally body, then re-raises via
///      js_throw so the throw propagates instead of being swallowed.
///   6. finally runs (if present), then falls through to merge (only the
///      normal-completion path reaches this merge finally)
/// Emit `js_try_push()` + setjmp in the CURRENT block, branching to
/// `exc_label` on a longjmp (exception) and `normal_label` otherwise.
///
/// CRITICAL: setjmp must carry `returns_twice` on the call site too (not
/// just the declaration). Without it, LLVM -O2 promotes alloca-backed
/// locals to SSA registers and the longjmp return path sees stale
/// pre-setjmp values. The standard `blk.call()` doesn't support call
/// attributes, so the instruction is emitted manually.
///
/// setjmp variant selection — decided by `crate::setjmp_abi` from the
/// compile target's LLVM triple (`ctx.target_triple`), NOT host `cfg!`,
/// so cross-compiles emit the target's ABI. The same `SetjmpAbi` drives
/// the extern declaration in `runtime_decls/strings_part2.rs`, so the
/// call and the prototype can't diverge. See `crate::setjmp_abi` for the
/// per-target rationale (Windows 2-arg `_setjmp`, Apple fast `_setjmp`,
/// plain `setjmp` elsewhere).
///
/// Also used by the async rejection boundary in `stmt/mod.rs`
/// (`lower_async_rejecting_stmts_inner`) — same setjmp, different
/// exception continuation.
pub(super) fn emit_setjmp_dispatch(ctx: &mut FnCtx<'_>, exc_label: &str, normal_label: &str) {
    use crate::types::{I32, PTR};
    let abi = crate::setjmp_abi::setjmp_abi_for_triple(ctx.target_triple);
    let blk = ctx.block();
    let jmpbuf = blk.call(PTR, "js_try_push", &[]);
    let sjr_reg = blk.next_reg();
    blk.emit_raw(abi.call_instruction(&sjr_reg, &jmpbuf));
    let is_exc = blk.icmp_ne(I32, &sjr_reg, "0");
    blk.cond_br(&is_exc, exc_label, normal_label);
}

pub(crate) fn lower_try(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    catch: Option<&perry_hir::CatchClause>,
    finally: Option<&[perry_hir::Stmt]>,
) -> Result<()> {
    // Mark the enclosing function so IR emission adds `#1` (noinline) and
    // runs the setjmp volatile-promotion pass.
    //
    // At -O2 on aarch64, LLVM's mem2reg/SROA would otherwise promote allocas
    // to SSA registers across the setjmp call, and `longjmp` — which restores
    // the callee-saved registers snapshotted by `setjmp` — would revert the
    // mutations the try body made, so the catch block reads stale values.
    // `returns_twice` on the setjmp call site alone is not sufficient.
    //
    // The fix is C's `volatile` rule, not `optnone`: the
    // `enter_try_region`/`exit_try_region` brackets below record every store
    // the try body emits, and `LlFunction::to_ir` gives just those allocas
    // volatile accesses. Everything else in the function — loop counters,
    // arithmetic, compares, branches — stays fully optimizable (#6385).
    ctx.func.has_try = true;

    // Allocate blocks.
    let try_body_idx = ctx.new_block("try.body");
    let catch_idx = ctx.new_block("try.catch");
    let finally_idx = ctx.new_block("try.finally");

    let try_body_label = ctx.block_label(try_body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let finally_label = ctx.block_label(finally_idx);

    // --- current block: setjmp dispatch ---
    emit_setjmp_dispatch(ctx, &catch_label, &try_body_label);

    // --- try body ---
    ctx.current_block = try_body_idx;
    // Track that this try frame is open so any `return` inside the body
    // pops it via `js_try_end` before falling through to the function's
    // ret. Decremented after the body finishes lowering.
    ctx.try_depth += 1;
    // Everything lowered from here on runs between the setjmp above and a
    // possible longjmp into `try.catch`, so its stores must survive that
    // longjmp (#6385).
    ctx.func.enter_try_region();
    lower_stmts(ctx, body)?;
    ctx.func.exit_try_region();
    ctx.try_depth -= 1;
    if !ctx.block().is_terminated() {
        ctx.block().call_void("js_try_end", &[]);
        ctx.block().br(&finally_label);
    }

    // --- catch ---
    ctx.current_block = catch_idx;
    ctx.block().call_void("js_try_end", &[]);
    if let Some(clause) = catch {
        let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
        ctx.block().call_void("js_clear_exception", &[]);
        // Bind the catch param (if any) to the exception value.
        if let Some((id, _name)) = &clause.param {
            // Slot lives in the entry block — a closure inside the
            // catch body may capture the exception binding and get
            // called from a sibling branch that the catch block
            // doesn't dominate.
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.block().store(DOUBLE, &exc, &slot);
        }
        if let Some(f) = finally {
            // Per spec TryStatement : try Block Catch Finally — a throw
            // escaping the CATCH body must still run the finally, whose
            // own abrupt completion (throw) replaces the pending one.
            // Protect the catch body with its own frame: on a longjmp out
            // of it, run a dedicated copy of the finally body, then
            // re-raise the catch's exception (unless the finally itself
            // terminated abruptly — its terminator stands).
            // Refs test262 S12.14_A7_T2/T3, S12.14_A13_T3.
            let cbody_idx = ctx.new_block("try.catch.body");
            let cfail_idx = ctx.new_block("try.catch.fail");
            let cbody_label = ctx.block_label(cbody_idx);
            let cfail_label = ctx.block_label(cfail_idx);
            emit_setjmp_dispatch(ctx, &cfail_label, &cbody_label);

            ctx.current_block = cbody_idx;
            ctx.try_depth += 1;
            // The catch body sits inside its OWN setjmp (the one just emitted):
            // a throw escaping it longjmps to `try.catch.fail`, which re-runs
            // the finally and reads locals. So its stores are also
            // "modified between setjmp and longjmp" (#6385).
            ctx.func.enter_try_region();
            lower_stmts(ctx, &clause.body)?;
            ctx.func.exit_try_region();
            ctx.try_depth -= 1;
            if !ctx.block().is_terminated() {
                ctx.block().call_void("js_try_end", &[]);
                ctx.block().br(&finally_label);
            }

            ctx.current_block = cfail_idx;
            ctx.block().call_void("js_try_end", &[]);
            let exc2 = ctx.block().call(DOUBLE, "js_get_exception", &[]);
            lower_stmts(ctx, f)?;
            if !ctx.block().is_terminated() {
                ctx.block().call_void("js_throw", &[(DOUBLE, &exc2)]);
                ctx.block().unreachable();
            }
        } else {
            lower_stmts(ctx, &clause.body)?;
            if !ctx.block().is_terminated() {
                ctx.block().br(&finally_label);
            }
        }
    } else {
        // No catch clause: this is a `try { ... } finally { ... }`
        // (or a bare `try { ... } finally {}`). The longjmp landed
        // here because the try body threw. ECMAScript requires the
        // finally to run and then the ORIGINAL exception to RE-PROPAGATE
        // — it must NOT be swallowed. Previously this block only did
        // `js_try_end()` + fell through to the shared merge finally and
        // the function returned `undefined`, silently eating the throw.
        //
        // Issue #37 / effect's `internalCall` "forced" path:
        // `try { return body() } finally {}` swallowed body()'s throw,
        // surfacing as `(FiberFailure) Error: {}`.
        //
        // Capture the pending exception BEFORE running finally (the
        // finally body may touch exception state), run a dedicated copy
        // of the finally body on this exception path, then re-raise via
        // js_throw — unless the finally itself completed abruptly (a
        // `return`/`throw` inside finally overrides the pending
        // exception, per spec), in which case its own terminator stands.
        let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
        if let Some(f) = finally {
            lower_stmts(ctx, f)?;
        }
        if !ctx.block().is_terminated() {
            ctx.block().call_void("js_throw", &[(DOUBLE, &exc)]);
            ctx.block().unreachable();
        }
    }

    // --- finally / merge (normal-completion path) ---
    ctx.current_block = finally_idx;
    if let Some(f) = finally {
        lower_stmts(ctx, f)?;
    }
    Ok(())
}
