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
