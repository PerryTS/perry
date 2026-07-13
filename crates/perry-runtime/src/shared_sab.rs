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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::buffer::BufferHeader;

/// Set of `BufferHeader` addresses that back a `SharedArrayBuffer`.
static SHARED_SAB_REGISTRY: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();

/// Latched true by the first SAB allocation. Mirrors `EXTERNAL_BUFFERS_NONEMPTY`
/// in `buffer::header` and exists for the same reason: `is_shared_sab` sits on
/// two hot paths that run for *every* pointer-shaped value —
/// `buffer::is_registered_buffer`'s final fallback (which JSON.stringify runs
/// per serialized pointer, #6009) and the GC's dead-buffer scan, which probes
/// every registered buffer on every full trace. Without the latch both take the
/// registry mutex on each miss. The overwhelming majority of processes never
/// allocate a `SharedArrayBuffer` at all, so they can answer `false` from a
/// single relaxed atomic load and never touch the lock.
static SHARED_SAB_NONEMPTY: AtomicBool = AtomicBool::new(false);

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
    // Release-store AFTER the insert is visible, so a thread that observes the
    // latch also observes the entry it was latched for.
    SHARED_SAB_NONEMPTY.store(true, Ordering::Release);
    buf
}

/// True if `addr` is a process-global `SharedArrayBuffer` backing store. Unlike
/// the thread-local `buffer::is_shared_array_buffer`, this answers correctly on
/// every thread — used by the cross-thread serializer to recognise a SAB
/// pointer that has no `GcHeader`, and by the GC's dead-buffer scan to refuse
/// to treat one as a collectable GC allocation (see
/// `buffer::header::registered_buffer_is_dead_post_trace`).
pub fn is_shared_sab(addr: usize) -> bool {
    if !SHARED_SAB_NONEMPTY.load(Ordering::Acquire) {
        return false;
    }
    registry()
        .lock()
        .map(|r| r.contains(&addr))
        .unwrap_or(false)
}

/// Snapshot the SAB backing set for one GC dead-buffer scan.
///
/// The scan tests every registered buffer, and calling [`is_shared_sab`] per
/// buffer would take the registry mutex once per buffer per full trace. The set
/// is tiny (a handful of entries even in heavy `Atomics` users) and the GC is
/// stop-the-world here, so lock it once and hand the scan a plain set. `None` —
/// the case for nearly every process — means no SAB was ever allocated and the
/// scan can skip the check entirely without allocating anything.
pub(crate) fn snapshot_shared_sabs() -> Option<HashSet<usize>> {
    if !SHARED_SAB_NONEMPTY.load(Ordering::Acquire) {
        return None;
    }
    registry().lock().ok().map(|r| r.clone())
}

/// Test-only: pretend `addr` is a process-global SAB backing.
///
/// A real backing has no `GcHeader`, so the GC's dead-buffer scan can only
/// mistake it for a collectable object when the malloc metadata preceding the
/// block happens to read as a dead `GC_TYPE_BUFFER` header — a coincidence a
/// test cannot force without writing outside the allocation. Seeding an
/// ordinary GC buffer (whose real header genuinely says "dead") into this
/// registry reproduces the same decision deterministically, so the veto in
/// `buffer::header::registered_buffer_is_dead_post_trace` can be proven rather
/// than assumed. Callers MUST pair this with [`test_unseed_shared_sab`] — the
/// registry is process-global and never cleared.
#[cfg(test)]
pub(crate) fn test_seed_shared_sab(addr: usize) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(addr);
    SHARED_SAB_NONEMPTY.store(true, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_unseed_shared_sab(addr: usize) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&addr);
}
