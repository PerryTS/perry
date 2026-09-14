use super::*;

#[test]
fn handle_stack_growth_preserves_every_moving_root_including_last() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let capacity_before = RuntimeHandleScope::capacity_for_tests();
    let count = capacity_before + 257;
    let mut roots = Vec::new();
    for i in 0..count {
        let text = format!("moving handle payload number {i}");
        let ptr = crate::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        roots.push(match i % 3 {
            0 => scope.root_string_ptr(ptr),
            1 => scope.root_nanbox_u64(string_bits(ptr as usize)),
            _ => scope.root_heap_word_u64(ptr as u64),
        });
    }
    assert!(RuntimeHandleScope::capacity_for_tests() > capacity_before);
    let address = |i: usize, root: &RuntimeHandle<'_>| match i % 3 {
        0 => root.with_const_ptr(|ptr: *const crate::StringHeader| ptr as usize),
        1 => (root.get_nanbox_u64() & POINTER_MASK) as usize,
        _ => root.get_heap_word_u64() as usize,
    };
    let before: Vec<_> = roots
        .iter()
        .enumerate()
        .map(|(i, h)| address(i, h))
        .collect();
    gc_collect_minor();
    for (i, root) in roots.iter().enumerate() {
        let after = address(i, root);
        assert_ne!(before[i], after, "root {i} was not relocated");
        unsafe {
            assert_string_bytes(
                after as *const crate::StringHeader,
                format!("moving handle payload number {i}").as_bytes(),
            );
        }
    }
    // Grow again while these relocated handles remain live, then collect again.
    let capacity = RuntimeHandleScope::capacity_for_tests();
    for i in 0..=capacity {
        let _ = scope.root_nanbox_f64(i as f64);
    }
    assert!(RuntimeHandleScope::capacity_for_tests() > capacity);
    gc_collect_minor();
    for (i, root) in roots.iter().enumerate() {
        unsafe {
            assert_string_bytes(
                address(i, root) as *const crate::StringHeader,
                format!("moving handle payload number {i}").as_bytes(),
            );
        }
    }
}

#[test]
fn handle_stack_real_throw_restores_live_outer_scope_and_ffi_roots() {
    unsafe extern "C" {
        fn js_ffi_root_scope_enter() -> usize;
        fn js_ffi_root_push_nanbox(bits: u64) -> usize;
    }
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _evacuation = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let ptr = crate::js_string_from_bytes(b"outer root across caught throw".as_ptr(), 30);
    let root = scope.root_string_ptr(ptr);
    let base = RuntimeHandleScope::active_len_for_tests();
    let result: Result<(), f64> = crate::exception::catch_js_throw(|| {
        let inner = RuntimeHandleScope::new();
        for i in 0..257 {
            let _ = inner.root_nanbox_f64(i as f64);
        }
        unsafe {
            let _ffi_base = js_ffi_root_scope_enter();
            js_ffi_root_push_nanbox(42.0f64.to_bits());
        }
        assert!(RuntimeHandleScope::active_len_for_tests() > base);
        crate::exception::js_throw(17.0);
    });
    assert_eq!(result, Err(17.0));
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), base);
    gc_collect_minor();
    root.with_const_ptr(|after: *const crate::StringHeader| {
        assert_ne!(ptr, after.cast_mut(), "the surviving outer root must move");
        unsafe { assert_string_bytes(after, b"outer root across caught throw") };
    });
    let tail = scope.root_nanbox_f64(23.0);
    assert_eq!(tail.get_nanbox_f64(), 23.0);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), base + 1);
}

#[test]
fn handle_stack_pushes_and_updates_shade_all_kinds_during_marking() {
    let _guard = GcTestIsolationGuard::new();
    clear_marks();
    clear_mark_seeds();
    let pointers: Vec<_> = (0..10)
        .map(|_| crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT))
        .collect();
    let valid_ptrs = build_valid_pointer_set();
    let scope = RuntimeHandleScope::new();
    let mut state = new_runtime_handle_root_scan_state();
    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    let mut budget = 1;
    assert!(scan_runtime_handle_roots_mut_step(
        &mut marker,
        state.as_mut(),
        &mut budget
    ));
    // The scanner has already passed the current prefix. These pre-existing
    // white objects must be shaded by publishing roots, not allocate-black.
    let active = IncrementalMarkBarrierTestGuard::new(&valid_ptrs);
    let roots = [
        scope.root_nanbox_u64(ptr_bits(pointers[0] as usize)),
        scope.root_heap_word_u64(pointers[1] as u64),
        scope.root_raw_mut_ptr(pointers[2]),
        scope.root_string_ptr(pointers[3].cast()),
        scope.root_bigint_ptr(pointers[4]),
    ];
    for ptr in &pointers[..5] {
        assert_marked_user_ptr(*ptr as usize, "new handle during marking");
    }
    roots[0].set_nanbox_u64(ptr_bits(pointers[5] as usize));
    roots[1].set_heap_word_u64(pointers[6] as u64);
    roots[2].set_raw_mut_ptr(pointers[7]);
    roots[3].set_raw_const_ptr(pointers[8]);
    roots[4].set_raw_const_ptr(pointers[9]);
    for ptr in &pointers[5..] {
        assert_marked_user_ptr(*ptr as usize, "updated handle during marking");
    }
    drop(active);
    clear_marks();
    let _idle = scope.root_nanbox_u64(ptr_bits(pointers[0] as usize));
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(pointers[0])).gc_flags & GC_FLAG_MARKED,
            0
        );
    }
}

#[test]
fn handle_stack_budgeted_scan_resumes_after_growth_and_truncation() {
    let _guard = GcTestIsolationGuard::new();
    let scope = RuntimeHandleScope::new();
    let _first = scope.root_nanbox_f64(1.0);
    let inner = RuntimeHandleScope::new();
    let _second = inner.root_nanbox_f64(2.0);
    let mut seen = Vec::new();
    let mut record = |value| seen.push(value);
    let mut visitor = RuntimeRootVisitor::for_copy(&mut record);
    let mut state = new_runtime_handle_root_scan_state();
    let mut budget = 1;
    assert!(!scan_runtime_handle_roots_mut_step(
        &mut visitor,
        state.as_mut(),
        &mut budget
    ));
    assert_eq!(budget, 0);
    drop(inner);
    let count = RuntimeHandleScope::capacity_for_tests() + 1;
    for i in 0..count {
        let _ = scope.root_nanbox_f64((i + 3) as f64);
    }
    let mut budget = count + 1;
    assert!(scan_runtime_handle_roots_mut_step(
        &mut visitor,
        state.as_mut(),
        &mut budget
    ));
    assert_eq!(budget, 1);
    assert_eq!(seen.len(), count + 1);
    assert_eq!(seen[0], 1.0);
    assert_eq!(*seen.last().unwrap(), (count + 2) as f64);
    assert!(!seen.contains(&2.0));
}
