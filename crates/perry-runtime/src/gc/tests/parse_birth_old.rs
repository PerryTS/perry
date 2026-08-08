//! #7598: the parse-cohort birth window. Allocations inside a
//! [`ParseBirthOldScope`] are born tenured in old-gen; outside, the young
//! path is untouched. Both polarities asserted, plus nesting — the window
//! must survive an inner scope's exit (stringify force-materialize nests).

use super::super::*;
use super::support::*;

fn alloc_probe_object() -> usize {
    let packed_keys = b"a\0";
    crate::object::js_object_alloc_with_shape(0x7598, 1, packed_keys.as_ptr(), 2) as usize
}

#[test]
fn a_parse_birth_window_births_old_and_tenured_and_nests() {
    let _guard = GcTestIsolationGuard::new();

    // Outside any window: young.
    let young = alloc_probe_object();
    assert!(
        matches!(
            crate::arena::classify_heap_generation(young),
            crate::arena::HeapGeneration::Nursery
        ),
        "outside a window the young path must be untouched"
    );

    let outer = ParseBirthOldScope::new();
    let old_born = alloc_probe_object();
    assert!(
        matches!(
            crate::arena::classify_heap_generation(old_born),
            crate::arena::HeapGeneration::Old
        ),
        "inside the window births go to old-gen"
    );
    unsafe {
        let header = (old_born as *mut u8).sub(GC_HEADER_SIZE) as *mut GcHeader;
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "Old => TENURED must hold at birth (#7602 contract)"
        );
    }

    // Nesting: an inner scope's exit must not close the outer window.
    {
        let _inner = ParseBirthOldScope::new();
    }
    let still_old = alloc_probe_object();
    assert!(
        matches!(
            crate::arena::classify_heap_generation(still_old),
            crate::arena::HeapGeneration::Old
        ),
        "the outer window must survive an inner scope's drop"
    );

    drop(outer);
    let young_again = alloc_probe_object();
    assert!(
        matches!(
            crate::arena::classify_heap_generation(young_again),
            crate::arena::HeapGeneration::Nursery
        ),
        "closing the last window must restore young births"
    );
}
