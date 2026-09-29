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
//! * Windows x86-64: the native TLS sequence spelled out — `gs:[0x58]`
//!   indexed by the image's `_tls_index`, plus [`PERRY_AGENT_PTRS`]'s offset
//!   in the image's TLS block, published below as `PERRY_AGENT_PTRS_SECREL`;
//! * every other target, and any ELF image that can be `dlopen`ed (a dylib or
//!   staticlib output, where initial-exec TLS may not fit the static TLS
//!   block): the runtime accessor call.
//!
//! A slot holding null means "not published yet" and emitted code takes the
//! accessor call, which publishes it. A slot is only ever written by its own
//! agent, and cleared before the state it points into is freed.

use std::cell::Cell;

/// Slot 1: this agent's implicit-`this` cell.
pub use crate::codegen_abi::AGENT_PTR_IMPLICIT_THIS;
/// Slot 0: the address of this agent's ordinary shape-directory mirror.
pub use crate::codegen_abi::AGENT_PTR_SHAPE_DIR;
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
/// (`perry-codegen/src/expr/agent_ptr.rs`); Windows x86-64 reaches it through
/// `PERRY_AGENT_PTRS_SECREL` instead (below).
#[cfg_attr(not(windows), no_mangle)]
#[thread_local]
pub static PERRY_AGENT_PTRS: AgentPtrs = AgentPtrs({
    let mut slots = [const { Cell::new(std::ptr::null()) }; AGENT_PTR_SLOTS];
    // The shape-directory slot is never null: until this agent publishes its
    // own mirror it names the shared empty directory, so the generic-read
    // miss front reads it without a null test.
    slots[AGENT_PTR_SHAPE_DIR] =
        Cell::new(std::ptr::addr_of!(crate::object::shapes::PERRY_EMPTY_SHAPE_DIR) as *const u8);
    slots
});

// Windows x86-64: `PERRY_AGENT_PTRS`'s section-relative offset in this image's
// TLS block (`.tls$`), as a 4-byte constant under a stable name. The static
// itself cannot be exported there (above), but `sym` names it under whatever
// symbol rustc gave it, and `.secrel32` is exactly the relocation rustc's own
// access to it uses (`mov _tls_index; mov gs:[0x58]; mov [..+idx*8];
// sym@SECREL32`). Generated code performs that same sequence with this
// constant, so it reads the block without a call
// (`perry-codegen/src/expr/agent_ptr.rs`, `AgentPtrAccess::WindowsTeb`).
#[cfg(all(windows, target_arch = "x86_64"))]
core::arch::global_asm!(
    ".section .rdata,\"dr\"",
    ".globl PERRY_AGENT_PTRS_SECREL",
    ".p2align 2",
    "PERRY_AGENT_PTRS_SECREL:",
    ".secrel32 {agent_ptrs}",
    ".text",
    agent_ptrs = sym PERRY_AGENT_PTRS,
);

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

/// The address of this agent's ordinary shape-directory mirror
/// (`ShapeSlab::ordinary_dir_addr`), published into slot
/// [`AGENT_PTR_SHAPE_DIR`]. A generic read site passes it to its GC-leaf miss
/// front (`read_confirm::js_object_get_field_ic_front`) so the front reads no
/// thread-local; this is the accessor for targets where emitted code cannot
/// read the block inline. The slab also publishes the slot whenever it
/// publishes its directory, so the inline reads see it once any shape exists
/// on this agent. The address is stable for the thread's life, so the slot is
/// never cleared; the mirror it names is. A leaf: one TLS read and one store.
#[no_mangle]
pub extern "C" fn perry_shape_dir_cell() -> *const u8 {
    let dir = crate::object::shapes::ordinary_dir_addr();
    publish(AGENT_PTR_SHAPE_DIR, dir);
    dir
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
