//! Collector-level coverage for the adaptive tenuring threshold
//! (gc/tenuring.rs): the copying minor must stop re-copying a saturated
//! survivor cohort by lowering the promotion age when Eden survivor influx
//! is heavy, and must restore the power-on age when the influx subsides.
//! Pure-formula coverage lives in `gc::tenuring::tests`; this file drives
//! real copying minors end to end.

use super::super::super::*;
use super::super::support::*;

const SLOTS: u32 = 340;

/// One rooted ~4 KB heap string per slot: comfortably under the 16 KB
/// large-object threshold (so it is nursery-allocated and copyable), while
/// 340 of them (~1.4 MB) exceed the 1 MB desired survivor size, which is
/// what makes the influx "heavy".
fn fill_slots_with_heavy_influx() {
    for slot in 0..SLOTS {
        let payload = vec![b'a' + (slot % 26) as u8; 4096];
        let s = crate::string::js_string_from_bytes(payload.as_ptr(), payload.len() as u32);
        js_shadow_slot_set(slot, string_bits(s as usize));
    }
}

#[test]
fn heavy_influx_lowers_threshold_and_promotes_next_cycle() {
    let _guard = CopyingNurseryTestGuard::new(SLOTS);
    assert_eq!(
        crate::gc::tenuring::tenuring_survivals(),
        GC_COPY_PROMOTION_SURVIVALS,
        "guard must start every test at the power-on threshold"
    );

    fill_slots_with_heavy_influx();
    let before = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(before));

    // Cycle 1 runs at the power-on threshold: the cohort is copied into a
    // survivor space (ages to 1), and its influx re-tunes the threshold down
    // to promote-on-first-copy.
    let _ = gc_collect_minor();
    assert_eq!(
        crate::gc::tenuring::tenuring_survivals(),
        1,
        "a >desired Eden survivor influx must drop the threshold to 1"
    );
    let after_first = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(
        crate::arena::pointer_in_nursery(after_first),
        "cycle 1 still ran at the power-on threshold, so the cohort ages in a survivor space"
    );

    // Cycle 2 promotes the whole cohort instead of re-copying it: this is
    // the ping-pong the adaptive threshold exists to break.
    let _ = gc_collect_minor();
    for slot in 0..SLOTS {
        let addr = (js_shadow_slot_get(slot) & POINTER_MASK) as usize;
        assert!(
            !crate::arena::pointer_in_nursery(addr),
            "slot {slot}: survivor must be tenured on the cycle after the threshold drop"
        );
    }
}

/// #7929: a real copying minor must feed its move census into the nursery
/// band's object denomination.
///
/// The pure-function coverage lives in `gc::tenuring::tests`; that coverage
/// passes with the `copying.rs` call site deleted, which is exactly the "the
/// gate runs but its subject never did" shape. This test drives a real cycle.
///
/// ★ The discriminating quantity is **the recorded mean equals THIS cycle's
/// measured mean, and differs from the seed**. Asserting only that the mean is
/// nonzero would be satisfied by the seed itself, so an unwired build would
/// pass it — a presence check, not a proof.
#[test]
fn copying_minor_feeds_the_object_denomination_census() {
    let _guard = CopyingNurseryTestGuard::new(SLOTS);
    let base = crate::gc::tenuring::influx_driven_nursery_cap_bytes();
    let seed = crate::gc::tenuring::mean_surviving_object_bytes();
    assert_eq!(
        seed,
        crate::gc::tenuring::NURSERY_CAP_REFERENCE_OBJECT_BYTES,
        "an unmeasured process must pace as the pre-#7929 collector did"
    );

    // Two-field object literals: the representation #7928 took 72 B -> 56 B,
    // i.e. the one whose object budget this term exists to hold constant.
    for slot in 0..SLOTS {
        let obj = crate::object::js_object_alloc(0, 2);
        crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(slot as f64));
        crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(-1.0));
        js_shadow_slot_set(slot, ptr_bits(obj as usize));
    }

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let moved_objects =
        trace.copying_nursery.copied_objects + trace.copying_nursery.promoted_objects;
    let moved_bytes = trace.copying_nursery.copied_bytes + trace.copying_nursery.promoted_bytes;
    assert!(
        moved_objects >= SLOTS as usize,
        "the cycle must actually have moved the cohort: {moved_objects} objects"
    );

    let recorded = crate::gc::tenuring::mean_surviving_object_bytes();
    assert_eq!(
        recorded,
        moved_bytes / moved_objects,
        "the census must carry THIS cycle's measured mean ({moved_bytes} B over \
         {moved_objects} objects)"
    );
    assert_ne!(
        recorded, seed,
        "fixture is vacuous: a measured mean equal to the seed cannot distinguish a wired \
         build from an unwired one"
    );
    assert!(
        recorded < crate::gc::tenuring::NURSERY_CAP_REFERENCE_OBJECT_BYTES,
        "fixture must exercise the SCALING arm, not the one-sided clamp (mean {recorded} B)"
    );

    // And the band moved with it, proportionally.
    assert_eq!(
        crate::gc::tenuring::influx_driven_nursery_cap_bytes(),
        base * crate::gc::tenuring::nursery_cap_object_scale_permille(recorded) / 1000
    );
}

#[test]
fn quiet_cycles_restore_power_on_threshold_debounced() {
    let _guard = CopyingNurseryTestGuard::new(SLOTS);

    fill_slots_with_heavy_influx();
    let _ = gc_collect_minor();
    assert_eq!(crate::gc::tenuring::tenuring_survivals(), 1);
    // Promote the cohort out of the nursery so later cycles are quiet.
    let _ = gc_collect_minor();

    // Quiet cycles (near-zero influx: the rooted cohort is old-gen now) must
    // raise the threshold one debounced step at a time, not snap back — a
    // single quiet cycle (e.g. a malloc-count trigger before Eden fills)
    // must not flush the aging pipeline into a copy burst.
    let mut seen = vec![crate::gc::tenuring::tenuring_survivals()];
    for _ in 0..8 {
        let _ = gc_collect_minor();
        seen.push(crate::gc::tenuring::tenuring_survivals());
    }
    assert_eq!(
        *seen.last().unwrap(),
        GC_COPY_PROMOTION_SURVIVALS,
        "sustained quiet influx must restore the power-on threshold, saw {seen:?}"
    );
    for pair in seen.windows(2) {
        assert!(
            pair[1] == pair[0] || pair[1] == pair[0] + 1,
            "threshold must rise at most one step per cycle, saw {seen:?}"
        );
    }
}
