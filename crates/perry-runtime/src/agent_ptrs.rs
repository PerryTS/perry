//! Per-agent runtime pointers that generated code reads WITHOUT a call.
//!
//! Some runtime state is per agent (per thread) and read on hot emitted paths:
//! the first is the implicit-`this` cell a direct method call binds around
//! the call (`perry-codegen/src/expr/method_site.rs`), which otherwise costs
//! two runtime calls per method call. This block is the one place such
//! pointers live, at fixed slots, so emitted code can load one directly:
//!
//! * ELF executables (Linux, Android; x86-64 and aarch64): an `initialexec`
//!   thread-local access to [`PERRY_AGENT_PTRS`] — `mov %fs:off` (or
//!   `mrs tpidr_el0` + a GOT offset) and one load, no call;
//! * Apple aarch64: the pthread TSD fast path generated code already uses
//!   (`HotTls`, `perry-codegen/src/expr/hot_tls.rs`), whose appended
//!   `agent_ptrs` field holds this block's address — Mach-O has no
//!   initial-exec model, so a direct thread-local access would be a TLV thunk
//!   call;
//! * every other target, and any image that can be `dlopen`ed (a dylib or
//!   staticlib output, where initial-exec TLS may not fit the static TLS
//!   block): the runtime accessor call.
//!
//! A slot holding null means "not published yet" and emitted code takes the
//! accessor call, which publishes it. A slot is only ever written by its own
//! agent, and cleared before the state it points into is freed.

use std::cell::Cell;

/// Slot 1: this agent's implicit-`this` cell. (Slot 0 is reserved.)
pub use crate::codegen_abi::AGENT_PTR_IMPLICIT_THIS;
/// Slots in the block. **Must equal `AGENT_PTR_SLOTS` in
/// `perry-codegen/src/expr/agent_ptr.rs`** (the emitted global's type).
pub use crate::codegen_abi::AGENT_PTR_SLOTS;

#[repr(C)]
pub struct AgentPtrs([Cell<*const u8>; AGENT_PTR_SLOTS]);

/// The per-agent block. Exported under a stable name for generated code;
/// `#[thread_local]` (not `thread_local!`) because generated code must be able
/// to name the TLS symbol itself. Const-initialised and without drop glue, so
/// no destructor is registered and a late read during teardown sees null.
///
/// Not exported on Windows: MSVC targets cannot export TLS across images, so
/// rustc emits a thread-local shim for the static under the same symbol name
/// and a `#[no_mangle]` static fails to build ("symbol `PERRY_AGENT_PTRS` is
/// already defined"). Only ELF executables name it
/// (`perry-codegen/src/expr/agent_ptr.rs`); Windows takes the accessor call.
#[cfg_attr(not(windows), no_mangle)]
#[thread_local]
pub static PERRY_AGENT_PTRS: AgentPtrs =
    AgentPtrs([const { Cell::new(std::ptr::null()) }; AGENT_PTR_SLOTS]);

/// Publish (or clear, with null) one of this agent's pointers.
#[inline]
pub(crate) fn publish(slot: usize, ptr: *const u8) {
    PERRY_AGENT_PTRS.0[slot].set(ptr);
}

/// This thread's block address, for `HotTls::agent_ptrs`.
#[inline]
pub(crate) fn hot_addr() -> *mut u8 {
    &PERRY_AGENT_PTRS as *const AgentPtrs as *mut u8
}

/// The address of this agent's implicit-`this` cell, published into slot
/// [`AGENT_PTR_IMPLICIT_THIS`] for emitted code that binds `this` around a
/// direct method call. A leaf: one TLS read and one store.
#[no_mangle]
pub extern "C" fn perry_implicit_this_cell() -> *const u8 {
    let cell = &crate::tls_hot::hot().implicit_this as *const std::cell::Cell<u64> as *const u8;
    publish(AGENT_PTR_IMPLICIT_THIS, cell);
    cell
}
