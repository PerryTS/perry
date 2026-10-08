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
//! * [`AgentPtrAccess::WindowsTeb`] — Windows x86-64, any output kind: the
//!   same sequence the compiler emits for a native thread-local, spelled out
//!   because the runtime cannot export the block under a stable name there
//!   (`agent_ptrs.rs`): `gs:[0x58]` (the TEB's `ThreadLocalStoragePointer`)
//!   indexed by the image's `_tls_index` gives this thread's TLS block for
//!   the image, and the block sits at `PERRY_AGENT_PTRS_SECREL` (a `.secrel32`
//!   the runtime emits for its own static) inside it. Emitted code and the
//!   runtime are linked into ONE image, so `_tls_index` is theirs.
//! * [`AgentPtrAccess::Call`] — everything else (wasm, arm64_32, Windows
//!   aarch64, x86-64 Darwin, ELF dylib/staticlib outputs): the runtime
//!   accessor. x86-64 Darwin has no call-free thread-local model (every
//!   Mach-O thread-local access is a TLV thunk call) and the runtime's
//!   pthread-TSD fast path (`HotTls`, the Apple aarch64 route) is built for
//!   aarch64 only.
//!
//! In every inline form a null slot means "not published yet" and takes the
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
    WindowsTeb,
    AppleTsd,
    Call,
}

/// `PERRY_AGENT_PTRS`'s offset in the image's TLS block on Windows x86-64
/// (`agent_ptrs.rs`), and the image's TLS index.
pub(crate) const AGENT_PTRS_SECREL_SYMBOL: &str = "PERRY_AGENT_PTRS_SECREL";
pub(crate) const TLS_INDEX_SYMBOL: &str = "_tls_index";
/// `NT_TIB64`/`TEB64.ThreadLocalStoragePointer`, off `gs`.
const TEB_TLS_POINTER_OFFSET: &str = "88";

thread_local! {
    /// Whether the module being compiled is linked into an EXECUTABLE (set per
    /// module by `codegen::compile_module`); anything else must not assume
    /// initial-exec TLS.
    static OUTPUT_IS_EXECUTABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn set_output_is_executable(executable: bool) {
    OUTPUT_IS_EXECUTABLE.with(|c| c.set(executable));
}

/// Whether thread-locals of an `output_type` image for `triple` may be
/// addressed as fixed offsets from the thread pointer (local-exec): only in
/// an ELF executable, whose TLS block is the static one every thread is
/// created with. A `dlopen`ed image (a plugin, a shared library) must keep
/// the dynamic models. The compile applies it to its own module
/// (`LlModule::use_local_exec_tls`), so the object emitters, which run on
/// other threads, read the model from the IR rather than from shared state.
pub(crate) fn program_tls_is_local_exec(triple: &str, output_type: &str) -> bool {
    elf_triple(triple) && output_type == "executable"
}

fn elf_triple(triple: &str) -> bool {
    (triple.contains("linux") || triple.contains("android"))
        && (triple.starts_with("x86_64") || triple.starts_with("aarch64"))
}

/// How this compile reaches the block. `PERRY_AGENT_PTR_ACCESS=call` forces
/// the call (A/B knob).
pub(crate) fn agent_ptr_access(ctx: &FnCtx<'_>) -> AgentPtrAccess {
    if std::env::var("PERRY_AGENT_PTR_ACCESS").as_deref() == Ok("call") {
        return AgentPtrAccess::Call;
    }
    let triple = ctx.target_triple;
    if elf_triple(triple) && OUTPUT_IS_EXECUTABLE.with(|c| c.get()) {
        return AgentPtrAccess::InitialExec;
    }
    if triple.starts_with("x86_64") && triple.contains("windows") {
        return AgentPtrAccess::WindowsTeb;
    }
    if super::hot_tls::inline_hot_tls_enabled(ctx) {
        return AgentPtrAccess::AppleTsd;
    }
    AgentPtrAccess::Call
}

/// The current value of per-agent pointer `slot`, for a GC-leaf callee that
/// accepts `absent` (a constant operand meaning "not available here") in its
/// place: one initial-exec load in an ELF executable (the slot must never be
/// null there); the `HotTls` read on Apple aarch64, `absent` when the direct
/// TSD path is unavailable or the block is not published; the slot's `gc-leaf`
/// runtime accessor everywhere else. No null test and no fallback call on the
/// inline forms, so a site pays only the read. Ends in the block where the
/// returned register holds the value.
pub(crate) fn emit_agent_ptr_or(ctx: &mut FnCtx<'_>, slot: usize, absent: &str) -> String {
    debug_assert!(slot < AGENT_PTR_SLOTS);
    let slot_off = (slot * 8).to_string();
    let access = agent_ptr_access(ctx);
    match access {
        AgentPtrAccess::InitialExec | AgentPtrAccess::WindowsTeb => {
            let at = emit_slot_addr(ctx, access, &slot_off);
            ctx.block().load(PTR, &at)
        }
        AgentPtrAccess::AppleTsd => {
            let lookup = super::hot_tls::emit_hot_tls_lookup(ctx, "agent_ptr");
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
            let fast_pred = blk.label.clone();
            let join_idx = ctx.new_block("agent_ptr.join");
            let join_label = ctx.block_label(join_idx);
            ctx.block().br(&join_label);
            ctx.current_block = lookup.slow_idx;
            let slow_pred = ctx.block().label.clone();
            ctx.block().br(&join_label);
            ctx.current_block = join_idx;
            ctx.block()
                .phi(PTR, &[(&val, &fast_pred), (absent, &slow_pred)])
        }
        AgentPtrAccess::Call => ctx.block().call(PTR, agent_ptr_accessor(slot), &[]),
    }
}

/// The address of the slot `slot_off` bytes into this thread's block, for the
/// two forms that name the block through the thread pointer (module docs).
pub(crate) fn emit_slot_addr(
    ctx: &mut FnCtx<'_>,
    access: AgentPtrAccess,
    slot_off: &str,
) -> String {
    let blk = ctx.block();
    let block = match access {
        AgentPtrAccess::InitialExec => format!("@{AGENT_PTRS_SYMBOL}"),
        AgentPtrAccess::WindowsTeb => {
            // `mov gs:[0x58]` — a plain load in the x86 `gs` address space
            // (256). Plain, not volatile: it is re-read after every call, and
            // a thread switch happens only inside a call.
            let tls_array = blk.next_reg();
            blk.emit_raw(format!(
                "  {tls_array} = load ptr, ptr addrspace(256) inttoptr (i64 {TEB_TLS_POINTER_OFFSET} to ptr addrspace(256)), align 8"
            ));
            let index = blk.load(crate::types::I32, &format!("@{TLS_INDEX_SYMBOL}"));
            let index = blk.zext(crate::types::I32, &index, crate::types::I64);
            let entry = blk.gep(PTR, &tls_array, &[(crate::types::I64, &index)]);
            let image_block = blk.load(PTR, &entry);
            let secrel = blk.load(crate::types::I32, &format!("@{AGENT_PTRS_SECREL_SYMBOL}"));
            let secrel = blk.zext(crate::types::I32, &secrel, crate::types::I64);
            blk.gep(
                crate::types::I8,
                &image_block,
                &[(crate::types::I64, &secrel)],
            )
        }
        AgentPtrAccess::AppleTsd | AgentPtrAccess::Call => {
            unreachable!("{access:?} does not name the block through the thread pointer")
        }
    };
    blk.gep(crate::types::I8, &block, &[(crate::types::I64, slot_off)])
}

/// The `gc-leaf` runtime accessor of per-agent pointer `slot`.
fn agent_ptr_accessor(slot: usize) -> &'static str {
    match slot {
        crate::runtime_abi::AGENT_PTR_SHAPE_DIR => "perry_shape_dir_cell",
        crate::runtime_abi::AGENT_PTR_CLASS_VALUES => "perry_class_value_dir_cell",
        _ => unreachable!("agent pointer slot {slot} has no accessor"),
    }
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
        AgentPtrAccess::InitialExec | AgentPtrAccess::WindowsTeb => {
            let slow_idx = ctx.new_block("agent_ptr.slow");
            let fast_idx = ctx.new_block("agent_ptr.fast");
            let at = emit_slot_addr(ctx, access, &slot_off);
            let blk = ctx.block();
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
