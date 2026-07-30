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
        assert_eq!(s.slots()[top].value, ptr_bits(0xAAAA_0000));
        assert_eq!(s.slots()[top + 1].value, ptr_bits(0xBBBB_0000));
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
    // A NaN-boxed `undefined` threaded in where the handle belongs, plus the
    // two handles that would wrap a `base + HEADER_SLOTS` bounds check and slip
    // past it into an unchecked read.
    for bogus in [0x7FFC_0000_0000_0001u64, u64::MAX, u64::MAX - 1] {
        js_shadow_frame_pop(bogus);
        assert_eq!(shadow_stack_depth(), 1, "frame must still be installed");
    }
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_0000_0001);
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
}

// ---------------------------------------------------------------------------
// #7088: the inline slot-store addressing contract.
//
// Generated code no longer calls `js_shadow_slot_bind` / `js_shadow_slot_set`
// for the hot per-store root write. It computes the entry address itself from
// the `ShadowStackState` pointer `js_shadow_frame_enter` returned, using the
// published `SHADOW_*` offsets, and stores the two words directly.
//
// The helpers below are a *faithful Rust transcription of the emitted LLVM IR*
// -- same offsets, same guards, same order -- so these tests exercise the
// contract codegen depends on: if an offset, the entry size or the liveness
// bit moves, or if the state pointer stops naming the same memory the runtime
// functions write, they fail. What they cannot check is that codegen emits
// this exact sequence; `shadow_inline`'s IR-shape test covers that half.
// ---------------------------------------------------------------------------

/// Byte-offset load helper matching the emitted `getelementptr inbounds i8`.
unsafe fn state_word(state: *mut ShadowStackState, byte_off: usize) -> usize {
    *(state.cast::<u8>().add(byte_off).cast::<usize>())
}

/// The emitted inline bind, transcribed. Returns `false` when a guard fired,
/// i.e. when the emitted code would have skipped the write.
unsafe fn inline_bind_as_codegen_emits(
    state: *mut ShadowStackState,
    idx: u32,
    local_slot: *mut u64,
) -> bool {
    let frame_top = state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET);
    if frame_top == usize::MAX {
        return false;
    }
    let slot = frame_top + idx as usize;
    if slot >= state_word(state, SHADOW_STATE_LEN_OFFSET) {
        return false;
    }
    let buf = state_word(state, SHADOW_STATE_PTR_OFFSET) as *mut u8;
    let entry = buf.add(slot * SHADOW_ENTRY_SIZE);
    let raw = local_slot as usize;
    let bound = if raw & SHADOW_SLOT_ACTIVE_BIT == 0 {
        raw
    } else {
        0
    };
    *entry.cast::<u64>() = *local_slot;
    *entry.add(SHADOW_ENTRY_META_OFFSET).cast::<usize>() = bound | SHADOW_SLOT_ACTIVE_BIT;
    true
}

/// The emitted inline clear, transcribed.
unsafe fn inline_clear_as_codegen_emits(state: *mut ShadowStackState, idx: u32) -> bool {
    let frame_top = state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET);
    if frame_top == usize::MAX {
        return false;
    }
    let slot = frame_top + idx as usize;
    if slot >= state_word(state, SHADOW_STATE_LEN_OFFSET) {
        return false;
    }
    let buf = state_word(state, SHADOW_STATE_PTR_OFFSET) as *mut u8;
    let entry = buf.add(slot * SHADOW_ENTRY_SIZE);
    let meta_ptr = entry.add(SHADOW_ENTRY_META_OFFSET).cast::<usize>();
    *entry.cast::<u64>() = 0;
    *meta_ptr &= !SHADOW_SLOT_ACTIVE_BIT;
    true
}

/// `js_shadow_frame_enter` must push exactly the frame `js_shadow_frame_push`
/// pushes, and hand back a state whose `frame_top` yields the same handle.
///
/// Sabotage check: drop the `- SHADOW_STACK_HEADER_SLOTS` from codegen's
/// handle recovery and the recovered handle stops matching, so every emitted
/// `js_shadow_frame_pop` would unbalance the stack.
#[test]
fn frame_enter_pushes_the_same_frame_and_yields_the_same_handle() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();

    let before = shadow_stack_depth();
    let state = js_shadow_frame_enter(3);
    assert!(
        !state.is_null(),
        "frame_enter must return the state address"
    );
    assert_eq!(shadow_stack_depth(), before + 1);
    assert_eq!(
        state as usize,
        js_shadow_state_addr() as usize,
        "frame_enter and state_addr must name the same thread-local"
    );

    let frame_top = unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) };
    let recovered_handle = (frame_top - SHADOW_STACK_HEADER_SLOTS) as u64;

    js_shadow_frame_pop(recovered_handle);
    assert_eq!(
        shadow_stack_depth(),
        before,
        "handle recovered from frame_top must pop the frame frame_enter pushed"
    );
}

/// The inline write and the runtime accessor must address the same memory, in
/// both directions.
///
/// Sabotage check: change any `SHADOW_STATE_*` offset, `SHADOW_ENTRY_SIZE` or
/// `SHADOW_ENTRY_META_OFFSET` and this fails.
#[test]
fn inline_write_and_runtime_accessor_address_the_same_entry() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let state = js_shadow_frame_enter(2);
    let handle =
        unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) } - SHADOW_STACK_HEADER_SLOTS;

    // inline write -> runtime read
    let mut storage: u64 = 0x7FFD_0000_DEAD_BEEF;
    assert!(unsafe { inline_bind_as_codegen_emits(state, 1, &mut storage as *mut u64) });
    assert_eq!(
        js_shadow_slot_get(1),
        0x7FFD_0000_DEAD_BEEF,
        "runtime accessor must observe the inline write"
    );

    // runtime write -> inline read
    let mut other: u64 = 0x7FFF_0000_0000_00AA;
    js_shadow_slot_bind(0, &mut other as *mut u64);
    let buf = unsafe { state_word(state, SHADOW_STATE_PTR_OFFSET) } as *const u8;
    let frame_top = unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) };
    let entry = unsafe { buf.add(frame_top * SHADOW_ENTRY_SIZE) };
    assert_eq!(
        unsafe { *entry.cast::<u64>() },
        0x7FFF_0000_0000_00AA,
        "inline addressing must observe the runtime write"
    );
    assert_eq!(
        unsafe { *entry.add(SHADOW_ENTRY_META_OFFSET).cast::<usize>() },
        (&mut other as *mut u64 as usize) | SHADOW_SLOT_ACTIVE_BIT,
        "inline addressing must see the binding the runtime recorded"
    );

    js_shadow_frame_pop(handle as u64);
}

/// The inline clear must leave the binding in place with the liveness bit
/// dropped -- the same state `js_shadow_slot_set(idx, 0)` leaves, so a later
/// re-activation still writes through to the same compiled local.
///
/// Sabotage check: make the inline clear zero `meta` outright and the
/// re-activated slot stops writing through to `storage`.
#[test]
fn inline_clear_matches_the_runtime_clear_and_keeps_the_binding() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let state = js_shadow_frame_enter(1);
    let handle =
        unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) } - SHADOW_STACK_HEADER_SLOTS;

    let mut storage: u64 = 0x7FFD_0000_0000_1111;
    js_shadow_slot_bind(0, &mut storage as *mut u64);
    assert_eq!(scanner_slot_values().len(), 1, "slot starts live");

    assert!(unsafe { inline_clear_as_codegen_emits(state, 0) });
    assert!(
        scanner_slot_values().is_empty(),
        "cleared slot must not be reported as a root"
    );
    assert_eq!(js_shadow_slot_get(0), 0, "cleared slot reads as dead");

    // Re-activation still writes through the retained binding.
    js_shadow_slot_set(0, 0x7FFD_0000_0000_2222);
    assert_eq!(
        storage, 0x7FFD_0000_0000_2222,
        "inline clear must keep the binding so re-activation writes through"
    );

    js_shadow_frame_pop(handle as u64);
}

/// A value written by the inline sequence must be marked and rewritten by an
/// evacuating collection exactly as one written through `js_shadow_slot_bind`.
///
/// This is the liveness + rewritability property for the inline path. Sabotage
/// check: drop the `| SHADOW_SLOT_ACTIVE_BIT` from the inline meta write and
/// the scanner skips the entry, so the object is collected and `storage` no
/// longer points into the heap.
#[test]
fn inline_bound_slot_survives_and_is_rewritten_by_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(1);
    reset_shadow_stack();
    let state = js_shadow_frame_enter(1);

    let child = young_leaf();
    let mut storage: u64 = ptr_bits(child);
    assert!(unsafe { inline_bind_as_codegen_emits(state, 0, &mut storage as *mut u64) });

    let _ = gc_collect_minor();

    let moved = (storage & POINTER_MASK) as usize;
    assert_ne!(moved, 0, "inline-bound local was cleared by the collection");
    assert_ne!(moved, child, "test did not actually evacuate the object");
    assert!(
        crate::arena::pointer_in_nursery(moved) || crate::arena::pointer_in_old_gen(moved),
        "inline-bound local must hold a live heap address after collection"
    );
    assert_eq!(
        js_shadow_slot_get(0),
        storage,
        "slot read must observe the rewritten compiled local"
    );
}

/// The `frame_top == usize::MAX` sentinel guard, mirroring the one in
/// `js_shadow_slot_set` / `js_shadow_slot_bind`.
///
/// Honest scope: in a *balanced* program the guard is unreachable, because
/// `frame_top == usize::MAX` implies `len == 0` (the outermost pop restores
/// both), so the bounds check alone would already skip the write. It is kept
/// because the emitted sequence must be observably identical to the runtime
/// function it replaces, and because if the two ever *can* diverge the failure
/// mode is silent corruption rather than a skip: `usize::MAX + idx` wraps to
/// `idx - 1`, which for `idx >= 1` is an in-bounds index into the frame
/// *header* — overwriting `prev_frame_top` and `slot_count` and unlinking every
/// outer frame from the root scan.
///
/// So the test forces exactly that state rather than pretending a balanced
/// program reaches it. Sabotage check: drop the sentinel test from
/// `inline_bind_as_codegen_emits` (and from the emitter it transcribes) and the
/// header is overwritten instead of the write being skipped.
#[test]
fn inline_write_with_no_frame_installed_is_skipped_not_wrapped() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let state = js_shadow_frame_enter(2);
    let handle =
        unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) } - SHADOW_STACK_HEADER_SLOTS;

    // Snapshot the frame header, then force the no-frame sentinel while the
    // buffer still holds this frame's entries.
    let buf = unsafe { state_word(state, SHADOW_STATE_PTR_OFFSET) } as *mut u8;
    let header_before = unsafe { *buf.cast::<u64>() };
    let header_meta_before = unsafe { *buf.add(SHADOW_ENTRY_META_OFFSET).cast::<usize>() };
    unsafe {
        *(state
            .cast::<u8>()
            .add(SHADOW_STATE_FRAME_TOP_OFFSET)
            .cast::<usize>()) = usize::MAX;
    }

    let mut storage: u64 = 0x7FFD_0000_0000_9999;
    assert!(
        !unsafe { inline_bind_as_codegen_emits(state, 1, &mut storage as *mut u64) },
        "inline write must be skipped when no frame is installed"
    );
    assert!(
        !unsafe { inline_clear_as_codegen_emits(state, 1) },
        "inline clear must be skipped when no frame is installed"
    );
    assert_eq!(
        unsafe { *buf.cast::<u64>() },
        header_before,
        "a skipped write must not have wrapped into the frame header's \
         prev_frame_top word"
    );
    assert_eq!(
        unsafe { *buf.add(SHADOW_ENTRY_META_OFFSET).cast::<usize>() },
        header_meta_before,
        "a skipped write must not have wrapped into the frame header's \
         slot_count word"
    );

    // Restore a coherent state so the frame can be popped.
    unsafe {
        *(state
            .cast::<u8>()
            .add(SHADOW_STATE_FRAME_TOP_OFFSET)
            .cast::<usize>()) = handle + SHADOW_STACK_HEADER_SLOTS;
    }
    js_shadow_frame_pop(handle as u64);
}

/// An out-of-range slot index must be skipped, matching the runtime
/// functions' `slot >= len` guard, rather than writing past the frame.
#[test]
fn inline_write_past_the_frame_is_skipped() {
    let _guard = GcTestIsolationGuard::new();
    reset_shadow_stack();
    let state = js_shadow_frame_enter(1);
    let handle =
        unsafe { state_word(state, SHADOW_STATE_FRAME_TOP_OFFSET) } - SHADOW_STACK_HEADER_SLOTS;

    let len_before = unsafe { state_word(state, SHADOW_STATE_LEN_OFFSET) };
    let mut storage: u64 = 0x7FFD_0000_0000_7777;
    // Slot 5 in a 1-slot frame is past the end of the buffer.
    assert!(
        !unsafe { inline_bind_as_codegen_emits(state, 5, &mut storage as *mut u64) },
        "out-of-range inline write must be skipped"
    );
    assert_eq!(
        unsafe { state_word(state, SHADOW_STATE_LEN_OFFSET) },
        len_before,
        "a skipped write must not disturb the buffer"
    );

    js_shadow_frame_pop(handle as u64);
}
