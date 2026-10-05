use super::*;

#[test]
fn heap_word_iterator_reads_refreshed_slots_after_stack_growth() {
    let scope = RuntimeHandleScope::new();
    let _unrelated = scope.root_nanbox_f64(19.0);
    let start = RuntimeHandleScope::active_len_for_tests();
    let input = [11.0f64.to_bits(), 12.0f64.to_bits()];
    let rooted = scope.root_heap_word_u64_slice_iter(&input);
    assert_eq!(rooted.len(), 2);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), start + 2);
    let capacity = RuntimeHandleScope::capacity_for_tests();
    {
        let nested = RuntimeHandleScope::new();
        for i in 0..=capacity {
            nested.root_nanbox_f64(i as f64);
        }
        assert!(RuntimeHandleScope::capacity_for_tests() > capacity);
        for (offset, value) in [21.0f64, 22.0].into_iter().enumerate() {
            RuntimeHandle {
                index: start + offset,
                stack: scope.stack,
                _scope: PhantomData,
            }
            .set_heap_word_u64(value.to_bits());
        }
    }
    assert_eq!(
        rooted.collect::<Vec<_>>(),
        vec![21.0f64.to_bits(), 22.0f64.to_bits()]
    );
    assert_eq!(scope.root_heap_word_u64_slice_iter(&[]).len(), 0);
}

#[test]
fn copy_visitor_can_read_update_and_grow_the_handle_stack() {
    let scope = RuntimeHandleScope::new();
    let first = scope.root_nanbox_f64(11.0);
    let second = scope.root_nanbox_f64(12.0);
    let count = runtime_handle_stack().capacity() + 1;
    let mut visits = 0;
    let mut callback = |value| {
        visits += 1;
        if value == 11.0 {
            first.set_nanbox_f64(21.0);
            let nested = RuntimeHandleScope::new();
            for i in 0..count {
                let _ = nested.root_nanbox_f64(i as f64);
            }
            assert_eq!(second.get_nanbox_f64(), 12.0);
        }
    };
    scan_runtime_handle_roots_mut(&mut RuntimeRootVisitor::for_copy(&mut callback));
    assert_eq!(visits, 2);
    assert_eq!(
        first.get_nanbox_f64(),
        21.0,
        "a non-rewriting visit must not undo callback updates"
    );
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), scope.base + 2);
}

#[test]
fn ffi_indices_and_kind_checks_survive_growth_and_restore() {
    let base = js_ffi_root_scope_enter();
    let nanbox = js_ffi_root_push_nanbox(13.0f64.to_bits());
    let heap_word = js_ffi_root_push_heap_addr(0);
    for i in 0..257 {
        js_ffi_root_push_nanbox((i as f64).to_bits());
    }
    assert_eq!(js_ffi_root_get_nanbox(nanbox), 13.0f64.to_bits());
    assert_eq!(js_ffi_root_get_heap_addr(heap_word), 0);
    assert_eq!(js_ffi_root_get_heap_addr(nanbox), 0);
    assert_eq!(js_ffi_root_get_nanbox(heap_word), 0);
    js_ffi_root_scope_exit(usize::MAX);
    assert_eq!(js_ffi_root_scope_enter(), base + 259);
    js_ffi_root_scope_exit(base);
    assert_eq!(js_ffi_root_scope_enter(), base);
    assert_eq!(js_ffi_root_get_nanbox(nanbox), 0);

    let scope = RuntimeHandleScope::new();
    let root = scope.root_nanbox_f64(31.0);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        root.get_heap_word_u64()
    }))
    .is_err());
    runtime_handle_stack_restore(base);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| { root.get_nanbox_f64() }))
            .is_err()
    );
}

#[test]
#[cfg(not(any(target_os = "android", target_env = "ohos")))]
fn handle_storage_teardown_clears_cache_before_late_scope_drop() {
    struct LateScope(RefCell<Option<RuntimeHandleScope>>);
    impl Drop for LateScope {
        fn drop(&mut self) {
            assert_eq!(runtime_handle_stack().len(), 0);
            assert_eq!(runtime_handle_stack().capacity(), 0);
            assert!(crate::tls_hot::hot().runtime_handle_stack.get().is_null());
            drop(self.0.get_mut().take());
            assert_eq!(runtime_handle_stack().len(), 0);
            assert!(
                std::panic::catch_unwind(|| {
                    let scope = RuntimeHandleScope::new();
                    let _ = scope.root_nanbox_f64(1.0);
                })
                .is_err(),
                "released TLS storage must not be resurrected"
            );
        }
    }
    thread_local! {
        static BEFORE_BUFFER: LateScope = const { LateScope(RefCell::new(None)) };
    }
    std::thread::spawn(|| {
        BEFORE_BUFFER.with(|_| {});
        let scope = RuntimeHandleScope::new();
        let root = scope.root_nanbox_f64(7.0);
        assert_eq!(root.get_nanbox_f64(), 7.0);
        assert!(!crate::tls_hot::hot().runtime_handle_stack.get().is_null());
        BEFORE_BUFFER.with(|late| *late.0.borrow_mut() = Some(scope));
    })
    .join()
    .expect("handle teardown thread");
}

#[cfg(any(target_os = "android", target_env = "ohos"))]
#[test]
fn late_scope_drop_resolves_os_tls_without_retaining_its_address() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DROPPED: AtomicBool = AtomicBool::new(false);
    struct LateScope(RefCell<Option<RuntimeHandleScope>>);
    impl Drop for LateScope {
        fn drop(&mut self) {
            drop(self.0.get_mut().take());
            DROPPED.store(true, Ordering::SeqCst);
        }
    }
    thread_local! {
        static BEFORE_BUFFER: LateScope = const { LateScope(RefCell::new(None)) };
    }
    std::thread::spawn(|| {
        BEFORE_BUFFER.with(|_| {});
        let scope = RuntimeHandleScope::new();
        let root = scope.root_nanbox_f64(7.0);
        assert_eq!(root.get_nanbox_f64(), 7.0);
        BEFORE_BUFFER.with(|late| *late.0.borrow_mut() = Some(scope));
    })
    .join()
    .expect("late OS-TLS scope thread");
    assert!(DROPPED.load(Ordering::SeqCst));
}

#[test]
fn native_argument_cells_are_marked_and_rewritten_in_place() {
    let nursery = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid = build_valid_pointer_set();
    let destination = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let cell = std::cell::UnsafeCell::new(f64::from_bits(POINTER_TAG | nursery as u64));
    let before = RuntimeHandleScope::active_len_for_tests();
    let scope = RuntimeHandleScope::new();
    let root = unsafe { scope.root_heap_word_cell(&cell) };
    scan_runtime_handle_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid));
    let header = unsafe { header_from_user_ptr(nursery) as *mut GcHeader };
    unsafe {
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
        set_forwarding_address(header, destination);
    }
    scan_runtime_handle_roots_mut(&mut RuntimeRootVisitor::for_rewrite(&valid));
    let expected = POINTER_TAG | destination as u64;
    assert_eq!(
        unsafe { (*cell.get()).to_bits() },
        expected,
        "the native constructor/replacer buffer must receive the collector rewrite"
    );
    assert_eq!(root.get_heap_word_u64(), expected);
    root.set_heap_word_u64(43.0f64.to_bits());
    assert_eq!(unsafe { *cell.get() }, 43.0);
    drop(scope);
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), before);
}
