//! `Stmt::Try` lowering — setjmp/longjmp-based exception handling.

use super::*;

/// Try/catch/finally via setjmp/longjmp.
///
/// The CFG pattern:
///   1. Call js_try_push() to get a jmp_buf pointer
///   2. Call setjmp(jmpbuf) — returns 0 on first call, non-0 after longjmp
///   3. Branch: 0 → try_body, non-0 → catch_entry
///   4. try_body runs, calls js_try_end(), branches to finally
///   5. catch_entry calls js_try_end(), reads exception, runs catch, branches to finally
///   6. finally runs (if present), then falls through to merge
pub(crate) fn lower_try(
    ctx: &mut FnCtx<'_>,
    body: &[perry_hir::Stmt],
    catch: Option<&perry_hir::CatchClause>,
    finally: Option<&[perry_hir::Stmt]>,
) -> Result<()> {
    use crate::types::{I32, PTR};

    // Mark the enclosing function so IR emission adds `#1`
    // (noinline optnone). At -O2 on aarch64, LLVM's mem2reg/SROA will
    // otherwise promote allocas to SSA registers across the setjmp
    // call — making mutations performed in the try body invisible in
    // the catch block after longjmp. `returns_twice` on the setjmp
    // call site alone is not sufficient.
    ctx.func.has_try = true;

    // Allocate blocks.
    let try_body_idx = ctx.new_block("try.body");
    let catch_idx = ctx.new_block("try.catch");
    let finally_idx = ctx.new_block("try.finally");

    let try_body_label = ctx.block_label(try_body_idx);
    let catch_label = ctx.block_label(catch_idx);
    let finally_label = ctx.block_label(finally_idx);

    // --- current block: setjmp dispatch ---
    let blk = ctx.block();
    let jmpbuf = blk.call(PTR, "js_try_push", &[]);
    // CRITICAL: setjmp must carry `returns_twice` on the call site
    // too (not just the declaration). Without it, LLVM -O2 promotes
    // alloca-backed locals to SSA registers and the longjmp return
    // path sees stale pre-setjmp values instead of the try-body's
    // assignments. The standard `blk.call()` doesn't support call
    // attributes, so we emit the instruction manually.
    let sjr_reg = blk.next_reg();
    // setjmp variant selection — must match the declaration in
    // `runtime_decls.rs`. See that file for the rationale; the short
    // version:
    //   - Apple: `_setjmp` (LLVM-IR name) → linker `__setjmp` = fast
    //     variant (skips the sigprocmask / sigaltstack syscalls that
    //     normally cost ~500 ns each on macOS arm64).
    //   - Linux: `setjmp` is already fast — no swap needed.
    //   - Windows: `_setjmp(buf, frame_ptr)` (different ABI).
    if cfg!(target_os = "windows") {
        blk.emit_raw(format!(
            "{} = call i32 @_setjmp(ptr {}, ptr null) #0",
            sjr_reg, jmpbuf
        ));
    } else if cfg!(target_vendor = "apple") {
        blk.emit_raw(format!(
            "{} = call i32 @_setjmp(ptr {}) #0",
            sjr_reg, jmpbuf
        ));
    } else {
        blk.emit_raw(format!("{} = call i32 @setjmp(ptr {}) #0", sjr_reg, jmpbuf));
    }
    let sjr = sjr_reg;
    let is_exc = blk.icmp_ne(I32, &sjr, "0");
    blk.cond_br(&is_exc, &catch_label, &try_body_label);

    // --- try body ---
    ctx.current_block = try_body_idx;
    // Track that this try frame is open so any `return` inside the body
    // pops it via `js_try_end` before falling through to the function's
    // ret. Decremented after the body finishes lowering.
    ctx.try_depth += 1;
    lower_stmts(ctx, body)?;
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
        lower_stmts(ctx, &clause.body)?;
    }
    if !ctx.block().is_terminated() {
        ctx.block().br(&finally_label);
    }

    // --- finally / merge ---
    ctx.current_block = finally_idx;
    if let Some(f) = finally {
        lower_stmts(ctx, f)?;
    }
    Ok(())
}
