//! Death pruning for the buffer-identity side tables (#6337).
//!
//! `finalize_collected_dead_buffer` exists to drop "every registry/side-table
//! entry keyed by a dead buffer's address" — its own doc comment cites
//! preventing the #6080 ABA class. But `DATA_VIEW_REGISTRY` and
//! `SHARED_ARRAY_BUFFER_REGISTRY` were never pruned anywhere in the tree: they
//! had zero `.remove`/`.retain` sites. That leaked one permanent entry per
//! `DataView` / SAB-flagged buffer ever created, and let a recycled address
//! (`arena_reset_empty_blocks` resets a fully-empty block's offset to 0 while
//! KEEPING its base pointer) inherit a dead view's identity.
//!
//! The SharedArrayBuffer side has a wrinkle the DataView side does not: a
//! *process-global* SAB backing (`shared_sab::alloc_shared_sab`) is not a GC
//! allocation at all and is never freed, so it must be vetoed as a dead
//! candidate rather than pruned — see
//! `test_seeded_shared_sab_is_never_a_dead_buffer_candidate`.

use super::super::*;
use super::support::*;

fn full_gc() {
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
}

/// Drops a test-seeded `shared_sab` entry even if the test panics — the
/// registry is process-global and never cleared, so a leaked seed would make
/// every later test in this binary treat that address as a live SAB backing.
struct SeededSharedSabGuard(usize);

impl Drop for SeededSharedSabGuard {
    fn drop(&mut self) {
        crate::shared_sab::test_unseed_shared_sab(self.0);
    }
}

/// A dead `DataView`'s `DATA_VIEW_REGISTRY` entry must go. `is_data_view` gates
/// `util.types.isDataView`, `ArrayBuffer.isView`, the `[object DataView]` tag,
/// and the structuredClone / `.slice()` re-marking paths — a fresh Buffer
/// landing on the recycled address would answer to all of them.
#[test]
fn test_dead_data_view_registry_entry_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_data_view(addr);
    assert!(
        crate::buffer::is_data_view(addr),
        "test premise: the view is registered"
    );

    // No roots: dead at the full trace. (Buffers are TENURED old-gen residents,
    // so only a FULL trace can prove them dead.)
    full_gc();

    assert!(
        !crate::buffer::is_data_view(addr),
        "dead buffer must not keep its DataView identity — a recycled address \
         would answer to util.types.isDataView / ArrayBuffer.isView"
    );
}

/// The SAB-flagged buffers that actually die are the arena-allocated copies:
/// `SharedArrayBuffer.prototype.slice` (`object/buffer_dispatch.rs`) and
/// structuredClone (`builtins/globals.rs`) both `buffer_alloc` a fresh
/// `BufferHeader` and re-`mark_as_shared_array_buffer` it. Those are ordinary
/// GC-heap objects whose addresses get recycled.
#[test]
fn test_dead_shared_array_buffer_flag_pruned_on_full_gc() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_shared_array_buffer(addr);
    assert!(
        crate::buffer::is_shared_array_buffer(addr),
        "test premise: the SAB flag is registered"
    );

    full_gc();

    assert!(
        !crate::buffer::is_shared_array_buffer(addr),
        "dead SAB-flagged buffer must not keep its identity — a recycled \
         address would answer to util.types.isSharedArrayBuffer"
    );
}

/// The safety inverse: a LIVE (rooted) view keeps its flag. Full mark-sweep is
/// non-moving, so the rooted buffers keep their addresses.
///
/// `CopyingNurseryTestGuard::new(2)` — not `GcTestIsolationGuard` — because
/// only it pushes the shadow frame that makes `js_shadow_slot_set` an actual
/// root. Under the plain isolation guard the slot writes land in no frame, the
/// buffers stay unreachable, and the test would "pass" for the wrong reason.
#[test]
fn test_live_data_view_and_shared_array_buffer_flags_survive_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(2);

    let view = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_data_view(view);
    let sab = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_shared_array_buffer(sab);

    js_shadow_slot_set(0, ptr_bits(view));
    js_shadow_slot_set(1, ptr_bits(sab));

    full_gc();

    assert!(
        crate::buffer::is_data_view(view),
        "a live (rooted) DataView must keep its registry entry"
    );
    assert!(
        crate::buffer::is_shared_array_buffer(sab),
        "a live (rooted) SAB-flagged buffer must keep its registry entry"
    );
}

/// The leak regression: N views, all references dropped, one full collection,
/// and both registries must DRAIN. A per-address `is_*` probe cannot show this
/// — before the fix the tables grew monotonically for the life of the process,
/// one permanent entry per `DataView` / SAB-flagged buffer ever created.
#[test]
fn test_data_view_and_sab_registries_drain_after_owners_die() {
    let _guard = GcTestIsolationGuard::new();

    const N: usize = 4096;
    let base_views = crate::buffer::test_data_view_registry_len();
    let base_sabs = crate::buffer::test_shared_array_buffer_registry_len();

    for _ in 0..N {
        let view = crate::buffer::buffer_alloc(16) as usize;
        crate::buffer::mark_as_data_view(view);
        let sab = crate::buffer::buffer_alloc(16) as usize;
        crate::buffer::mark_as_shared_array_buffer(sab);
    }

    assert_eq!(
        crate::buffer::test_data_view_registry_len(),
        base_views + N,
        "test premise: every DataView registered"
    );
    assert_eq!(
        crate::buffer::test_shared_array_buffer_registry_len(),
        base_sabs + N,
        "test premise: every SAB-flagged buffer registered"
    );

    // Every one of them is unreachable — no shadow slot, no global root.
    full_gc();

    assert_eq!(
        crate::buffer::test_data_view_registry_len(),
        base_views,
        "DATA_VIEW_REGISTRY must drain when its buffers are swept (it had no \
         .remove site at all before #6337)"
    );
    assert_eq!(
        crate::buffer::test_shared_array_buffer_registry_len(),
        base_sabs,
        "SHARED_ARRAY_BUFFER_REGISTRY must drain when its buffers are swept"
    );
}

/// A process-global `SharedArrayBuffer` backing must NEVER be treated as a
/// collectable GC object.
///
/// `alloc_shared_sab` takes the block straight from `alloc_zeroed`: no
/// `GcHeader`, never freed (that is what lets the bytes alias across
/// `perry/thread` agents, #4913). But `js_shared_array_buffer_new` DOES
/// `register_buffer` it, so it reaches the dead-buffer scan on every full
/// trace, where `try_read_gc_header` would sniff the 8 bytes BEFORE the malloc
/// block and read the allocator's own metadata as a header — one arbitrary byte
/// against `GC_TYPE_BUFFER` (10), the next against the mark/pin/forward bits.
///
/// A chance match declares a LIVE SAB dead, and `finalize_collected_dead_buffer`
/// then runs `view::remove_entries_for_dead_buffer`, which retains on
/// `info.backing != addr` — silently unregistering EVERY live typed-array view
/// over that SAB, i.e. exactly the views cross-agent `Atomics` wait/notify
/// resolve their absolute slot addresses through.
///
/// That coincidence cannot be forced from a test without writing outside the
/// allocation, so seed an ordinary GC buffer — whose real header genuinely says
/// "dead" — into the SAB registry. It reproduces the same decision
/// deterministically: without the veto this buffer is reported dead.
#[test]
fn test_seeded_shared_sab_is_never_a_dead_buffer_candidate() {
    let _guard = GcTestIsolationGuard::new();

    let addr = crate::buffer::buffer_alloc(32) as usize;
    crate::buffer::mark_as_shared_array_buffer(addr);
    crate::shared_sab::test_seed_shared_sab(addr);
    let _seeded = SeededSharedSabGuard(addr);

    // Unrooted, and its real GcHeader is unmarked — the scan would list it.
    let dead = crate::buffer::collect_dead_registered_buffers_post_trace(true);

    assert!(
        !dead.contains(&addr),
        "a process-global SAB backing must never be reported dead: it has no \
         GcHeader and is never freed, and finalizing it would unregister every \
         live typed-array view over it (breaking cross-agent Atomics)"
    );
}

/// End-to-end companion to the veto test: a real `new SharedArrayBuffer(n)`
/// backing survives a full collection with no roots at all, and stays
/// recognisable through every predicate — the thread-local flag, the buffer
/// registry, and the process-global registry the cross-thread serializer and
/// `Atomics` futex keying depend on.
#[test]
fn test_process_global_sab_backing_survives_full_gc_unrooted() {
    let _guard = GcTestIsolationGuard::new();

    let buf = crate::buffer::js_shared_array_buffer_new(64);
    let addr = buf as usize;
    assert!(crate::shared_sab::is_shared_sab(addr));

    // Deliberately unrooted. The backing is never freed, so this must be a
    // no-op for it — a SAB is reachable from other agents, not from this
    // thread's roots.
    full_gc();

    assert!(
        crate::shared_sab::is_shared_sab(addr),
        "the process-global SAB registry must still recognise the backing"
    );
    assert!(
        crate::buffer::is_shared_array_buffer(addr),
        "the SAB must still answer util.types.isSharedArrayBuffer"
    );
    assert!(
        crate::buffer::is_registered_buffer(addr),
        "the SAB must still answer as a registered buffer"
    );
}
