use super::*;

pub fn reset_for_tests() {
    HOOKS.lock().unwrap().clear();
    RESOURCES.lock().unwrap().clear();
    GC_DESTROY_QUEUE.lock().unwrap().clear();
    CONTEXT_SNAPSHOTS.lock().unwrap().clear();
    ASYNC_WRAP_PROVIDERS.store(0, Ordering::Relaxed);
    TOP_LEVEL_RESOURCE.store(0, Ordering::Relaxed);
    HOOKS_ACTIVE.store(0, Ordering::Relaxed);
    PROMISE_HOOKS_ACTIVE.store(0, Ordering::Relaxed);
    NEXT_ASYNC_ID.store(2, Ordering::Relaxed);
    NEXT_CONTEXT_SNAPSHOT_ID.store(1, Ordering::Relaxed);
    CURRENT_EXECUTION_ID.with(|c| c.set(0));
    CURRENT_TRIGGER_ID.with(|c| c.set(0));
    EXECUTION_STACK.with(|s| s.borrow_mut().clear());
}

pub(crate) fn test_seed_async_hooks_scanner_roots(callback: *const ClosureHeader, resource: f64) {
    reset_for_tests();
    HOOKS.lock().unwrap().push(HookRecord {
        callbacks: HookCallbacks {
            init: callback,
            before: callback,
            after: callback,
            destroy: callback,
            promise_resolve: callback,
        },
        enabled: true,
        track_promises: true,
    });
    HOOKS_ACTIVE.store(1, Ordering::Relaxed);
    PROMISE_HOOKS_ACTIVE.store(1, Ordering::Relaxed);
    RESOURCES.lock().unwrap().insert(
        1,
        ResourceMeta {
            type_name: "test".to_string(),
            trigger_async_id: 0,
            resource,
            context: crate::async_context::AsyncContextSnapshot::default(),
            destroyed: false,
        },
    );
}

pub(crate) fn test_async_hooks_scanner_snapshot() -> (usize, u64) {
    let callback = HOOKS
        .lock()
        .unwrap()
        .first()
        .map(|hook| hook.callbacks.init as usize)
        .unwrap_or(0);
    let resource_bits = RESOURCES
        .lock()
        .unwrap()
        .get(&1)
        .map(|meta| meta.resource.to_bits())
        .unwrap_or(0);
    (callback, resource_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn throwing_lifecycle_hook(_closure: *const ClosureHeader, _async_id: f64) -> f64 {
        crate::exception::js_throw(73.0)
    }

    fn enable_throwing_lifecycle_hook(before_phase: bool) {
        let callback = js_closure_alloc(throwing_lifecycle_hook as *const u8, 0);
        let mut callbacks = HookCallbacks::empty();
        if before_phase {
            callbacks.before = callback;
        } else {
            callbacks.after = callback;
        }
        HOOKS.lock().unwrap().push(HookRecord {
            callbacks,
            enabled: true,
            track_promises: false,
        });
        HOOKS_ACTIVE.store(1, Ordering::Relaxed);
    }

    // #7680: no lock needed here anymore. The per-test globals isolate this
    // thread's reset and resource-id sequence from concurrent tests.
    #[test]
    fn resource_ids_are_monotonic_even_without_hooks() {
        reset_for_tests();
        let a = init_resource("A", TAG_UNDEFINED_F64, true);
        let b = init_resource("B", TAG_UNDEFINED_F64, true);
        assert!(a.async_id > 1);
        assert_eq!(b.async_id, a.async_id + 1);
    }

    #[test]
    fn before_after_restore_execution_ids() {
        reset_for_tests();
        let ids = init_resource("A", TAG_UNDEFINED_F64, true);
        before(ids.async_id, ids.trigger_async_id);
        assert_eq!(execution_async_id_u64(), ids.async_id);
        after(ids.async_id);
        assert_eq!(execution_async_id_u64(), 0);
    }

    #[test]
    fn resource_scope_restores_context_when_lifecycle_hooks_throw() {
        const STORE: i64 = -9_401;
        for before_phase in [true, false] {
            reset_for_tests();
            crate::async_context::clear_store(STORE);
            crate::async_context::enter_with(STORE, 11.0);
            let ids = init_resource("throwing-scope", TAG_UNDEFINED_F64, true);
            crate::async_context::enter_with(STORE, 22.0);
            enable_throwing_lifecycle_hook(before_phase);

            let mut completion_ran = false;
            let outcome = try_run_resource_scope(ids, || {
                completion_ran = true;
                TAG_UNDEFINED_F64
            });

            assert_eq!(outcome.unwrap_err().to_bits(), 73.0f64.to_bits());
            assert_eq!(completion_ran, !before_phase);
            assert_eq!(execution_async_id_u64(), 0);
            assert!(EXECUTION_STACK.with(|stack| stack.borrow().is_empty()));
            assert_eq!(crate::async_context::get_store(STORE), Some(22.0));
            crate::async_context::clear_store(STORE);
        }
        reset_for_tests();
    }

    #[test]
    fn track_promises_filters_hooks_and_activity() {
        reset_for_tests();
        let mut callbacks = HookCallbacks::empty();
        callbacks.init = std::ptr::NonNull::<ClosureHeader>::dangling().as_ptr();
        HOOKS.lock().unwrap().extend([
            HookRecord {
                callbacks,
                enabled: false,
                track_promises: false,
            },
            HookRecord {
                callbacks,
                enabled: false,
                track_promises: true,
            },
        ]);
        let suppressed = AsyncHookHandle { index: 0 };
        let tracked = AsyncHookHandle { index: 1 };
        js_async_hook_enable(&suppressed as *const AsyncHookHandle as i64);
        assert!(hooks_active());
        assert!(!promise_hooks_active());
        js_async_hook_enable(&tracked as *const AsyncHookHandle as i64);
        assert!(promise_hooks_active());
        assert_eq!(enabled_callbacks(false).len(), 2);
        assert_eq!(enabled_callbacks(true).len(), 1);
        js_async_hook_disable(&tracked as *const AsyncHookHandle as i64);
        assert!(hooks_active());
        assert!(!promise_hooks_active());
        js_async_hook_disable(&suppressed as *const AsyncHookHandle as i64);
        assert!(!hooks_active());
        reset_for_tests();
    }

    #[test]
    fn async_hooks_state_survives_a_foreign_reset_for_tests() {
        reset_for_tests();
        let ids = init_resource("survivor", TAG_UNDEFINED_F64, true);
        assert!(RESOURCES.lock().unwrap().contains_key(&ids.async_id));
        std::thread::spawn(reset_for_tests)
            .join()
            .expect("the clearing thread panicked");
        assert!(RESOURCES.lock().unwrap().contains_key(&ids.async_id));
        assert!(NEXT_ASYNC_ID.load(Ordering::Relaxed) > ids.async_id);
        reset_for_tests();
    }

    #[test]
    fn native_async_resource_accepts_string_and_symbol_expandos() {
        reset_for_tests();
        crate::symbol::test_clear_symbol_side_table_roots();
        let type_ptr = js_string_from_bytes(b"ExpandoResource".as_ptr(), 15);
        let type_value = crate::value::js_nanbox_string(type_ptr as i64);
        let handle = js_async_resource_new(type_value, TAG_UNDEFINED_F64);
        assert!(is_async_resource_handle(handle));
        let resource = crate::value::js_nanbox_pointer(handle);
        let symbol = unsafe { crate::symbol::js_symbol_new_empty() };
        unsafe {
            crate::symbol::js_object_set_symbol_property(resource, symbol, TAG_UNDEFINED_F64);
        }
        crate::value::js_dyn_index_set(resource, symbol, 41.0);
        assert_eq!(
            unsafe { crate::symbol::js_object_get_symbol_property(resource, symbol) }.to_bits(),
            41.0f64.to_bits()
        );
        let name_ptr = js_string_from_bytes(b"label".as_ptr(), 5);
        let name = crate::value::js_nanbox_string(name_ptr as i64);
        crate::proxy::js_put_value_set(resource, name, 42.0, resource, 1);
        assert_eq!(
            try_async_resource_property_dispatch(handle, "label").map(f64::to_bits),
            Some(42.0f64.to_bits())
        );
        reset_for_tests();
    }
}
