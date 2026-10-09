//! Exception-safe callback bridges for separately linked async providers.

use super::{
    destroy, init_resource, init_resource_with_trigger, try_enter_resource_scope,
    try_leave_resource_scope, AsyncResourceIds, RESOURCES,
};

/// A native provider whose metadata lifetime follows an ordinary GC object.
/// The binding holds this object in JS state; its Rust payload holds only the
/// existing async ids. No socket registry or native owner address is created.
///
/// # Safety
/// type_ptr/type_len is UTF-8 valid for this call. MAX selects the current
/// execution id; every other trigger is passed through unchanged.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_owned_provider_new(
    type_ptr: *const u8,
    type_len: usize,
    trigger_async_id: u64,
) -> f64 {
    if type_ptr.is_null() {
        return super::TAG_UNDEFINED_F64;
    }
    let name = std::str::from_utf8_unchecked(std::slice::from_raw_parts(type_ptr, type_len));
    let trigger = if trigger_async_id == u64::MAX {
        super::execution_async_id_u64()
    } else {
        trigger_async_id
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    // Match the existing native-provider resource's observable prototype.
    let owner = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc_null_proto(0, 0) as i64,
    ));
    let payload = super::AsyncResourcePayload {
        ids: AsyncResourceIds {
            async_id: 0,
            trigger_async_id: trigger,
        },
    };
    if !crate::native_payload::attach_to_object(
        owner.get_nanbox_f64(),
        &super::ASYNC_RESOURCE_FAMILY,
        payload,
        std::mem::size_of::<super::AsyncResourcePayload>(),
    ) {
        return super::TAG_UNDEFINED_F64;
    }
    let ids = super::init_resource_metadata(name, owner.get_nanbox_f64(), true, trigger);
    let raw = crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64());
    if let Some(payload) = super::resource_payload(raw) {
        payload.ids = ids;
    }
    let _ = super::swap_resource_value(ids.async_id, super::TAG_UNDEFINED_F64);
    owner.get_nanbox_f64()
}

/// Emit init after the binding has published the provider as an ordinary JS
/// edge and copied its ids into native fields. An init hook can then close or
/// reopen the owner without leaving work that mutates its old native state.
#[no_mangle]
pub extern "C" fn js_async_hooks_owned_provider_emit_init(resource: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resource = scope.root_nanbox_f64(resource);
    let raw = crate::value::js_nanbox_get_pointer(resource.get_nanbox_f64());
    let Some(ids) = (unsafe { super::resource_payload(raw) }).map(|payload| payload.ids) else {
        return;
    };
    let name = RESOURCES
        .lock()
        .unwrap()
        .get(&ids.async_id)
        .map(|meta| meta.type_name.clone());
    if let Some(name) = name {
        super::emit_init(
            ids.async_id,
            &name,
            ids.trigger_async_id,
            resource.get_nanbox_f64(),
        );
    }
}

/// Run a native event in an owned provider's scope, including temporary
/// executionAsyncResource publication. Restore context and that publication
/// before returning an exception to the outer native event boundary.
pub(crate) fn owned_provider_scope(
    resource: f64,
    callback: impl FnOnce() -> f64,
) -> Result<f64, f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(resource);
    owned_provider_scope_rooted(&scope, &owner, callback)
}

/// Share the event boundary's roots; before/after hooks may move its inputs.
pub(crate) fn owned_provider_scope_rooted(
    scope: &crate::gc::RuntimeHandleScope,
    owner: &crate::gc::RuntimeHandle<'_>,
    callback: impl FnOnce() -> f64,
) -> Result<f64, f64> {
    if !crate::value::JSValue::from_bits(owner.get_nanbox_f64().to_bits()).is_pointer() {
        return crate::exception::js_call_catching(callback);
    }
    let raw = crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64());
    let Some(ids) = (unsafe { super::resource_payload(raw) }).map(|payload| payload.ids) else {
        return crate::exception::js_call_catching(callback);
    };
    let previous = scope.root_nanbox_f64(super::swap_resource_value(
        ids.async_id,
        owner.get_nanbox_f64(),
    ));
    let outcome = super::try_run_resource_scope(ids, callback);
    // A returned exception/result may itself move while another native cleanup
    // runs, so keep it in the same handle scope until publication is restored.
    let (threw, value) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let _ = super::swap_resource_value(ids.async_id, previous.get_nanbox_f64());
    if threw {
        Err(value.get_nanbox_f64())
    } else {
        Ok(value.get_nanbox_f64())
    }
}

/// End a JS-owned native provider after its terminal event. A hook exception
/// cannot retain its metadata; like other async-hook exceptions it is fatal.
#[no_mangle]
pub extern "C" fn js_async_hooks_owned_provider_destroy(resource: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_nanbox_f64(resource);
    if !crate::value::JSValue::from_bits(owner.get_nanbox_f64().to_bits()).is_pointer() {
        return;
    }
    let raw = crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64());
    let Some(id) = (unsafe { super::resource_payload(raw) }).map(|payload| payload.ids.async_id)
    else {
        return;
    };
    let outcome = crate::exception::js_call_catching(|| {
        destroy(id);
        super::TAG_UNDEFINED_F64
    });
    if let Err(error) = outcome {
        RESOURCES.lock().unwrap().remove(&id);
        crate::exception::exit_on_uncaught(error);
    }
}

extern "C" fn deferred_destroy_step(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(
        crate::fn_info!(deferred_destroy_step, 0; with_declared(0)),
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

fn provider_ids(async_id: u64) -> AsyncResourceIds {
    let trigger_async_id = RESOURCES
        .lock()
        .unwrap()
        .get(&async_id)
        .map(|meta| meta.trigger_async_id)
        .unwrap_or(0);
    AsyncResourceIds {
        async_id,
        trigger_async_id,
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
    if let Err(error) = try_enter_resource_scope(provider_ids(async_id)) {
        crate::exception::js_throw(error);
    }
}

#[no_mangle]
pub extern "C" fn js_async_hooks_provider_leave(async_id: u64) {
    if let Err(error) = try_leave_resource_scope(async_id) {
        crate::exception::js_throw(error);
    }
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
    provider_run_catching(async_id, DestroyPolicy::Never, callback, data)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DestroyPolicy {
    Never,
    Always(u32),
    OnError(u32),
}

/// Variant used by terminal external-provider events. Teardown is scheduled
/// after scope restoration and before a caught JavaScript exception is
/// rethrown, so a throwing listener cannot strand the resource.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_deferred_destroy(
    async_id: u64,
    check_turns: u32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    provider_run_catching(async_id, DestroyPolicy::Always(check_turns), callback, data)
}

/// Schedule terminal teardown only when scope entry, the callback, or scope
/// exit throws. This lets a multi-phase provider protect an early phase while
/// leaving its normal destroy timing to the final phase.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_deferred_destroy_on_error(
    async_id: u64,
    check_turns: u32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    provider_run_catching(
        async_id,
        DestroyPolicy::OnError(check_turns),
        callback,
        data,
    )
}

unsafe fn provider_run_catching(
    async_id: u64,
    destroy_policy: DestroyPolicy,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    if let Err(error) = try_enter_resource_scope(provider_ids(async_id)) {
        let error = scope.root_nanbox_f64(error);
        if let DestroyPolicy::Always(turns) | DestroyPolicy::OnError(turns) = destroy_policy {
            defer_destroy_after_check_turns(async_id, turns);
        }
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    let outcome = crate::exception::js_call_catching(|| callback(data));
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let leave = try_leave_resource_scope(async_id);
    let (leave_threw, leave_result) = match leave {
        Ok(()) => (
            false,
            scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED)),
        ),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    if let DestroyPolicy::Always(turns) = destroy_policy {
        defer_destroy_after_check_turns(async_id, turns);
    } else if let DestroyPolicy::OnError(turns) = destroy_policy {
        if threw || leave_threw {
            defer_destroy_after_check_turns(async_id, turns);
        }
    }
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    if leave_threw {
        crate::exception::js_throw(leave_result.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}

/// Provider callback wrapper for external EventEmitter-style dispatch. It
/// additionally roots the listener receiver for the duration of the callback
/// and can retire a one-shot provider before propagating a JavaScript
/// exception. `callback` makes its own JS calls and passes them the receiver
/// itself: nothing here binds `this` for it.
#[no_mangle]
pub unsafe extern "C" fn js_async_hooks_provider_run_catching_with_this(
    async_id: u64,
    this_value: f64,
    destroy_after: i32,
    callback: unsafe extern "C" fn(*mut std::ffi::c_void) -> f64,
    data: *mut std::ffi::c_void,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let _receiver_root = scope.root_nanbox_f64(this_value);
    if let Err(error) = try_enter_resource_scope(provider_ids(async_id)) {
        let error = scope.root_nanbox_f64(error);
        if destroy_after != 0 {
            let _ = crate::exception::js_call_catching(|| {
                destroy(async_id);
                f64::from_bits(crate::value::TAG_UNDEFINED)
            });
        }
        crate::exception::js_throw(error.get_nanbox_f64());
    }
    let outcome = crate::exception::js_call_catching(|| callback(data));
    let (threw, result) = match outcome {
        Ok(value) => (false, scope.root_nanbox_f64(value)),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let leave = try_leave_resource_scope(async_id);
    let (leave_threw, leave_result) = match leave {
        Ok(()) => (
            false,
            scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED)),
        ),
        Err(error) => (true, scope.root_nanbox_f64(error)),
    };
    let destroy_outcome = (destroy_after != 0).then(|| {
        crate::exception::js_call_catching(|| {
            destroy(async_id);
            f64::from_bits(crate::value::TAG_UNDEFINED)
        })
    });
    let (destroy_threw, destroy_result) = match destroy_outcome {
        Some(Err(error)) => (true, scope.root_nanbox_f64(error)),
        _ => (
            false,
            scope.root_nanbox_f64(f64::from_bits(crate::value::TAG_UNDEFINED)),
        ),
    };
    if threw {
        crate::exception::js_throw(result.get_nanbox_f64());
    }
    if leave_threw {
        crate::exception::js_throw(leave_result.get_nanbox_f64());
    }
    if destroy_threw {
        crate::exception::js_throw(destroy_result.get_nanbox_f64());
    }
    result.get_nanbox_f64()
}
