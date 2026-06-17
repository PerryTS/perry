//! Process-global `SharedArrayBuffer` backing store + registry (#4913 Stage 2).
//!
//! A `SharedArrayBuffer` is the one JavaScript value whose bytes must alias the
//! same physical memory across every `perry/thread` agent. Ordinary buffers are
//! thread-local slab / arena allocations whose addresses are only meaningful on
//! the owning thread, and crossing a thread boundary deep-copies them — so they
//! cannot back cross-agent `Atomics` coordination.
//!
//! SAB backing is therefore allocated directly from the global allocator,
//! never freed (matching Perry's "buffers live for the life of the process"
//! model — see `buffer::header`), and recorded in a process-global registry so
//! any thread can:
//!   * recognise a raw pointer as a shared backing store (during cross-thread
//!     serialization, before the missing `GcHeader` would be misread), and
//!   * re-register it in its own thread-local buffer / SAB tables when the
//!     value arrives from another agent.
//!
//! Because the address is a stable, process-wide heap address, an `Atomics`
//! slot inside a SAB has the same absolute byte address on every thread — which
//! is exactly the key the futex wait/notify table ([`crate::atomics_futex`])
//! uses to match a `notify` on one agent with a `wait` parked on another.

use std::alloc::{alloc_zeroed, handle_alloc_error, Layout};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use crate::buffer::BufferHeader;

/// Set of `BufferHeader` addresses that back a `SharedArrayBuffer`.
static SHARED_SAB_REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashSet<usize>> {
    SHARED_SAB_REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Header + data layout for a SAB of `size` data bytes. 8-byte alignment so the
/// data region (which begins immediately after the 8-byte `BufferHeader`) is
/// itself 8-aligned — required for `BigInt64Array` / `Float64` atomic slots.
fn sab_layout(size: u32) -> Layout {
    let total = std::mem::size_of::<BufferHeader>() + size as usize;
    Layout::from_size_align(total, 8).expect("shared SAB layout")
}

/// Allocate a process-global, never-freed `BufferHeader + size` block for a
/// `SharedArrayBuffer`. The returned address is stable for the life of the
/// process and valid (readable / writable) from every thread, so views built
/// over it on different agents alias the same physical bytes.
pub fn alloc_shared_sab(size: u32) -> *mut BufferHeader {
    let layout = sab_layout(size);
    // SAFETY: `layout` has non-zero size (BufferHeader is 8 bytes) and 8-byte
    // alignment. `alloc_zeroed` gives the spec-required zero-initialized bytes.
    let raw = unsafe { alloc_zeroed(layout) };
    if raw.is_null() {
        handle_alloc_error(layout);
    }
    let buf = raw as *mut BufferHeader;
    // SAFETY: `buf` points at a fresh `BufferHeader`-sized-and-aligned block.
    unsafe {
        (*buf).length = size;
        (*buf).capacity = size;
    }
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(buf as usize);
    buf
}

/// True if `addr` is a process-global `SharedArrayBuffer` backing store. Unlike
/// the thread-local `buffer::is_shared_array_buffer`, this answers correctly on
/// every thread — used by the cross-thread serializer to recognise a SAB
/// pointer that has no `GcHeader`.
pub fn is_shared_sab(addr: usize) -> bool {
    registry()
        .lock()
        .map(|r| r.contains(&addr))
        .unwrap_or(false)
}
