//! Full sweep reuses the death fan-out while preserving representation death.
use super::*;

fn isolated(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        let _guard = CopyingNurseryTestGuard::new(2);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let _lock = crate::array::test_serialize();
        f();
    })
    .join()
    .unwrap();
}

#[test]
fn full_pruned_cleanup_preserves_header_and_epoch_invalidation() {
    isolated(|| unsafe {
        let array = crate::array::js_array_alloc(1);
        let owner = array as usize;
        let header = header_from_user_ptr(array.cast());
        crate::array::test_seed_element_shape_record(owner);
        (*header)._reserved |= GC_LAYOUT_ALL_POINTERS | GC_ARRAY_RAW_F64_HOLES;
        let epoch = crate::array::js_array_element_shape_epoch();
        crate::array::prune_dead_element_shape_owners(&|addr| addr == owner);
        assert!(!crate::array::test_element_shape_record_exists(owner));
        crate::gc::layout::layout_clear_after_full_prune(owner);
        assert_eq!(
            (*header)._reserved
                & (GC_ARRAY_RAW_F64_LAYOUT
                    | GC_ARRAY_RAW_F64_HOLES
                    | GC_ARRAY_ELEMENT_SHAPE
                    | GC_LAYOUT_ALL_POINTERS),
            0
        );
        assert_eq!(crate::array::js_array_element_shape_epoch(), epoch + 1);
        assert_eq!((*header).obj_type, GC_TYPE_ARRAY);
    });
}

#[test]
fn missing_full_death_prune_is_detected_and_regular_cleanup_still_removes_it() {
    isolated(|| unsafe {
        let array = crate::array::js_array_alloc(1);
        let owner = array as usize;
        crate::array::test_seed_element_shape_record(owner);
        let red = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::gc::layout::layout_clear_after_full_prune(owner);
        }));
        assert!(
            red.is_err(),
            "a skipped death fan-out must invalidate the shortcut"
        );
        assert!(crate::array::test_element_shape_record_exists(owner));
        layout_clear_for_ptr(owner);
        assert!(!crate::array::test_element_shape_record_exists(owner));
    });
}
