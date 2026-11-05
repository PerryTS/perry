//! #6179: the budgeted-cycle membership classifier is the UNION of an arena
//! term (a plausible arena header on a known page) and the exact malloc
//! registry. It evaluates the arena term first, because nearly every traced
//! edge is an arena object and the registry term is a hash probe. These tests
//! pin that the order changes the cost and not the answer: each term still
//! admits its own objects.

use super::super::*;
use super::support::*;

fn isolated(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        f();
    })
    .join()
    .expect("classifier membership fixture must not panic");
}

#[test]
fn the_classifier_admits_a_malloc_object_through_the_registry_term() {
    isolated(|| unsafe {
        let user = gc_malloc(64, GC_TYPE_STRING);
        let header = header_from_user_ptr(user) as *mut GcHeader;
        // PREMISE: a malloc allocation lies outside every arena range, and
        // the arena term rejects its header, so only the registry term can
        // admit it. The classifier must still ask the registry when the page
        // is `Unknown`; returning early there would fail the last assertion.
        assert!(matches!(
            crate::arena::classify_heap_generation(user as usize),
            crate::arena::HeapGeneration::Unknown
        ));
        assert!(super::super::barrier::plausible_arena_user_ptr_header(header).is_none());
        assert!(gc_malloc_header_is_tracked(header));
        assert!(mark::classifier_valid_object_start(user as usize));
    });
}

#[test]
fn the_classifier_admits_an_arena_object_without_the_registry() {
    isolated(|| unsafe {
        let leaf = young_leaf();
        let header = header_from_user_ptr(leaf as *const u8) as *mut GcHeader;
        // PREMISE: the registry does not know this object, so the arena term
        // alone must admit it.
        assert!(!gc_malloc_header_is_tracked(header));
        assert!(super::super::barrier::plausible_arena_user_ptr_header(header).is_some());
        assert!(mark::classifier_valid_object_start(leaf));
    });
}
