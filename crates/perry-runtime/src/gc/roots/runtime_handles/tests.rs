use super::*;

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
