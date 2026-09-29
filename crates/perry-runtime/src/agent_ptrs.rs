//! Per-agent runtime pointers that generated code reads WITHOUT a call.
//!
//! Some runtime state is per agent (per thread) and read on hot emitted paths.
//! This block is the one place such pointers live, at fixed slots, so emitted
//! code can load one directly. No slot is published today: slot 1 held the
//! implicit-`this` cell until this-as-a-parameter deleted it, and slot 0 is
//! reserved for the megamorphic follow-up's shape-record directory.
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

/// Slots in the block. **Must equal `AGENT_PTR_SLOTS` in
/// `perry-codegen/src/expr/agent_ptr.rs`** (the emitted global's type).
pub use crate::codegen_abi::AGENT_PTR_SLOTS;
/// Slot 2: this agent's stack limit (`stack_guard`, #10812).
pub use crate::codegen_abi::AGENT_PTR_STACK_LIMIT;

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
#[allow(dead_code)] // no slot is published today (see the module doc)
#[inline]
pub(crate) fn publish(slot: usize, ptr: *const u8) {
    PERRY_AGENT_PTRS.0[slot].set(ptr);
}

/// Read one of this agent's pointers.
#[inline]
pub(crate) fn read(slot: usize) -> *const u8 {
    PERRY_AGENT_PTRS.0[slot].get()
}

/// This thread's block address, for `HotTls::agent_ptrs`.
#[inline]
pub(crate) fn hot_addr() -> *mut u8 {
    &PERRY_AGENT_PTRS as *const AgentPtrs as *mut u8
}
