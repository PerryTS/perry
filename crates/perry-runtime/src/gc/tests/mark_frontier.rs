use super::super::*;
use super::support::*;

fn assert_budgeted_frontier(root: usize, objects: &[usize], capacity_limit: usize) {
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    assert!(try_mark_value(POINTER_TAG | root as u64, &valid));
    clear_mark_seeds();
    let mut worklist = vec![unsafe { header_from_user_ptr(root as *const u8) }];
    let mut processed = 0;
    assert!(!drain_trace_worklist_step(
        &mut worklist,
        &mut processed,
        &valid,
        false,
        0
    ));
    assert_eq!(processed, 0);
    assert_eq!(worklist.len(), 1);
    loop {
        let before = processed;
        let done = drain_trace_worklist_step(&mut worklist, &mut processed, &valid, false, 7);
        assert!(
            processed - before <= 7,
            "each bounded step preserves its budget"
        );
        assert!(
            worklist.capacity() <= capacity_limit,
            "processed entries must not accumulate"
        );
        if done {
            assert!(worklist.is_empty());
            break;
        }
        assert!(processed > before);
    }
    assert!(
        processed <= objects.len(),
        "cycles and aliases must not queue objects twice"
    );
    for &address in objects {
        let header = unsafe { header_from_user_ptr(address as *const u8) };
        assert_ne!(
            unsafe { (*header).gc_flags } & GC_FLAG_MARKED,
            0,
            "reachable object was lost"
        );
    }
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn cyclic_chain_keeps_only_pending_headers_across_budgeted_steps() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let objects: Vec<_> = (0..4096)
        .map(|_| crate::array::js_array_alloc_with_length(1))
        .collect();
    for (index, &array) in objects.iter().enumerate() {
        let next = objects[(index + 1) % objects.len()];
        crate::array::js_array_set_f64(array, 0, f64::from_bits(ptr_bits(next as usize)));
    }
    let addresses: Vec<_> = objects.iter().map(|&p| p as usize).collect();
    assert_budgeted_frontier(addresses[0], &addresses, 8);
}

#[test]
fn shared_binary_tree_preserves_all_live_objects_with_a_small_frontier() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let objects: Vec<_> = (0..8191)
        .map(|_| crate::array::js_array_alloc_with_length(2))
        .collect();
    for (index, &array) in objects.iter().enumerate() {
        let left = 2 * index + 1;
        if left < objects.len() {
            crate::array::js_array_set_f64(
                array,
                0,
                f64::from_bits(ptr_bits(objects[left] as usize)),
            );
            crate::array::js_array_set_f64(
                array,
                1,
                f64::from_bits(ptr_bits(objects[left + 1] as usize)),
            );
        } else {
            // An alias to the root checks termination as well as reachability.
            crate::array::js_array_set_f64(array, 0, f64::from_bits(ptr_bits(objects[0] as usize)));
        }
    }
    let addresses: Vec<_> = objects.iter().map(|&p| p as usize).collect();
    assert_budgeted_frontier(addresses[0], &addresses, 32);
}
