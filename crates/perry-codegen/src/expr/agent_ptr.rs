//! Read a per-agent runtime pointer from emitted code, without a call where
//! the target allows it (`perry-runtime/src/agent_ptrs.rs`).
//!
//! The runtime keeps a small per-agent block of pointers,
//! `PERRY_AGENT_PTRS` (a `#[thread_local]` array of [`AGENT_PTR_SLOTS`]
//! pointers). How emitted code reaches it depends on the target:
//!
//! * [`AgentPtrAccess::InitialExec`] — ELF executables (Linux, Android;
//!   x86-64 and aarch64): the block is an `external thread_local(initialexec)`
//!   global, so the access is a thread-pointer-relative load (`mov %fs:off`
//!   after linker relaxation on x86-64; `mrs tpidr_el0` + GOT offset on
//!   aarch64). Not used for dylib/staticlib outputs: an image that can be
//!   `dlopen`ed may not fit its initial-exec TLS in the static TLS block.
//! * [`AgentPtrAccess::AppleTsd`] — Apple aarch64: Mach-O has no initial-exec
//!   model (every thread-local access is a TLV thunk call), so the block's
//!   address is read from the runtime's `HotTls` cache through the pthread
//!   TSD fast path (`hot_tls.rs`), at `HOT_TLS_AGENT_PTRS_OFFSET`.
//! * [`AgentPtrAccess::Call`] — everything else (Windows, wasm, arm64_32,
//!   x86-64 Darwin, dylib/staticlib outputs): the runtime accessor.
//!
//! In both inline forms a null slot means "not published yet" and takes the
//! accessor call, which publishes it; so the inline forms and the call are
//! equivalent by construction.

use super::FnCtx;
use crate::types::PTR;

/// Slots in the runtime block. **Must equal `agent_ptrs::AGENT_PTR_SLOTS`.**
pub(crate) const AGENT_PTR_SLOTS: usize = crate::runtime_abi::AGENT_PTR_SLOTS;
/// `tls_hot::HOT_TLS_AGENT_PTRS_OFFSET` (pinned there by `offset_of!`).
const HOT_TLS_AGENT_PTRS_OFFSET: usize = crate::runtime_abi::HOT_TLS_AGENT_PTRS_OFFSET;
/// The block's exported symbol.
pub(crate) const AGENT_PTRS_SYMBOL: &str = "PERRY_AGENT_PTRS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentPtrAccess {
    InitialExec,
    AppleTsd,
    Call,
}

thread_local! {
    /// Whether the module being compiled is linked into an EXECUTABLE (set per
    /// module by `codegen::compile_module`); anything else must not assume
    /// initial-exec TLS.
    static OUTPUT_IS_EXECUTABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn set_output_is_executable(executable: bool) {
    OUTPUT_IS_EXECUTABLE.with(|c| c.set(executable));
}

/// How this compile reaches the block. `PERRY_AGENT_PTR_ACCESS=call` forces
/// the call (A/B knob).
pub(crate) fn agent_ptr_access(ctx: &FnCtx<'_>) -> AgentPtrAccess {
    if std::env::var("PERRY_AGENT_PTR_ACCESS").as_deref() == Ok("call") {
        return AgentPtrAccess::Call;
    }
    let triple = ctx.target_triple;
    let elf = (triple.contains("linux") || triple.contains("android"))
        && (triple.starts_with("x86_64") || triple.starts_with("aarch64"));
    if elf && OUTPUT_IS_EXECUTABLE.with(|c| c.get()) {
        return AgentPtrAccess::InitialExec;
    }
    if super::hot_tls::inline_hot_tls_enabled(ctx) {
        return AgentPtrAccess::AppleTsd;
    }
    AgentPtrAccess::Call
}

/// Emit a load of per-agent pointer `slot`, falling back to `fallback_fn`
/// (a `gc-leaf` runtime accessor `() -> ptr` that also publishes it). Ends
/// in a fresh block where the returned register holds the pointer.
pub(crate) fn emit_agent_ptr(ctx: &mut FnCtx<'_>, slot: usize, fallback_fn: &str) -> String {
    debug_assert!(slot < AGENT_PTR_SLOTS);
    let access = agent_ptr_access(ctx);
    if access == AgentPtrAccess::Call {
        return ctx.block().call(PTR, fallback_fn, &[]);
    }
    let slot_off = (slot * 8).to_string();
    let (fast_pred, fast_val, slow_idx) = match access {
        AgentPtrAccess::InitialExec => {
            let slow_idx = ctx.new_block("agent_ptr.slow");
            let fast_idx = ctx.new_block("agent_ptr.fast");
            let blk = ctx.block();
            let at = blk.gep(
                crate::types::I8,
                &format!("@{AGENT_PTRS_SYMBOL}"),
                &[(crate::types::I64, &slot_off)],
            );
            let val = blk.load(PTR, &at);
            let ok = blk.icmp_ne(PTR, &val, "null");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(slow_idx);
            ctx.block().cond_br(&ok, &fast_label, &slow_label);
            ctx.current_block = fast_idx;
            (ctx.block().label.clone(), val, slow_idx)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "agent_ptr");
            let fast_idx = ctx.new_block("agent_ptr.fast");
            let field = super::hot_tls::hot_tls_field(
                ctx,
                &lookup.hot,
                &HOT_TLS_AGENT_PTRS_OFFSET.to_string(),
            );
            let blk = ctx.block();
            let block_ptr = blk.load(PTR, &field);
            let at = blk.gep(
                crate::types::I8,
                &block_ptr,
                &[(crate::types::I64, &slot_off)],
            );
            let val = blk.load(PTR, &at);
            let ok = blk.icmp_ne(PTR, &val, "null");
            let fast_label = ctx.block_label(fast_idx);
            let slow_label = ctx.block_label(lookup.slow_idx);
            ctx.block().cond_br(&ok, &fast_label, &slow_label);
            ctx.current_block = fast_idx;
            (ctx.block().label.clone(), val, lookup.slow_idx)
        }
        AgentPtrAccess::Call => unreachable!(),
    };
    let join_idx = ctx.new_block("agent_ptr.join");
    let join_label = ctx.block_label(join_idx);
    ctx.block().br(&join_label);
    ctx.current_block = slow_idx;
    let called = ctx.block().call(PTR, fallback_fn, &[]);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&join_label);
    ctx.current_block = join_idx;
    ctx.block()
        .phi(PTR, &[(&fast_val, &fast_pred), (&called, &slow_pred)])
}
