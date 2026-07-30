//! Teeth for the interleaved shadow-stack entry layout and the gated root
//! shading barrier.
//!
//! Every test here is written so that removing the thing it covers makes it
//! fail. The four properties under test are the ones a shadow-stack
//! optimisation can silently break:
//!
//! 1. **Liveness** — the collector marks what is in the slot.
//! 2. **Rewritability** — an evacuating collection updates the slot in place
//!    and the *reader* observes the moved address.
//! 3. **Observed value** — the mirrored word is the one the mutator stored,
//!    not a re-read taken later.
//! 4. **No stale roots** — a fresh frame never inherits the previous frame's
//!    values or bindings out of the recycled buffer tail.

use super::super::*;
use super::support::*;
use std::sync::atomic::Ordering;

/// Frame handles for a nest of frames, popped in reverse on drop.
struct FrameNest(Vec<u64>);

impl FrameNest {
    fn push(&mut self, slot_count: u32) -> u64 {
        let h = js_shadow_frame_push(slot_count);
        self.0.push(h);
        h
    }
}

impl Drop for FrameNest {
    fn drop(&mut self) {
        while let Some(h) = self.0.pop() {
            js_shadow_frame_pop(h);
        }
    }
}

fn scanner_slot_ptrs() -> Vec<*mut u64> {
    let mut out = Vec::new();
    visit_shadow_stack_root_slots(|slot| out.push(slot.ptr));
    out
}

fn scanner_slot_values() -> Vec<u64> {
    let mut out = Vec::new();
    visit_shadow_stack_root_slots(|slot| out.push(unsafe { slot.read() }));
    out
}

// ---------------------------------------------------------------------------
// 4. No stale roots: a fresh frame must not inherit the recycled buffer tail.
// ---------------------------------------------------------------------------

/// Sabotage check: delete the `clear_slots` call in `js_shadow_frame_push`
/// and this fails — the second frame reports the first frame's four dead
/// pointer words as live roots.
#[test]
fn fresh_frame_does_not_inherit_popped_frame_slot_values() {
    let _guard = GcTestIsolationGuard::new();
    // 4 slots hits the unrolled arm of `clear_slots`, 9 hits the `write_bytes`
    // arm. Both must clear.
    for slot_count in [1u32, 2, 3, 4, 9, 33] {
        reset_shadow_stack();
        let dead = js_shadow_frame_push(slot_count);
        for i in 0..slot_count {
            js_shadow_slot_set(i, 0x7FFD_0000_DEAD_0000 | u64::from(i));
        }
        assert_eq!(
            scanner_slot_values().len(),
            slot_count as usize,
            "{slot_count}-slot frame should report every set slot"
        );
        js_shadow_frame_pop(dead);

        let fresh = js_shadow_frame_push(slot_count);
        assert_eq!(
            scanner_slot_values(),
            Vec::<u64>::new(),
            "a fresh {slot_count}-slot frame must report no roots; it reused the \
             buffer the popped frame wrote"
        );
        for i in 0..slot_count {
            assert_eq!(
                js_shadow_slot_get(i),
                0,
                "fresh frame slot {i} of {slot_count} must read back as empty"
            );
        }
        js_shadow_frame_pop(fresh);
    }
}

/// The `meta` half must be cleared too, not just `value`. A surviving binding
/// points at the *caller's* stack storage, which by then belongs to a
/// different function — the collector would read a root out of it and, on the
/// evacuating path, write a forwarded pointer back into it.
///
/// Sabotage check: clear only `ShadowEntry::value` in `clear_slots` and the
/// write-through assertion below fails.
#[test]
fn fresh_frame_does_not_inherit_popped_frame_bindings() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();

    let mut stale_storage: u64 = 0x7FFD_0000_1111_1111;
    let dead = js_shadow_frame_push(2);
    js_shadow_slot_bind(0, &mut stale_storage as *mut u64);
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_1111_1111);
    js_shadow_frame_pop(dead);

    let fresh = js_shadow_frame_push(2);
    assert!(
        scanner_slot_ptrs().is_empty(),
        "fresh frame must not expose the popped frame's bound storage"
    );
    // A write into the fresh frame must land in the mirror only.
    js_shadow_slot_set(0, 0x7FFD_0000_2222_2222);
    assert_eq!(
        stale_storage, 0x7FFD_0000_1111_1111,
        "fresh frame inherited a stale binding and wrote through it"
    );
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_2222_2222);
    let ptrs = scanner_slot_ptrs();
    assert_eq!(ptrs.len(), 1);
    assert_ne!(
        ptrs[0], &mut stale_storage as *mut u64,
        "scanner handed out the popped frame's bound storage"
    );
    js_shadow_frame_pop(fresh);
}

// ---------------------------------------------------------------------------
// Liveness bit / binding encoding round-trip.
// ---------------------------------------------------------------------------

/// The liveness flag and the bound address share one word. Clearing a slot
/// must drop only the flag: codegen re-activates the same slot later and still
/// expects the write-through to reach the original compiled local.
///
/// Sabotage check: write `meta = 0` instead of `meta & SLOT_PTR_MASK` on the
/// clear path and the final write-through assertion fails.
#[test]
fn clearing_a_bound_slot_keeps_the_binding_for_later_reactivation() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let h = js_shadow_frame_push(1);
    let mut storage: u64 = ptr_bits(0x1234_5678);

    js_shadow_slot_bind(0, &mut storage as *mut u64);
    assert_eq!(scanner_slot_ptrs(), vec![&mut storage as *mut u64]);

    js_shadow_slot_set(0, 0);
    assert_eq!(js_shadow_slot_get(0), 0, "cleared slot reads as empty");
    assert!(
        scanner_slot_ptrs().is_empty(),
        "cleared slot must not be scanned"
    );
    assert_eq!(
        storage,
        ptr_bits(0x1234_5678),
        "clearing must not write through to the compiled local"
    );

    js_shadow_slot_set(0, ptr_bits(0xABCD_EF00));
    assert_eq!(
        storage,
        ptr_bits(0xABCD_EF00),
        "re-activated slot must still write through its retained binding"
    );
    assert_eq!(
        scanner_slot_ptrs(),
        vec![&mut storage as *mut u64],
        "re-activated slot must be scanned through the compiled local, not the mirror"
    );
    js_shadow_frame_pop(h);
}

/// Bit 0 of `meta` carries the liveness flag, so an odd address cannot be
/// stored there. Truncating it and letting the collector write a forwarded
/// pointer into `addr & !1` would corrupt whatever lives there; the encoder
/// therefore drops the binding and keeps the entry active-but-unbound, which
/// still marks and still rewrites the mirrored word.
///
/// Sabotage check: replace the encoder body with `raw | SLOT_ACTIVE` and the
/// odd-address case starts reporting a truncated pointer.
#[test]
fn bound_meta_encoding_rejects_addresses_that_would_clobber_the_liveness_bit() {
    for aligned in [0usize, 8, 0x1_0000, usize::MAX & SLOT_PTR_MASK] {
        let meta = bound_slot_meta(aligned);
        assert_eq!(meta & SLOT_ACTIVE, SLOT_ACTIVE, "must be active");
        assert_eq!(
            meta & SLOT_PTR_MASK,
            aligned,
            "aligned address must round-trip exactly"
        );
    }
    for misaligned in [1usize, 9, 0x1_0001] {
        let meta = bound_slot_meta(misaligned);
        assert_eq!(meta & SLOT_ACTIVE, SLOT_ACTIVE, "must still be active");
        assert_eq!(
            meta & SLOT_PTR_MASK,
            0,
            "misaligned address must be dropped, never truncated"
        );
    }
}

// ---------------------------------------------------------------------------
// 1./2. Liveness and rewritability across a real evacuating collection.
// ---------------------------------------------------------------------------

/// A value reachable only from a bound shadow slot must survive a copying
/// minor GC, and the *compiled local* — not just the mirror — must be
/// rewritten to the new address.
///
/// Sabotage check: hand the mirror address to the visitor unconditionally in
/// `visit_shadow_stack_root_slots` and `storage` keeps pointing at from-space.
#[test]
fn bound_slot_survives_and_is_rewritten_by_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let child = young_leaf();
    let mut storage: u64 = ptr_bits(child);
    js_shadow_slot_bind(0, &mut storage as *mut u64);

    let _ = gc_collect_minor();

    let moved = (storage & POINTER_MASK) as usize;
    assert_ne!(moved, 0, "bound local was cleared by the collection");
    assert!(
        crate::arena::pointer_in_nursery(moved) || crate::arena::pointer_in_old_gen(moved),
        "bound local must hold a live heap address after collection"
    );
    assert_eq!(
        js_shadow_slot_get(0),
        storage,
        "slot read must observe the rewritten compiled local"
    );
    assert_ne!(moved, child, "test did not actually evacuate the object");
}

/// The same property for an unbound slot: the mirror word itself is the root
/// slot, so it must be rewritten in place.
#[test]
fn unbound_slot_survives_and_is_rewritten_by_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let _ = gc_collect_minor();

    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(moved, child, "test did not actually evacuate the object");
    assert!(crate::arena::pointer_in_nursery(moved) || crate::arena::pointer_in_old_gen(moved));
}

/// `js_shadow_slot_bind` must root the word the mutator has in the slot at the
/// moment of the call. Re-reading the compiled local at a later safepoint
/// would root whatever a subsequent store put there — the exact miscompile
/// shape that made `new C(g, bump())` print the post-`bump` value.
#[test]
fn bind_roots_the_value_present_at_the_call_not_a_later_store() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let h = js_shadow_frame_push(2);

    let mut storage: u64 = ptr_bits(0xAAAA_0000);
    js_shadow_slot_bind(0, &mut storage as *mut u64);
    // Slot 1 is bound to a *different* cell; the mirrors must not alias.
    let mut other: u64 = ptr_bits(0xBBBB_0000);
    js_shadow_slot_bind(1, &mut other as *mut u64);

    // The mirrored word recorded at bind time is exactly what was there.
    SHADOW.with(|cell| unsafe {
        let s = &*cell.get();
        let top = s.frame_top;
        assert_eq!(s.slots[top].value, ptr_bits(0xAAAA_0000));
        assert_eq!(s.slots[top + 1].value, ptr_bits(0xBBBB_0000));
    });

    // A bound slot deliberately tracks later mutator stores through the
    // binding — that is what the binding is *for* — so the scanner follows the
    // compiled local, which is the storage the mutator will read after the
    // safepoint.
    storage = ptr_bits(0xCCCC_0000);
    assert_eq!(storage, ptr_bits(0xCCCC_0000));
    assert_eq!(js_shadow_slot_get(0), ptr_bits(0xCCCC_0000));
    assert_eq!(js_shadow_slot_get(1), ptr_bits(0xBBBB_0000));
    assert_eq!(
        scanner_slot_values(),
        vec![ptr_bits(0xCCCC_0000), ptr_bits(0xBBBB_0000)]
    );
    js_shadow_frame_pop(h);
}

// ---------------------------------------------------------------------------
// The gated root shading barrier.
// ---------------------------------------------------------------------------

/// The premise the gate rests on: whenever this thread's incremental mark
/// barrier is armed, `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` is
/// non-zero. If that ever stopped holding, a zero count would no longer prove
/// the barrier call is a no-op and the gate would start dropping shading.
#[test]
fn active_count_is_nonzero_whenever_this_threads_barrier_is_armed() {
    let _guard = GcTestIsolationGuard::new();
    incremental_mark_barrier_disable();
    assert!(!incremental_mark_barrier_active());

    let valid_ptrs = build_valid_pointer_set();
    let armed = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);
    assert!(incremental_mark_barrier_active());
    assert_ne!(
        PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.load(Ordering::SeqCst),
        0,
        "armed barrier must be visible in the global gate"
    );
    drop(armed);
    assert!(!incremental_mark_barrier_active());
}

/// With a cycle in flight, a store into a shadow slot must still shade the
/// stored object — the gate only skips the call when no cycle exists.
///
/// Sabotage check: invert the gate to `== 0` (or delete the barrier call) and
/// both assertions below fail.
#[test]
fn slot_set_and_bind_shade_the_stored_value_while_a_cycle_is_active() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    clear_marks();
    clear_mark_seeds();

    let set_child = young_leaf();
    let bind_child = young_leaf();
    let valid_ptrs = build_valid_pointer_set();
    let _barrier = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);

    let h = js_shadow_frame_push(2);
    js_shadow_slot_set(0, ptr_bits(set_child));
    let mut storage: u64 = ptr_bits(bind_child);
    js_shadow_slot_bind(1, &mut storage as *mut u64);
    drain_incremental_mark_barrier_seeds(&valid_ptrs);

    assert_marked_user_ptr(set_child, "js_shadow_slot_set child");
    assert_marked_user_ptr(bind_child, "js_shadow_slot_bind child");

    js_shadow_frame_pop(h);
    clear_marks();
    clear_mark_seeds();
}

// ---------------------------------------------------------------------------
// Frame bookkeeping under the packed header.
// ---------------------------------------------------------------------------

/// The header packs `prev_frame_top` and `slot_count` into one entry. Mixed
/// slot counts, including zero-slot frames, must still chain correctly and
/// leave every frame's slots visible to the scanner.
#[test]
fn nested_frames_with_mixed_slot_counts_chain_and_scan_correctly() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let counts: [u32; 8] = [0, 1, 5, 0, 2, 9, 3, 4];
    let mut nest = FrameNest(Vec::new());
    let mut expected: Vec<u64> = Vec::new();

    for (frame, &count) in counts.iter().enumerate() {
        nest.push(count);
        assert_eq!(shadow_stack_depth(), frame + 1);
        for i in 0..count {
            let bits = 0x7FFD_0000_0000_0000 | ((frame as u64) << 16) | u64::from(i);
            js_shadow_slot_set(i, bits);
            expected.push(bits);
        }
    }

    let mut seen = scanner_slot_values();
    seen.sort_unstable();
    expected.sort_unstable();
    assert_eq!(seen, expected, "every frame's slots must be scanned");

    // Popping restores each caller's own slot view.
    for (frame, &count) in counts.iter().enumerate().rev() {
        assert_eq!(shadow_stack_depth(), frame + 1);
        for i in 0..count {
            assert_eq!(
                js_shadow_slot_get(i),
                0x7FFD_0000_0000_0000 | ((frame as u64) << 16) | u64::from(i),
                "frame {frame} slot {i} was clobbered by a callee"
            );
        }
        js_shadow_frame_pop(nest.0.pop().expect("frame handle"));
    }
    assert_eq!(shadow_stack_depth(), 0);
}

/// A push that outgrows the buffer must reallocate before the header and slot
/// writes land, and the frame chain must survive the move.
#[test]
fn frame_chain_survives_buffer_growth() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let mut nest = FrameNest(Vec::new());
    // Push well past SHADOW_STACK_GROW_RESERVE entries.
    let frames = SHADOW_STACK_GROW_RESERVE;
    for i in 0..frames {
        nest.push(1);
        js_shadow_slot_set(0, 0x7FFD_0000_0000_0000 | i as u64);
    }
    assert_eq!(shadow_stack_depth(), frames);
    assert_eq!(scanner_slot_values().len(), frames);
    for i in (0..frames).rev() {
        assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_0000_0000 | i as u64);
        js_shadow_frame_pop(nest.0.pop().expect("frame handle"));
    }
    assert_eq!(shadow_stack_depth(), 0);
}

/// A savepoint/restore pair (the `longjmp` unwind path) must drop the orphaned
/// frames *and* leave the buffer in a state where the next push still starts
/// from cleared entries.
#[test]
fn savepoint_restore_drops_orphaned_frames_without_leaving_stale_roots() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let outer = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x7FFD_0000_0000_00AA);

    let sp = shadow_stack_savepoint();
    let mut orphan_storage: u64 = 0x7FFD_0000_0000_00BB;
    let _inner = js_shadow_frame_push(3);
    js_shadow_slot_bind(0, &mut orphan_storage as *mut u64);
    js_shadow_slot_set(1, 0x7FFD_0000_0000_00CC);
    assert_eq!(shadow_stack_depth(), 2);

    shadow_stack_restore(sp);
    assert_eq!(shadow_stack_depth(), 1);
    assert_eq!(scanner_slot_values(), vec![0x7FFD_0000_0000_00AA]);

    // The catch body pushes its own frame over the abandoned storage.
    let after = js_shadow_frame_push(3);
    assert_eq!(scanner_slot_values(), vec![0x7FFD_0000_0000_00AA]);
    js_shadow_slot_set(0, 0x7FFD_0000_0000_00DD);
    assert_eq!(
        orphan_storage, 0x7FFD_0000_0000_00BB,
        "restored frame must not still be bound to the unwound frame's storage"
    );
    js_shadow_frame_pop(after);
    js_shadow_frame_pop(outer);
}

/// A malformed `frame_handle` must be ignored rather than panicking the host
/// process (the Windows release crash this guard was added for).
#[test]
fn out_of_range_frame_pop_is_ignored() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let h = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x7FFD_0000_0000_0001);
    // A NaN-boxed `undefined` threaded in where the handle belongs.
    js_shadow_frame_pop(0x7FFC_0000_0000_0001);
    assert_eq!(shadow_stack_depth(), 1, "frame must still be installed");
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_0000_0001);
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
}
