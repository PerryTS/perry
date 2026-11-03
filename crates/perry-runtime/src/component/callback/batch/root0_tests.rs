use super::*;
use crate::{async_context as als, value::js_nanbox_pointer};

#[test]
fn initialization_context_does_not_pop_its_callers_execution_scope_when_it_throws() {
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::gc::gc_init();
    crate::async_hooks::reset_for_tests();
    let scope = RuntimeHandleScope::new();
    let token = als::AsyncLocalStoragePayload::default();
    als::set_store(token.token(), 11.0);
    let initializing = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    let (record, _) = super::super::saved::capture(js_nanbox_pointer(
        initializing.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
    ));
    let record = scope.root_nanbox_f64(record);
    als::set_store(token.token(), 17.0);
    let parent = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    let parent_id = unsafe { (*parent.get_raw_mut_ptr::<crate::promise::Promise>()).async_id };
    assert_ne!(parent_id, 0);
    let batch = FrameBatch::new(&scope, undefined());
    let parent = batch.for_promise(
        1,
        js_nanbox_pointer(parent.get_raw_mut_ptr::<crate::promise::Promise>() as i64),
    );
    let initialization = batch.for_context(
        0,
        record.get_nanbox_f64(),
        AsyncResourceIds {
            async_id: 0,
            trigger_async_id: 0,
        },
    );
    assert_eq!(
        parent.run(|| {
            assert_eq!(crate::async_hooks::execution_async_id_u64(), parent_id);
            assert_eq!(
                initialization.run(|| {
                    assert_eq!(als::get_store(token.token()), Some(11.0));
                    assert_eq!(crate::async_hooks::execution_async_id_u64(), parent_id);
                    crate::exception::js_throw(71.0);
                }),
                Err(71.0)
            );
            assert_eq!(als::get_store(token.token()), Some(17.0));
            assert_eq!(
                crate::async_hooks::execution_async_id_u64(),
                parent_id,
                "origin-zero work must not consume the caller's execution stack entry"
            );
            31.0
        }),
        Ok(31.0)
    );
    assert_eq!(als::get_store(token.token()), Some(17.0));
    assert_eq!(crate::async_hooks::execution_async_id_u64(), 0);
    als::clear_store(token.token());
    crate::async_hooks::reset_for_tests();
}
