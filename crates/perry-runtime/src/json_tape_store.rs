//! Owner of the JSON tape's backing bytes (#7539).
//!
//! A `LazyArrayHeader` used to be allocated as ONE arena object with its tape
//! copied inline after the header. For the 10 k-record `field_access` fixture
//! that is ~2.4 MB in a single allocation, which puts it over
//! `LARGE_OBJECT_THRESHOLD_BYTES` (16 KB), so `arena_alloc_gc` routed the whole
//! thing straight into the OLD generation with `GC_FLAG_TENURED` set. Old-gen
//! bytes are reclaimable only by a FULL collection, so a tape that dies at the
//! end of its loop iteration still accumulated at ~2.4 MB per parse until
//! `old_reclaim_pressure_due` fired (48 MB absolute / 32 MB growth).
//!
//! Measured at `origin/main` on the `bench_field_access.ts` fixture
//! (`PERRY_GC_TRACE=1`, 53 parses): 19 collections, 9 of them full, **6 of
//! those triggered by `old_gen_bytes`**. With `PERRY_JSON_TAPE=0` the same
//! program runs 14 collections with 5 fulls and only 2 `old_gen_bytes`
//! triggers. The cleanest attribution is `bench.ts` (roundtrip), which never
//! materialises anything: its nursery peaks at 4.1 MB while the OLD generation
//! peaks at **39.6 MB** and fires 5 `old_gen_bytes` fulls — in that program the
//! old generation *is* the tape.
//!
//! So the tape moves out of the GC heap entirely. It is a legitimate side
//! allocation by every test the runtime already applies to `Map`/`Set` entry
//! buffers:
//!
//! * **Pointer-free by construction.** `TapeEntry` is `{ offset: u32, kind:
//!   u8, link: u32 }` — three integer fields, two of which are too narrow to
//!   hold a 48-bit heap address, and the only writer of the region is one
//!   `copy_nonoverlapping` from a `&[TapeEntry]`. It therefore never needs
//!   scanning, marking, or rewriting.
//! * **Uniquely owned.** Exactly one `LazyArrayHeader` references a tape and
//!   nothing else can, so ownership is exact and needs no tracing.
//! * **Immutable and immovable after construction.**
//!
//! Lifetime is the proven Map/Set side-allocation shape, keyed by the owning
//! header's address:
//!
//! * [`register`] at construction, [`release`] when the owner dies.
//! * `GcFinalizeHookKind::LazyArrayTape` covers the non-copying sweeps.
//! * [`finalize_dead_copied_minor_from_space_lazy_arrays`] covers the copying
//!   minor, whose bulk from-space reset skips per-object finalizers.
//! * `GcMoveHookKind::LazyArraySideTables` rekeys an evacuated owner. The
//!   header is ~88 bytes now, so it is born in the NURSERY and the copying
//!   minor really does move it — unlike the old multi-megabyte header, which
//!   was born old and never moved.
//! * [`release_current_thread_lazy_tapes`] at thread teardown.
//!
//! On top of that the owner can disown its tape *deterministically*: once
//! `force_materialize_lazy` installs `materialized`, the tape is provably
//! garbage (every subsequent read goes through the `ArrayHeader`), so
//! `json_tape::release_tape_after_materialize` frees it right there without
//! waiting for any collector. That is the path `field_access` takes after
//! #7537's scan flip, which is why the fix does not depend on GC timing for
//! the workload that motivated it.

use std::alloc::{alloc, dealloc, Layout};
use std::cell::{Cell, RefCell};

use crate::json_tape::TapeEntry;

/// Owned backing store for one tape. Frees on drop.
pub(crate) struct TapeSideAllocation {
    ptr: *mut TapeEntry,
    len: usize,
}

impl TapeSideAllocation {
    #[inline]
    pub(crate) fn byte_len(&self) -> usize {
        tape_layout(self.len).size()
    }
}

impl Drop for TapeSideAllocation {
    fn drop(&mut self) {
        if self.ptr.is_null() || self.len == 0 {
            return;
        }
        unsafe {
            dealloc(self.ptr as *mut u8, tape_layout(self.len));
        }
        self.ptr = std::ptr::null_mut();
        self.len = 0;
    }
}

#[inline]
fn tape_layout(len: usize) -> Layout {
    Layout::array::<TapeEntry>(len.max(1)).expect("tape length overflows a Layout")
}

thread_local! {
    /// `LazyArrayHeader` address -> its tape bytes.
    static TAPE_REGISTRY: RefCell<crate::fast_hash::PtrHashMap<usize, TapeSideAllocation>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Live tape bytes on this thread.
    ///
    /// Deliberately NOT routed through `gc_note_external_side_alloc`. That
    /// counter feeds `external_side_live_bytes()`, which every
    /// `old_reclaim_pressure_due` call site ADDS to old-generation pressure —
    /// correct for a `Map`'s entries buffer, whose owner is typically tenured
    /// so only a full reclaim can free it, and exactly wrong here. A tape's
    /// owner is a nursery object that dies at any minor, and materialization
    /// frees the tape with no collector at all. Counting tape bytes as old-gen
    /// pressure would keep firing the very `old_gen_bytes` full collections
    /// #7539 exists to stop, and the fix would have measured as a no-op.
    static TAPE_LIVE_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Fast "this thread has never built a tape" gate, so the copying minor's
    /// from-space pass and the sweep's dead-owner pass cost a single `Cell`
    /// read on programs that never call `JSON.parse`.
    static TAPE_REGISTRY_NONEMPTY: Cell<bool> = const { Cell::new(false) };
}

/// Allocate and fill a detached copy of `entries`. Uses the Rust global
/// allocator directly — this is deliberately NOT a GC allocation, so it never
/// enters arena/old-gen accounting and never runs a collection.
///
/// Returns a null pointer for an empty tape; callers treat a null tape as
/// "length 0" and never dereference it.
pub(crate) fn allocate(entries: &[TapeEntry]) -> (*mut TapeEntry, TapeSideAllocation) {
    if entries.is_empty() {
        return (
            std::ptr::null_mut(),
            TapeSideAllocation {
                ptr: std::ptr::null_mut(),
                len: 0,
            },
        );
    }
    let layout = tape_layout(entries.len());
    let raw = unsafe { alloc(layout) } as *mut TapeEntry;
    assert!(
        !raw.is_null(),
        "json tape side allocation failed ({} bytes)",
        layout.size()
    );
    // GC_STORE_AUDIT(POINTER_FREE): TapeEntry is offset/kind/link numerics, no heap edges.
    unsafe {
        std::ptr::copy_nonoverlapping(entries.as_ptr(), raw, entries.len());
    }
    let allocation = TapeSideAllocation {
        ptr: raw,
        len: entries.len(),
    };
    note_allocated(allocation.byte_len());
    (raw, allocation)
}

/// Hand ownership of `allocation` to `header_addr`.
///
/// Must be called only once every allocation that could relocate the header is
/// behind us, because the key is the header's address.
pub(crate) fn register(header_addr: usize, allocation: TapeSideAllocation) {
    if allocation.ptr.is_null() || allocation.len == 0 {
        return;
    }
    TAPE_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        assert!(
            !registry.contains_key(&header_addr),
            "lazy array tape registered twice for the same header"
        );
        registry.insert(header_addr, allocation);
    });
    TAPE_REGISTRY_NONEMPTY.with(|c| c.set(true));
}

/// Live tape bytes owned by this thread. Diagnostic/test accounting only — see
/// `TAPE_LIVE_BYTES` for why this is not old-generation pressure.
#[inline]
pub(crate) fn live_bytes() -> usize {
    TAPE_LIVE_BYTES.with(Cell::get)
}

/// Drop the tape owned by `header_addr`, if any. Idempotent: the finalize
/// hook, the copied-minor from-space pass, and the deterministic
/// post-materialize release all funnel through here and any of them may run
/// first.
pub(crate) fn release(header_addr: usize) {
    if !TAPE_REGISTRY_NONEMPTY.with(Cell::get) {
        return;
    }
    let allocation = TAPE_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        let taken = registry.remove(&header_addr);
        if registry.is_empty() {
            TAPE_REGISTRY_NONEMPTY.with(|c| c.set(false));
        }
        taken
    });
    let Some(allocation) = allocation else {
        return;
    };
    note_freed(allocation.byte_len());
    drop(allocation);
}

#[inline]
fn note_allocated(bytes: usize) {
    TAPE_LIVE_BYTES.with(|c| c.set(c.get().saturating_add(bytes)));
}

#[inline]
fn note_freed(bytes: usize) {
    TAPE_LIVE_BYTES.with(|c| c.set(c.get().saturating_sub(bytes)));
}

/// Rekey after the copying minor evacuated an owner.
pub(crate) fn owner_moved(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    if !TAPE_REGISTRY_NONEMPTY.with(Cell::get) {
        return;
    }
    TAPE_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        let Some(allocation) = registry.remove(&old_addr) else {
            // Owner had no tape (empty tape, or already released after
            // materialization) — nothing to rekey.
            return;
        };
        if registry.contains_key(&new_addr) {
            registry.insert(old_addr, allocation);
            panic!("lazy array move destination already owns a tape");
        }
        registry.insert(new_addr, allocation);
    });
}

/// True when this thread has never registered a tape, so the collector's
/// per-cycle passes can skip the registry entirely.
#[inline]
pub(crate) fn registry_is_empty() -> bool {
    !TAPE_REGISTRY_NONEMPTY.with(Cell::get)
}

/// Registered owner addresses matching `is_dead`. Split from the release so
/// the caller can budget-chunk the frees the way the Map/Set sweep does.
pub(crate) fn collect_owners(is_dead: &dyn Fn(usize) -> bool) -> Vec<usize> {
    if registry_is_empty() {
        return Vec::new();
    }
    TAPE_REGISTRY.with(|r| {
        r.borrow()
            .keys()
            .copied()
            .filter(|&addr| is_dead(addr))
            .collect()
    })
}

/// Free every tape this thread still owns (thread teardown).
pub(crate) fn release_current_thread_lazy_tapes() {
    let allocations = TAPE_REGISTRY.with(|r| {
        r.borrow_mut()
            .drain()
            .map(|(_, allocation)| allocation)
            .collect::<Vec<_>>()
    });
    TAPE_REGISTRY_NONEMPTY.with(|c| c.set(false));
    for allocation in allocations {
        note_freed(allocation.byte_len());
        drop(allocation);
    }
}

/// Live tape bytes, cross-checked against the registry.
///
/// At rest the running counter and the registry must agree — they are updated
/// by different code paths (`allocate`/`release` vs `register`/`remove`), and a
/// drift between them would mean either a tape freed while still owned or an
/// entry whose bytes were never accounted.
#[cfg(test)]
pub(crate) fn registered_bytes() -> usize {
    let summed: usize =
        TAPE_REGISTRY.with(|r| r.borrow().values().map(TapeSideAllocation::byte_len).sum());
    assert_eq!(
        summed,
        live_bytes(),
        "tape byte counter drifted from the registry"
    );
    summed
}
