//! #7803: an object promoted to Old DURING THE DRAIN must still get its
//! old->young edges into the remembered set.
//!
//! `rebuild_evacuated_old_to_young_remembered_set` used to run above
//! `collector.drain()`, so it only covered objects the ROOT walks moved.
//! Everything the drain promoted — every transitively-reachable object —
//! was appended to `moved_headers` after the rebuild had already run. A
//! parent promoted mid-drain whose child stayed young then had no
//! remembered-set entry (the collector's own rewrite fires no mutator
//! barrier, so its page was never dirty), and the NEXT minor moved the
//! child without rewriting the parent's slot.
//!
//! The shape below is zod's: schema metadata built at module init, promoted
//! after aging out, never written again, read on every parse. The victim
//! surfaces arbitrarily later as a TypeError or an incoherent-header
//! pin-latch abort, which is why #7803 burned five refuted hypotheses
//! before the whole-heap from-space scan named this slot.
//!
//! # Why this cannot pass vacuously
//!
//! The test asserts its subject ran at every step: the parent must actually
//! be IN old-gen after the promoting minor (a tenuring-threshold change
//! fails loudly here instead of silently degrading the test), the child
//! must still be young at that point (otherwise there is no old->young edge
//! and nothing is being tested), and the final read asserts the slot points
//! at the child's LIVE bytes, not merely at a different address.

use super::*;

/// Read closure `user`'s single capture slot.
unsafe fn capture_bits_of(user: usize) -> u64 {
    *((user + std::mem::size_of::<crate::closure::ClosureHeader>()) as *const u64)
}

unsafe fn capture_slot_of(user: usize) -> *mut u64 {
    (user + std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
}

fn young_closure_capturing(bits: u64) -> usize {
    let size = std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>();
    let user = crate::arena::arena_alloc_gc(size, 8, GC_TYPE_CLOSURE);
    unsafe {
        init_test_closure_with_one_capture(user, bits);
    }
    user as usize
}

/// A one-capture closure padded past several generation pages, so whatever
/// the drain copies to old-gen AFTER it lands on a different page than
/// whatever was copied BEFORE it.
fn young_padded_closure_capturing(bits: u64) -> usize {
    let size = std::mem::size_of::<crate::closure::ClosureHeader>()
        + std::mem::size_of::<u64>()
        + 3 * crate::arena::GENERATION_PAGE_SIZE;
    let user = crate::arena::arena_alloc_gc(size, 8, GC_TYPE_CLOSURE);
    unsafe {
        init_test_closure_with_one_capture(user, bits);
    }
    user as usize
}

#[test]
fn drain_promoted_parent_keeps_its_young_child_edge_remembered() {
    let _guard = CopyingNurseryTestGuard::new(1);

    // parent captures a young leaf; intermediate captures parent. Only the
    // INTERMEDIATE is rooted, so the parent is reached — and, on the
    // promoting cycle, moved to Old — by the worklist DRAIN, never by a
    // root walk. That drain-phase promotion is the population the pre-fix
    // rebuild missed.
    // The spacer between intermediate and parent is load-bearing: the
    // intermediate is moved by the ROOT walk, so even the pre-fix
    // (pre-drain) rebuild covers IT, and its from-space-looking slot value
    // keeps its old PAGE in the sticky dirty set. The dirty-page scan is
    // PAGE-granular — it walks every object on a dirty page — so a parent
    // promoted onto the SAME fresh old page as the intermediate is repaired
    // by its neighbor's entry and the pre-fix ordering passes this test by
    // accident (measured twice while writing it: without the spacer the
    // parent lands 32 bytes after the intermediate). The drain copies the
    // spacer's 3 pages of padding between them, so the parent's page has no
    // remembered neighbor — zod's shape, where the victim array's page had
    // no such benefactor.
    let first_child = young_leaf();
    let parent = young_closure_capturing(ptr_bits(first_child));
    let spacer = young_padded_closure_capturing(ptr_bits(parent));
    let intermediate = young_closure_capturing(ptr_bits(spacer));
    js_shadow_slot_set(0, ptr_bits(intermediate));

    let deref = |slot: u64| (slot & POINTER_MASK) as usize;
    // slot0 -> intermediate -> spacer -> parent, re-derived through the
    // rooted chain after every collection.
    let parent_now = || unsafe {
        let intermediate = deref(js_shadow_slot_get(0));
        let spacer = deref(capture_bits_of(intermediate));
        deref(capture_bits_of(spacer))
    };

    // Age everyone to the brink of promotion (power-on threshold: promote on
    // the fourth survival — pinned by
    // `test_copying_minor_promotes_survivor_on_fourth_survival`).
    for _ in 0..3 {
        let _ = gc_collect_minor();
    }
    assert!(
        crate::arena::pointer_in_nursery(parent_now()),
        "parent must still be young after three survivals; the tenuring \
         threshold moved and this test no longer stages a drain promotion"
    );

    // Give the parent a FRESH young child while the parent itself is still
    // young: a store to a young parent creates no remembered-set entry, so
    // the only thing that can carry this edge across the parent's promotion
    // is the promoted-object rebuild under test.
    let second_child = young_leaf();
    let expected_bytes = unsafe {
        let s = second_child as *const crate::StringHeader;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::slice::from_raw_parts(data, (*s).byte_len as usize).to_vec()
    };
    // A RAW slot write, no barrier — mirroring the real-world shape: the
    // zod slot was filled at allocation time while its owner was young
    // (where no barrier is required), and never stored to again. Calling
    // the barrier here would hand the edge to the dirty-page machinery and
    // let the pre-fix ordering pass this test by accident.
    unsafe {
        let p = parent_now();
        *capture_slot_of(p) = ptr_bits(second_child);
    }

    // The promoting minor: intermediate (rooted) moves in the root walk;
    // parent moves — to Old — in the drain; the child (age 1) is copied to
    // survivor space. Subject-liveness asserts, not assumptions:
    let _ = gc_collect_minor();
    let parent_old = parent_now();
    assert!(
        crate::arena::pointer_in_old_gen(parent_old),
        "the fourth survival must promote the parent to old-gen; without \
         that there is no drain-promoted old parent and this test covers \
         nothing"
    );
    let child_after_promotion = unsafe { deref(capture_bits_of(parent_old)) };
    assert!(
        crate::arena::pointer_in_nursery(child_after_promotion),
        "the freshly-stored child must still be young after the parent's \
         promotion; an old->old edge tests nothing"
    );

    // The exposing minor: the child moves again (survivor -> survivor).
    // Only a remembered-set entry for the drain-promoted parent lets this
    // cycle rewrite the parent's capture slot. Pre-fix, the slot keeps
    // `child_after_promotion` — from-space about to be recycled.
    let _ = gc_collect_minor();
    let parent_old = parent_now();
    let child_final = unsafe { deref(capture_bits_of(parent_old)) };
    assert_ne!(
        child_final, child_after_promotion,
        "the parent's capture slot was not rewritten when its young child \
         moved: the drain-promoted parent never made it into the remembered \
         set (#7803's root cause — the rebuild ran before the drain)"
    );
    unsafe {
        assert_string_bytes(child_final as *const crate::StringHeader, &expected_bytes);
    }
}
