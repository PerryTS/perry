//! `Stmt::Try` lowering — LLVM `invoke`/`landingpad` exception handling
//! (#7302; SEH funclets on windows-msvc).
//!
//! The CFG pattern:
//!   1. `js_eh_try_push()` arms the handler (savepoint recording only — no
//!      jmp_buf; the runtime's `js_throw` raises through the unwinder and
//!      the frame's unwind tables carry the rest).
//!   2. Branch into the try body. While the body lowers, its landing-pad
//!      label is the active EH scope: every potentially-throwing call the
//!      body emits becomes an `invoke` unwinding there
//!      (`LlBlock::eh_invoke_suffix`).
//!   3. The landing pad funnels into the catch entry, which runs
//!      `js_try_end` → `js_get_exception` → `js_clear_exception` and binds
//!      the catch parameter.
//!   4. Catch/finally bodies lower under the *enclosing* scope, so a throw
//!      escaping them wires to the outer handler — or leaves the function
//!      when there is none. Re-raise sites (`js_throw` after a finally copy)
//!      go through the same call chokepoint and pick up the correct edge
//!      automatically.
//!
//! History: until #7302 this was setjmp/longjmp-based, which required
//! `returns_twice` + `noinline` on every try-containing function plus a
//! volatile-promotion pass over try-mutated allocas (#6385), and made
//! precise moving-GC roots unsound in try functions (a longjmp could skip a
//! statepoint relocation write-back — the motivating defect, #7174).

use super::*;

/// Arm the handler and materialize the unwind-target block(s) that funnel
/// the exception into `exc_label`. Returns the unwind label; the caller
/// pushes it as the EH scope around the protected body.
///
/// Two per-triple shapes (same rule as the old setjmp-ABI selection: decided
/// by the TARGET triple, not host `cfg!`):
///
/// - Itanium (Mach-O/ELF): one landing-pad block —
///   `landingpad {ptr,i32} catch ptr null` → `br %exc_label`. The pair is
///   ignored; the thrown value is read back from the runtime's rooted TLS
///   slot via `js_get_exception`.
/// - SEH (windows-msvc): `catchswitch within none [pad] unwind to caller` →
///   `catchpad [ptr @perry_seh_filter]` → `catchret to %exc_label`. The
///   filter matches Perry's `RaiseException` code; foreign SEH exceptions
///   (access violations etc.) keep unwinding past JS handlers.
///
/// Savepoint restores run at throw time (`js_throw`), which is sound
/// because the unwinder skips Rust cleanups exactly like `longjmp` did (the
/// runtime is built panic=abort with forced unwind tables; see
/// `perry-runtime/src/eh.rs`).
///
/// Also used by the async rejection boundary in `stmt/mod.rs`
/// (`lower_async_rejecting_stmts_inner`) — same dispatch, different
/// exception continuation.
pub(super) fn emit_eh_dispatch(ctx: &mut FnCtx<'_>, exc_label: &str, normal_label: &str) -> String {
    let msvc = ctx.target_triple.contains("-windows-");
    ctx.func.personality = Some(if msvc {
        "__C_specific_handler"
    } else {
        "perry_eh_personality"
    });

    ctx.block().call_void("js_eh_try_push", &[]);

    if msvc {
        let cs_idx = ctx.new_block("eh.cs");
        let pad_idx = ctx.new_block("eh.pad");
        let cs_label = ctx.block_label(cs_idx);
        let pad_label = ctx.block_label(pad_idx);

        ctx.block().br(normal_label);

        let saved = ctx.current_block;
        ctx.current_block = cs_idx;
        let cs = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = catchswitch within none [label %{}] unwind to caller",
            cs, pad_label
        ));
        ctx.block().mark_terminated();

        ctx.current_block = pad_idx;
        let pad = ctx.block().next_reg();
        ctx.block().emit_raw(format!(
            "{} = catchpad within {} [ptr @perry_seh_filter]",
            pad, cs
        ));
        ctx.block()
            .emit_raw(format!("catchret from {} to label %{}", pad, exc_label));
        ctx.block().mark_terminated();
        ctx.current_block = saved;
        cs_label
    } else {
        let lpad_idx = ctx.new_block("eh.lpad");
        let lpad_label = ctx.block_label(lpad_idx);

        ctx.block().br(normal_label);

        let saved = ctx.current_block;
        ctx.current_block = lpad_idx;
        let lp = ctx.block().next_reg();
        ctx.block()
            .emit_raw(format!("{} = landingpad {{ ptr, i32 }} catch ptr null", lp));
        ctx.block().br(exc_label);
        ctx.current_block = saved;
        lpad_label
    }
}

pub(crate) fn lower_try(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    catch: Option<&perry_hir::CatchClause>,
    finally: Option<&[perry_hir::Stmt]>,
) -> Result<()> {
    let try_body_idx = ctx.new_block("try.body");
    let catch_idx = ctx.new_block("try.catch");
    let finally_idx = ctx.new_block("try.finally");

    let try_body_label = ctx.block_label(try_body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let finally_label = ctx.block_label(finally_idx);

    // --- current block: arm handler, enter body; landing pad → catch ---
    let lpad_label = emit_eh_dispatch(ctx, &catch_label, &try_body_label);

    // --- try body (scope active) ---
    ctx.current_block = try_body_idx;
    // Return/break/continue inside the body pop the handler via js_try_end
    // before leaving (see `Stmt::Return` in stmt/mod.rs).
    ctx.try_depth += 1;
    ctx.func.push_eh_scope(lpad_label);
    lower_stmts(ctx, body)?;
    ctx.func.pop_eh_scope();
    ctx.try_depth -= 1;
    if !ctx.block().is_terminated() {
        ctx.block().call_void("js_try_end", &[]);
        ctx.block().br(&finally_label);
    }

    // --- catch (reached only through the landing pad) ---
    ctx.current_block = catch_idx;
    ctx.block().call_void("js_try_end", &[]);
    if let Some(clause) = catch {
        let exc = ctx.block().call(DOUBLE, "js_get_exception", &[]);
        ctx.block().call_void("js_clear_exception", &[]);
        // Bind the catch param (if any) to the exception value.
        if let Some((id, _name)) = &clause.param {
            // Slot lives in the entry block — a closure inside the catch
            // body may capture the exception binding and get called from a
            // sibling branch that the catch block doesn't dominate.
            //
            // #7209: the shadow-slot BIND must follow the store — after
            // js_clear_exception this alloca is the only root keeping the
            // exception alive, and an entry-hoisted bind would hand the
            // root-word decoder uninitialized stack bytes on the
            // non-throwing path.
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.block().store(DOUBLE, &exc, &slot);
            crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
        }
        if let Some(f) = finally {
            // Per spec TryStatement : try Block Catch Finally — a throw
            // escaping the CATCH body must still run the finally, whose own
            // abrupt completion (throw) replaces the pending one. Protect
            // the catch body with its own handler; its landing pad runs a
            // dedicated copy of the finally body, then re-raises the
            // catch's exception (unless the finally itself terminated
            // abruptly — its terminator stands).
            // Refs test262 S12.14_A7_T2/T3, S12.14_A13_T3.
            let cbody_idx = ctx.new_block("try.catch.body");
            let cfail_idx = ctx.new_block("try.catch.fail");
            let cbody_label = ctx.block_label(cbody_idx);
            let cfail_label = ctx.block_label(cfail_idx);
            let cfail_lpad = emit_eh_dispatch(ctx, &cfail_label, &cbody_label);

            ctx.current_block = cbody_idx;
            ctx.try_depth += 1;
            ctx.func.push_eh_scope(cfail_lpad);
            lower_stmts(ctx, &clause.body)?;
            ctx.func.pop_eh_scope();
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
        // No catch clause: `try { ... } finally { ... }`. ECMAScript
        // requires the finally to run and then the ORIGINAL exception to
        // RE-PROPAGATE — it must NOT be swallowed (issue #37: effect's
        // `internalCall` "forced" path). Capture the pending exception
        // BEFORE running finally (the finally body may touch exception
        // state), run a dedicated copy of the finally body on this
        // exception path, then re-raise via js_throw — unless the finally
        // itself completed abruptly (a `return`/`throw` inside finally
        // overrides the pending exception, per spec).
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
