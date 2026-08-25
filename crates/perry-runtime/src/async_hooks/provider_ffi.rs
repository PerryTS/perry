//! Exception-safe callback bridges for separately linked async providers.

use super::{
    destroy, enter_resource_scope, init_resource, init_resource_with_trigger, leave_resource_scope,
    AsyncResourceIds, RESOURCES,
};

extern "C" fn deferred_destroy_step(closure: *const crate::closure::ClosureHeader) -> f64 {
    let async_id = crate::closure::js_closure_get_capture_f64(closure, 0) as u64;
    let remaining = crate::closure::js_closure_get_capture_f64(closure, 1) as u32;
    if remaining == 0 {
        destroy(async_id);
    } else {
        schedule_deferred_destroy_step(async_id, remaining - 1);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn schedule_deferred_destroy_step(async_id: u64, remaining: u32) {
    crate::closure::js_register_closure_arity(deferred_destroy_step as *const u8, 0);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        deferred_destroy_step as *const u8,
        2,
    ));
    callback.with_mut_ptr(|callback| {
        crate::closure::js_closure_set_capture_f64(callback, 0, async_id as f64);
        crate::closure::js_closure_set_capture_f64(callback, 1, remaining as f64);
        crate::timer::js_set_immediate_callback(callback as i64);
    });
}

/// Retire a native provider after a fixed number of check phases. libuv handle
/// close callbacks do not fire synchronously with APIs such as `unwatchFile`
/// or one-shot zlib completion, so their destroy hooks must remain observable
/// only after the corresponding close turns have run.
pub fn defer_destroy_after_check_turns(async_id: u64, check_turns: u32) {
    if async_id == 0 {
        return;
    }
    if check_turns == 0 {
        destroy(async_id);
    } else {
        schedule_deferred_destroy_step(async_id, check_turns - 1);
    }
}

/// C ABI used by separately-linked native providers such as perry-ext-zlib.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_init(type_ptr: *const u8, type_len: usize) -> u64 {
    if type_ptr.is_null() {
        return 0;
    }
    let type_name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(type_ptr, type_len));
    let resource = crate::object::js_object_alloc_null_proto(0, 0);
    init_resource(
        type_name,
        crate::value::js_nanbox_pointer(resource as i64),
        true,
    )
    .async_id
}

#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_init_with_trigger(
    type_ptr: *const u8,
    type_len: usize,
    trigger_async_id: u64,
) -> u64 {
    if type_ptr.is_null() {
        return 0;
    }
    let type_name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(type_ptr, type_len));
    let resource = crate::object::js_object_alloc_null_proto(0, 0);
    init_resource_with_trigger(
        type_name,
        crate::value::js_nanbox_pointer(resource as i64),
        true,
        trigger_async_id,
    )
    .async_id
}

#[no_mangle]
pub extern "C" fn js_async_hooks_provider_enter(async_id: u64) {
    let trigger_async_id = RESOURCES
        .lock()
        .unwrap()
        .get(&async_id)
        .map(|meta| meta.trigger_async_id)
        .unwrap_or(0);
    enter_resource_scope(AsyncResourceIds {
        async_id,
        trigger_async_id,
    });
}

#[no_mangle]
pub extern "C" fn js_async_hooks_provider_leave(async_id: u64) {
    leave_resource_scope(async_id);
}

#[no_mangle]
pub extern "C" fn js_async_hooks_provider_destroy(async_id: u64) {
    destroy(async_id);
}

#[no_mangle]
pub extern "C" fn js_async_hooks_provider_defer_destroy(async_id: u64, check_turns: u32) {
    defer_destroy_after_check_turns(async_id, check_turns);
}

/// Run an external-provider callback while guaranteeing that the provider
/// scope is restored before a JS exception resumes unwinding into generated
/// code. Rust `Drop` guards cannot provide this guarantee because Perry's JS
/// exception transport deliberately skips runtime Rust cleanup frames.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching(
    async_id: u64,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    js_async_hooks_provider_enter(async_id);
    let outcome = crate::exception::js_call_catching(|| callback(data));
    let scope = crate::gc::RuntimeHandleScope::new();
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    js_async_hooks_provider_leave(async_id);
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}

/// Provider callback wrapper for external EventEmitter-style dispatch. It
/// additionally restores implicit `this` and can retire a one-shot provider
/// before propagating a JavaScript exception.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_with_this(
    async_id: u64,
    this_value: f64,
    destroy_after: i32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_value = scope.root_nanbox_f64(this_value);
    js_async_hooks_provider_enter(async_id);
    let previous_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        this_value.get_nanbox_f64(),
    ));
    let outcome = crate::exception::js_call_catching(|| callback(data));
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    crate::object::js_implicit_this_set(previous_this.get_nanbox_f64());
    js_async_hooks_provider_leave(async_id);
    if destroy_after != 0 {
        js_async_hooks_provider_destroy(async_id);
    }
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}
