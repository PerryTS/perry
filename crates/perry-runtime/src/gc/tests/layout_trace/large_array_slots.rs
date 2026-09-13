//! Array tracing is bounded by allocated storage, including unused growth slack.
//! These checks inspect ranges and marking without sweeping or moving objects.

use super::*;

#[test]
fn large_array_live_prefix_is_enumerated_and_marked() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();

    // The capacity produced by geometric growth is larger than every tested
    // live prefix. Allocate it once; the unused slots remain initialized holes.
    let arr = crate::array::js_array_alloc(1 << 24);
    let header = unsafe { header_from_user_ptr(arr.cast()) };
    for length in [9_000_000, 10_000_000, 16_000_000] {
        unsafe {
            (*arr).length = length;
            let range = crate::array::gc_element_slot_range(arr)
                .expect("a valid allocation must expose its live prefix");
            assert_eq!(range.slot_count(), length as usize);
        }

        let text = b"large-array-live-child";
        let child = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        let child_header = unsafe { header_from_user_ptr(child.cast()) };
        let index = length - 1;
        crate::array::js_array_set_f64(arr, 0, 42.0);
        crate::array::js_array_set_f64(arr, index, crate::value::js_nanbox_string(child as i64));

        // Visit the collector's shared descriptors, not just the range helper.
        // Mark, copy, rewrite and dirty-slot scans all consume this slot set.
        let expected_slot = unsafe { crate::array::array_elements_ptr(arr).add(index as usize) };
        let mut found = false;
        unsafe {
            visit_gc_rewrite_slot_descriptors(header, |descriptor| {
                descriptor.visit_slots(&mut |slot| {
                    found |= slot.slot == expected_slot;
                });
            });
        }
        assert!(found, "the last live element must be enumerated");

        let valid_ptrs = build_valid_pointer_set();
        let mut worklist = Vec::new();
        unsafe {
            trace_array(arr.cast(), &valid_ptrs, &mut worklist);
            assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
        }
        clear_marks();
        clear_mark_seeds();
    }
}

#[test]
fn array_slot_range_preserves_allocation_and_sparse_bounds() {
    let _trigger = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = crate::array::js_array_alloc(16);
    unsafe {
        let capacity = (*arr).capacity;
        (*arr).length = 3;
        (*arr).capacity = capacity + 1;
        assert!(crate::array::gc_element_slot_range(arr).is_none());
        (*arr).capacity = capacity;

        // Sparse logical length does not authorize scanning outside storage.
        (*arr).length = u32::MAX;
        assert_eq!(
            crate::array::gc_element_slot_range(arr)
                .unwrap()
                .slot_count(),
            capacity as usize
        );
        // A consumed queue prefix reduces the remaining dense capacity.
        (*arr).capacity -= 4;
        assert_eq!(
            crate::array::gc_element_slot_range(arr)
                .unwrap()
                .slot_count(),
            (capacity - 4) as usize
        );
        (*arr).length = 0;
        (*arr).capacity = capacity;
    }
}
