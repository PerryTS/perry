//! #11471 thread-exit regression tests (listeners group).
//!
//! Each test populates a process-global perry-runtime table from a thread
//! through the real entry point, proves the entry is live while the thread
//! runs, and asserts the thread's exit released it.

use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::{ClosureHeader, JSValue, ObjectHeader};

extern "C" fn thunk1(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _a: f64,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

extern "C" fn thunk2(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _a: f64,
    _b: f64,
) -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn pointer_value(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let key = perry_runtime::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    perry_runtime::js_object_set_field_by_name(obj, key, value);
}

#[test]
fn thread_exit_disables_the_threads_v8_promise_hooks() {
    let (callback, live) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(thunk1, 1),
            0,
        ));
        let addr = || closure.get_raw_mut_ptr::<ClosureHeader>() as usize;
        let _stop =
            perry_runtime::v8::js_v8_promise_hooks_on_settled(pointer_value(addr() as *const u8));
        let live = perry_runtime::v8::promise_hook_callback_registered_for_test(addr());
        (addr(), live)
    })
    .join()
    .unwrap();
    assert!(live, "the hook must be registered while its thread lives");
    assert!(
        !perry_runtime::v8::promise_hook_callback_registered_for_test(callback),
        "a dead thread's v8.promiseHooks callback outlived its heap"
    );
}

#[test]
fn thread_exit_disables_the_threads_async_hooks_create_hook_record() {
    let (callback, live) = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(thunk1, 1),
            0,
        ));
        let options = scope.root_raw_mut_ptr(perry_runtime::js_object_alloc(0, 1));
        let addr = || closure.get_raw_mut_ptr::<ClosureHeader>() as usize;
        set_field(
            options.get_raw_mut_ptr::<ObjectHeader>(),
            "init",
            pointer_value(addr() as *const u8),
        );
        let _hook = perry_runtime::async_hooks::js_async_hooks_create_hook(pointer_value(
            options.get_raw_mut_ptr::<ObjectHeader>() as *const u8,
        ));
        perry_runtime::async_hooks::js_async_hook_enable(
            perry_runtime::value::js_nanbox_get_pointer(_hook),
        );
        let live = perry_runtime::async_hooks::hook_callback_registered_for_test(addr());
        (addr(), live)
    })
    .join()
    .unwrap();
    assert!(
        live,
        "the createHook record must exist while its thread lives"
    );
    assert!(
        !perry_runtime::async_hooks::hook_callback_registered_for_test(callback),
        "a dead thread's createHook callback outlived its heap"
    );
}

#[test]
fn thread_exit_releases_the_threads_async_hooks_resources_and_snapshots() {
    let (async_id, store_bits, live) = std::thread::spawn(|| {
        let als_payload = perry_runtime::async_context::AsyncLocalStoragePayload::default();
        let als_token = als_payload.token();
        let scope = RuntimeHandleScope::new();
        let resource = scope.root_raw_mut_ptr(perry_runtime::js_object_alloc(0, 0));
        let store = scope.root_raw_mut_ptr(perry_runtime::js_object_alloc(0, 0));
        let ids = perry_runtime::async_hooks::init_resource(
            "PERRY_11471",
            pointer_value(resource.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
            true,
        );
        perry_runtime::async_context::enter_with(
            als_token,
            pointer_value(store.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
        );
        let _snapshot =
            perry_runtime::async_hooks::js_async_local_storage_static_snapshot_direct(0);
        let store_bits =
            JSValue::pointer(store.get_raw_mut_ptr::<ObjectHeader>() as *const u8).bits();
        let live = perry_runtime::async_hooks::resource_tracked_for_test(ids.async_id)
            && perry_runtime::async_hooks::context_snapshot_holds_for_test(store_bits);
        (ids.async_id, store_bits, live)
    })
    .join()
    .unwrap();
    assert!(
        live,
        "the resource and the snapshot must be registered while their thread lives"
    );
    assert!(
        !perry_runtime::async_hooks::resource_tracked_for_test(async_id),
        "a dead thread's async resource outlived its heap"
    );
    assert!(
        !perry_runtime::async_hooks::context_snapshot_holds_for_test(store_bits),
        "a dead thread's AsyncLocalStorage snapshot outlived its heap"
    );
}

#[test]
fn thread_exit_clears_the_async_hooks_singletons_it_allocated() {
    let (top, providers, live) = std::thread::spawn(|| {
        perry_runtime::async_hooks::clear_cached_singletons_for_test();
        let top = perry_runtime::async_hooks::js_async_hooks_execution_async_resource().to_bits();
        let providers = perry_runtime::async_hooks::js_async_hooks_async_wrap_providers().to_bits();
        let cached = perry_runtime::async_hooks::cached_singletons_for_test();
        (top, providers, cached == (top, providers))
    })
    .join()
    .unwrap();
    assert!(
        live,
        "the singletons must be cached while their thread lives"
    );
    let (top_after, providers_after) = perry_runtime::async_hooks::cached_singletons_for_test();
    assert_ne!(
        top_after, top,
        "a dead thread's executionAsyncResource() object stayed cached"
    );
    assert_ne!(
        providers_after, providers,
        "a dead thread's asyncWrapProviders object stayed cached"
    );
}

/// The addresses the console.log singleton's root scanner reports right now,
/// read without allocating (a fresh allocation could land at the dead
/// thread's reused address and read as "still cached").
fn console_log_singleton_roots() -> Vec<u64> {
    let mut seen = Vec::new();
    perry_runtime::builtins::scan_console_log_singleton_roots(&mut |value| {
        seen.push(value.to_bits() & 0x0000_FFFF_FFFF_FFFF)
    });
    seen
}

#[test]
fn thread_exit_clears_the_console_log_singleton_it_allocated() {
    let (closure, live) = std::thread::spawn(|| {
        let bits = perry_runtime::builtins::js_console_log_as_closure().to_bits();
        let closure = bits & 0x0000_FFFF_FFFF_FFFF;
        (closure, console_log_singleton_roots().contains(&closure))
    })
    .join()
    .unwrap();
    assert!(live, "the singleton must be cached while its thread lives");
    assert!(
        !console_log_singleton_roots().contains(&closure),
        "a dead thread's console.log closure stayed cached process-wide"
    );
}

#[test]
fn thread_exit_clears_the_global_this_root_slot_it_wrote() {
    // Every thread's first `globalThis` overwrites the shared slot, so a
    // concurrent test can replace ours before we look; retry until one
    // attempt observes its own write.
    let mut observed = None;
    for _ in 0..50 {
        let (global, live) = std::thread::spawn(|| {
            let value = perry_runtime::object::js_get_global_this();
            let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
            (
                raw,
                perry_runtime::object::global_this_root_slot_for_test() == raw,
            )
        })
        .join()
        .unwrap();
        if live {
            observed = Some(global);
            break;
        }
    }
    let global = observed.expect("the root slot must name the thread's globalThis while it lives");
    assert_ne!(
        perry_runtime::object::global_this_root_slot_for_test(),
        global,
        "a dead thread's globalThis stayed in the process-global root slot"
    );
}

#[test]
fn thread_exit_releases_the_threads_tls_client_records() {
    const HANDLE: i64 = 0x1147_1002;
    let live = std::thread::spawn(|| {
        let scope = RuntimeHandleScope::new();
        let closure = scope.root_raw_mut_ptr(perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(thunk2, 2),
            0,
        ));
        let options = scope.root_raw_mut_ptr(perry_runtime::js_object_alloc(0, 1));
        set_field(
            options.get_raw_mut_ptr::<ObjectHeader>(),
            "checkServerIdentity",
            pointer_value(closure.get_raw_mut_ptr::<ClosureHeader>() as *const u8),
        );
        unsafe {
            perry_runtime::tls::js_tls_client_record_start(
                HANDLE,
                pointer_value(options.get_raw_mut_ptr::<ObjectHeader>() as *const u8),
                std::ptr::null(),
                0,
            );
        }
        perry_runtime::tls::tls_client_metadata(HANDLE)
            .is_some_and(|metadata| metadata.check_server_identity != 0)
    })
    .join()
    .unwrap();
    assert!(
        live,
        "the record must hold the thread's checkServerIdentity while it lives"
    );
    assert!(
        perry_runtime::tls::tls_client_metadata(HANDLE).is_none(),
        "a dead thread's checkServerIdentity record outlived its heap"
    );
}
