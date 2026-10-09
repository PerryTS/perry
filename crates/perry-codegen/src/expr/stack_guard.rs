//! #10812: the prologue stack check of a compiled JS body.
//!
//! ```llvm
//!   %limit = load ptr, <agent-pointer slot AGENT_PTR_STACK_LIMIT>
//!   %sp    = call i64 @llvm.read_register.i64(metadata !{!"rsp"})
//!   %deep  = icmp ult i64 %sp, ptrtoint(%limit)
//!   br i1 %deep, label %stack_guard.overflow, label %stack_guard.ok
//! stack_guard.overflow:
//!   call void @js_stack_overflow()      ; cold; throws RangeError
//!   unreachable
//! ```
//!
//! The limit is published per agent by `perry-runtime/src/stack_guard.rs`; a
//! null slot (not published) compares as 0 and never fires. The slot is read
//! inline where the agent-pointer block is (`agent_ptr.rs`: local-exec TLS
//! in ELF executables, the pthread-TSD hot cache on Apple aarch64). Where it
//! would cost a runtime call per function entry, no check is emitted and an
//! overflow still faults as before.
//!
//! On x86-64 in an ELF executable the check is two instructions,
//! `cmp %fs:PERRY_AGENT_PTRS@TPOFF+16, %rsp; jb overflow`: the slot is a
//! local-exec thread-local, so its load folds into the compare as a
//! `%fs:`-relative operand, and the stack pointer is read with
//! `llvm.read_register`, which the compare takes as its register operand
//! directly (an inline-asm `movq %rsp` would cost a register and a move).
//! Neither forces a frame pointer, as `llvm.frameaddress` would.
//!
//! At a body's entry the overflow arm ends in `unreachable` after the call
//! (`js_stack_overflow` is `-> !` in the runtime). Nothing of the body is live
//! there, so its statepoint carries no roots: the arm spills and reloads
//! nothing and does not branch back. A throw at entry abandons the frame
//! before any value of it exists; no `try` region covers an entry, and each
//! caller keeps its own roots at its own call.
//!
//! The check a `preserve_nonecc` recursion clone makes before calling another
//! clone ([`emit_stack_guard_before_clone_call`]) keeps its arm on a path to a
//! return instead (`br` back to the call), and the declaration stays without
//! `noreturn` for it: a block that ends the function without returning stops
//! ShrinkWrap from sinking the clone's prologue past its frameless leaf path
//! (`spec_preserve_none_tests::the_clone_entry_is_shrink_wrapped_frameless`).
//! There the arm sits mid-body, after the clone's frame exists, so its
//! spills are the price of the frameless entry. An entry check has no such
//! trade: a body that carries one sets up its frame at entry either way.

use super::agent_ptr::{agent_ptr_access, AgentPtrAccess};
use super::FnCtx;
use crate::types::{I64, I8, PTR};

const AGENT_PTR_STACK_LIMIT: usize = crate::runtime_abi::AGENT_PTR_STACK_LIMIT;
const HOT_TLS_AGENT_PTRS_OFFSET: usize = crate::runtime_abi::HOT_TLS_AGENT_PTRS_OFFSET;

/// Emit the entry check of a body; lowering continues in a fresh block
/// reached when the frame is above the limit.
///
/// A `preserve_nonecc` recursion clone (#8175) is skipped: its leaf path is
/// shrink-wrapped frameless, and a check whose slow path makes a call would
/// put the frame back on every entry. Its recursion is checked at the call
/// instead ([`emit_stack_guard_before_clone_call`]), where the frame exists.
pub(crate) fn emit_stack_guard(ctx: &mut FnCtx<'_>) {
    if ctx.func.is_preserve_none() {
        return;
    }
    emit_check(ctx, Arm::EndsHere);
}

/// The check before a direct call from one `preserve_nonecc` recursion clone
/// to another (`callee`), standing in for the entry check the callee skips.
pub(crate) fn emit_stack_guard_before_clone_call(ctx: &mut FnCtx<'_>, callee: &str) {
    if ctx.func.is_preserve_none() && ctx.func.reg_counter().callee_preserve_none(callee) {
        emit_check(ctx, Arm::RejoinsTheCall);
    }
}

/// How the overflow arm ends (module docs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arm {
    /// `unreachable`: nothing live across the call (a body's entry).
    EndsHere,
    /// `br` back to the checked call, keeping the clone's leaf path
    /// shrink-wrappable.
    RejoinsTheCall,
}

fn emit_check(ctx: &mut FnCtx<'_>, arm: Arm) {
    if ctx.block().is_terminated() {
        return;
    }
    let sp_register = if ctx.target_triple.starts_with("x86_64") {
        "rsp"
    } else if ctx.target_triple.starts_with("aarch64") || ctx.target_triple.starts_with("arm64") {
        "sp"
    } else {
        return;
    };
    let slot_off = (AGENT_PTR_STACK_LIMIT * 8).to_string();
    let ok_idx;
    let limit = match agent_ptr_access(ctx) {
        AgentPtrAccess::Call => return,
        // The runtime publishes no stack limit on Windows (`stack_bounds` is
        // `None` there), so the slot stays 0 and a check could never fire:
        // emit none, as before the block was reachable inline on Windows.
        AgentPtrAccess::WindowsTeb => return,
        access @ AgentPtrAccess::ExecutableTls => {
            ok_idx = ctx.new_block("stack_guard.ok");
            let at = super::agent_ptr::emit_slot_addr(ctx, access, &slot_off);
            ctx.block().load(PTR, &at)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "stack_guard");
            ok_idx = ctx.new_block("stack_guard.ok");
            // Hot cache not ready yet: skip the check.
            let ok_label = ctx.block_label(ok_idx);
            let fast_idx = ctx.current_block;
            ctx.current_block = lookup.slow_idx;
            ctx.block().br(&ok_label);
            ctx.current_block = fast_idx;
            let field = super::hot_tls::hot_tls_field(
                ctx,
                &lookup.hot,
                &HOT_TLS_AGENT_PTRS_OFFSET.to_string(),
            );
            let blk = ctx.block();
            let block_ptr = blk.load(PTR, &field);
            let at = blk.gep(I8, &block_ptr, &[(I64, &slot_off)]);
            blk.load(PTR, &at)
        }
    };
    let overflow_idx = ctx.new_block("stack_guard.overflow");
    let ok_label = ctx.block_label(ok_idx);
    let overflow_label = ctx.block_label(overflow_idx);
    let blk = ctx.block();
    let sp = blk.next_reg();
    blk.emit_raw(format!(
        "{sp} = call i64 @llvm.read_register.i64(metadata !{{!\"{sp_register}\"}})"
    ));
    let limit = blk.ptrtoint(&limit, I64);
    let deep = blk.icmp_ult(I64, &sp, &limit);
    blk.cond_br(&deep, &overflow_label, &ok_label);
    ctx.current_block = overflow_idx;
    ctx.block().call_void("js_stack_overflow", &[]);
    match arm {
        Arm::EndsHere => ctx.block().unreachable(),
        Arm::RejoinsTheCall => ctx.block().br(&ok_label),
    }
    ctx.current_block = ok_idx;
}
