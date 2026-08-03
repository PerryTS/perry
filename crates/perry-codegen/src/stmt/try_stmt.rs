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

/// Invoke-EH (#7302) counterpart of [`emit_setjmp_dispatch`]: arm the
/// handler (`js_eh_try_push` — savepoints only, no jmp_buf), branch into
/// the protected body, and materialize the unwind-target block(s) that
/// funnel the exception into `exc_label`. Returns the unwind label; the
/// caller pushes it as the EH scope around the protected body so every
/// potentially-throwing call inside carries the unwind edge.
///
/// Two per-triple shapes (same rule as `crate::setjmp_abi`: decided by the
/// TARGET triple, not host `cfg!`):
///
/// - Itanium (Mach-O/ELF): one landing-pad block —
///   `landingpad {ptr,i32} catch ptr null` → `br %exc_label`. The pair is
///   ignored; the thrown value is read back from the runtime's rooted TLS
///   slot via `js_get_exception`, exactly as the setjmp path did.
/// - SEH (windows-msvc): `catchswitch within none [pad] unwind to caller` →
///   `catchpad [ptr @perry_seh_filter]` → `catchret to %exc_label`. The
///   filter matches Perry's `RaiseException` code; foreign SEH exceptions
///   (access violations etc.) keep unwinding past JS handlers, matching the
///   setjmp path (which never caught them either).
///
/// Savepoint restores already ran at throw time (`js_throw`), which is
/// sound because the unwinder skips Rust cleanups just like `longjmp` did
/// (runtime built panic=abort; see `perry-runtime/src/eh.rs`).
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
    if crate::eh_mode::invoke_eh_enabled() {
        return lower_try_invoke(ctx, body, catch, finally);
    }
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
            // #7209: BIND the slot the frame is already sized for.
            //
            // `collect_pointer_typed_locals` assigns the catch parameter an
            // index — it is implicitly `Any`, i.e. pointer-possible — so
            // `js_shadow_frame_enter`'s count already includes it. Nothing ever
            // bound it, so `active[idx]` stayed false and the collector never
            // dereferenced this alloca: the frame was sized for a root that did
            // not exist.
            //
            // Sharper than an ordinary missing root, because
            // `js_clear_exception()` two lines up has already dropped the
            // RUNTIME's reference. From here the exception is reachable only
            // through this alloca, and the catch body is arbitrary user code —
            // so a precise-roots collection can SWEEP it, not merely move it.
            //
            // Emitted here rather than hoisted to entry setup precisely because
            // the slot must not go active before the store: on the non-throwing
            // path this alloca is never written, and an entry-hoisted bind
            // would hand the root-word decoder uninitialized stack bytes. After
            // the store is what `Stmt::Let` does for every ordinary local, and
            // it reuses the RESERVED index rather than growing the frame.
            crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
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

/// Invoke-EH lowering of `Stmt::Try` (#7302). Structurally the same CFG as
/// the setjmp version — the differences are the transport, not the shape:
///
///   1. `js_eh_try_push()` arms the handler (savepoints, no jmp_buf) and the
///      body is entered by a plain branch — no setjmp, no `returns_twice`,
///      no volatile promotion, no `noinline`.
///   2. While the body lowers, its landing-pad label is the active EH scope:
///      every potentially-throwing call becomes an `invoke` unwinding there.
///   3. The landing pad funnels into the same catch-entry sequence the
///      setjmp path used (`js_try_end` → `js_get_exception` →
///      `js_clear_exception`).
///   4. Catch/finally bodies lower under the *enclosing* scope (the inner
///      scope is popped first), so a throw escaping them wires to the outer
///      handler — or leaves the function entirely when there is none. The
///      re-raise sites (`js_throw` after a finally copy) go through the same
///      chokepoint and pick up the correct edge automatically.
pub(crate) fn lower_try_invoke(
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
    // before leaving — same bookkeeping as the setjmp path.
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
        if let Some((id, _name)) = &clause.param {
            // Entry-block slot + shadow-slot bind: identical to the setjmp
            // path (#7209 — after js_clear_exception this alloca is the only
            // root keeping the exception alive, and the bind must follow the
            // store so the root-word decoder never sees uninitialized bytes).
            let slot = ctx.func.alloca_entry(DOUBLE);
            ctx.locals.insert(*id, slot.clone());
            ctx.block().store(DOUBLE, &exc, &slot);
            crate::expr::emit_shadow_slot_bind_for_local(ctx, *id);
        }
        if let Some(f) = finally {
            // Spec: a throw escaping the CATCH body must still run the
            // finally, whose own abrupt completion replaces the pending one.
            // Protect the catch body with its own handler; its landing pad
            // runs a dedicated finally copy and re-raises.
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
        // try/finally with no catch: run the finally copy on the exception
        // path, then RE-RAISE the original exception (it must not be
        // swallowed — issue #37). Capture before the finally body, which may
        // touch exception state; a `return`/`throw` inside the finally
        // overrides the pending exception per spec (its terminator stands).
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
