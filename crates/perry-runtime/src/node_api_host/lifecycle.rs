use super::*;
use crate::async_hooks::AsyncResourceIds;
use crate::value::JSValue;
use std::ffi::{c_char, c_void};
use std::sync::LazyLock;

pub type NapiCleanupHook = Option<unsafe extern "C" fn(*mut c_void)>;
pub type NapiAsyncCleanupHook =
    Option<unsafe extern "C" fn(NapiAsyncCleanupHookHandle, *mut c_void)>;

pub(crate) struct AsyncContextRecord {
    pub env_serial: u64,
    pub ids: AsyncResourceIds,
    pub destroyed: bool,
}

pub(crate) struct CallbackScopeRecord {
    pub env_serial: u64,
    pub context: usize,
    pub closed: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CleanupHookRecord {
    pub callback: usize,
    pub argument: usize,
}

pub(crate) struct AsyncCleanupHookRecord {
    pub env_serial: u64,
    pub callback: usize,
    pub argument: usize,
    pub active: bool,
}

pub(crate) struct InstanceDataRecord {
    pub data: usize,
    pub finalizer: Option<super::metadata::FinalizerRecord>,
}

#[repr(C)]
pub struct NapiNodeVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub release: *const c_char,
}

unsafe impl Send for NapiNodeVersion {}
unsafe impl Sync for NapiNodeVersion {}

static NODE_VERSION: LazyLock<NapiNodeVersion> = LazyLock::new(|| NapiNodeVersion {
    major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap_or(0),
    minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap_or(0),
    patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap_or(0),
    release: c"perry".as_ptr(),
});

fn string_value(env: NapiEnv, value: NapiValue) -> Result<String, NapiStatus> {
    let bits = value_bits(env, value)?;
    if !JSValue::from_bits(bits).is_any_string() {
        return Err(NapiStatus::StringExpected);
    }
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((pointer, length)) =
        crate::string::str_bytes_from_jsvalue(f64::from_bits(bits), &mut scratch)
    else {
        return Err(NapiStatus::StringExpected);
    };
    let bytes = if length == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(pointer, length as usize) }
    };
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

#[no_mangle]
pub unsafe extern "C" fn napi_async_init(
    env: NapiEnv,
    async_resource: NapiValue,
    async_resource_name: NapiValue,
    result: *mut NapiAsyncContext,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let resource_bits = if async_resource.is_null() {
        crate::value::TAG_UNDEFINED
    } else {
        match value_bits(env, async_resource) {
            Ok(bits) => bits,
            Err(status) => return set_status(env, status, "resource is not a live handle"),
        }
    };
    let name = match string_value(env, async_resource_name) {
        Ok(name) => name,
        Err(status) => return set_status(env, status, "resource name must be a string"),
    };
    let ids = crate::async_hooks::init_resource(&name, f64::from_bits(resource_bits), true);
    let context = with_env_mut(env, |env| {
        let mut record = Box::new(AsyncContextRecord {
            env_serial: env.serial,
            ids,
            destroyed: false,
        });
        let pointer = (&mut *record) as *mut AsyncContextRecord as NapiAsyncContext;
        env.async_context_lookup
            .insert(pointer as usize, env.async_contexts.len());
        env.async_contexts.push(record);
        pointer
    });
    let Some(context) = context else {
        return NapiStatus::InvalidArg;
    };
    *result = context;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_async_destroy(env: NapiEnv, context: NapiAsyncContext) -> NapiStatus {
    let ids = with_env_mut(env, |env| {
        let record = env.async_context_mut(context)?;
        record.destroyed = true;
        Some(record.ids)
    })
    .flatten();
    let Some(ids) = ids else {
        return set_status(env, NapiStatus::InvalidArg, "async context is not live");
    };
    match catch_value_call(env, || {
        crate::async_hooks::destroy(ids.async_id);
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }) {
        Ok(_) => ok(env),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_make_callback(
    env: NapiEnv,
    context: NapiAsyncContext,
    recv: NapiValue,
    function: NapiValue,
    argc: usize,
    argv: *const NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    let ids = with_env(env, |env| {
        env.async_context(context).map(|record| record.ids)
    })
    .flatten();
    let Some(ids) = ids else {
        return set_status(env, NapiStatus::InvalidArg, "async context is not live");
    };
    if let Err(error) = crate::async_hooks::try_enter_resource_scope(ids) {
        return store_pending_exception(env, error.to_bits());
    }
    let call_status = napi_call_function(env, recv, function, argc, argv, result);
    let leave = crate::async_hooks::try_leave_resource_scope(ids.async_id);
    if call_status != NapiStatus::Ok {
        return call_status;
    }
    if let Err(error) = leave {
        return store_pending_exception(env, error.to_bits());
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_open_callback_scope(
    env: NapiEnv,
    resource_object: NapiValue,
    context: NapiAsyncContext,
    result: *mut NapiCallbackScope,
) -> NapiStatus {
    if result.is_null() || (!resource_object.is_null() && value_bits(env, resource_object).is_err())
    {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "callback scope arguments are invalid",
        );
    }
    if with_env(env, |env| env.async_context(context).is_some()) != Some(true) {
        return set_status(env, NapiStatus::InvalidArg, "async context is not live");
    }
    let scope = with_env_mut(env, |env| {
        let mut record = Box::new(CallbackScopeRecord {
            env_serial: env.serial,
            context: context as usize,
            closed: false,
        });
        let pointer = (&mut *record) as *mut CallbackScopeRecord as NapiCallbackScope;
        env.callback_scope_stack
            .push(env.callback_scope_tokens.len());
        env.callback_scope_tokens.push(record);
        pointer
    });
    let Some(scope) = scope else {
        return NapiStatus::InvalidArg;
    };
    *result = scope;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_close_callback_scope(
    env: NapiEnv,
    scope: NapiCallbackScope,
) -> NapiStatus {
    if scope.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "callback scope must not be null",
        );
    }
    with_env_mut(env, |env| {
        let Some(&top) = env.callback_scope_stack.last() else {
            return env.set_status(
                NapiStatus::CallbackScopeMismatch,
                "callback scopes must close in LIFO order",
            );
        };
        let Some(record) = env.callback_scope_tokens.get_mut(top).map(Box::as_mut) else {
            return env.set_status(NapiStatus::InvalidArg, "callback scope is unknown");
        };
        if !std::ptr::eq(record, scope.cast::<CallbackScopeRecord>())
            || record.env_serial != env.serial
            || record.closed
        {
            return env.set_status(
                NapiStatus::CallbackScopeMismatch,
                "callback scopes must close in LIFO order",
            );
        }
        record.closed = true;
        env.callback_scope_stack.pop();
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_set_instance_data(
    env: NapiEnv,
    data: *mut c_void,
    finalize_cb: NapiFinalize,
    finalize_hint: *mut c_void,
) -> NapiStatus {
    let replacement = InstanceDataRecord {
        data: data as usize,
        finalizer: super::metadata::finalizer(finalize_cb, data, finalize_hint),
    };
    let replaced = with_env_mut(env, |env| {
        env.instance_data = Some(replacement);
    });
    if replaced.is_none() {
        return NapiStatus::InvalidArg;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_instance_data(
    env: NapiEnv,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let data = with_env(env, |env| env.instance_data.as_ref().map(|data| data.data));
    let Some(data) = data else {
        return NapiStatus::InvalidArg;
    };
    *result = data.unwrap_or(0) as *mut c_void;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_add_env_cleanup_hook(
    env: NapiEnv,
    callback: NapiCleanupHook,
    argument: *mut c_void,
) -> NapiStatus {
    let Some(callback) = callback else {
        return set_status(env, NapiStatus::InvalidArg, "cleanup hook must not be null");
    };
    if with_env_mut(env, |env| {
        env.cleanup_hooks.push(CleanupHookRecord {
            callback: callback as usize,
            argument: argument as usize,
        });
    })
    .is_none()
    {
        return NapiStatus::InvalidArg;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_remove_env_cleanup_hook(
    env: NapiEnv,
    callback: NapiCleanupHook,
    argument: *mut c_void,
) -> NapiStatus {
    let Some(callback) = callback else {
        return set_status(env, NapiStatus::InvalidArg, "cleanup hook must not be null");
    };
    let removed = with_env_mut(env, |env| {
        env.cleanup_hooks
            .iter()
            .rposition(|hook| {
                hook.callback == callback as usize && hook.argument == argument as usize
            })
            .map(|index| env.cleanup_hooks.remove(index))
            .is_some()
    });
    match removed {
        Some(true) => ok(env),
        Some(false) => set_status(
            env,
            NapiStatus::InvalidArg,
            "cleanup hook was not registered",
        ),
        None => NapiStatus::InvalidArg,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_add_async_cleanup_hook(
    env: NapiEnv,
    callback: NapiAsyncCleanupHook,
    argument: *mut c_void,
    result: *mut NapiAsyncCleanupHookHandle,
) -> NapiStatus {
    if result.is_null() || callback.is_none() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "async cleanup callback and result must not be null",
        );
    }
    let callback = callback.unwrap();
    let handle = with_env_mut(env, |env| {
        let mut record = Box::new(AsyncCleanupHookRecord {
            env_serial: env.serial,
            callback: callback as usize,
            argument: argument as usize,
            active: true,
        });
        let pointer = (&mut *record) as *mut AsyncCleanupHookRecord as NapiAsyncCleanupHookHandle;
        env.async_cleanup_lookup
            .insert(pointer as usize, env.async_cleanup_hooks.len());
        env.async_cleanup_hooks.push(record);
        pointer
    });
    let Some(handle) = handle else {
        return NapiStatus::InvalidArg;
    };
    *result = handle;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_remove_async_cleanup_hook(
    handle: NapiAsyncCleanupHookHandle,
) -> NapiStatus {
    let env = current_env();
    with_env_mut(env, |env| {
        let Some(&index) = env.async_cleanup_lookup.get(&(handle as usize)) else {
            return env.set_status(NapiStatus::InvalidArg, "async cleanup hook is unknown");
        };
        let Some(record) = env.async_cleanup_hooks.get_mut(index).map(Box::as_mut) else {
            return env.set_status(NapiStatus::InvalidArg, "async cleanup hook is unknown");
        };
        if record.env_serial != env.serial || !record.active {
            return env.set_status(NapiStatus::InvalidArg, "async cleanup hook is not active");
        }
        record.active = false;
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_node_version(
    env: NapiEnv,
    result: *mut *const NapiNodeVersion,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = &*NODE_VERSION;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_uv_event_loop(
    env: NapiEnv,
    result: *mut *mut c_void,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = std::ptr::null_mut();
    set_status(
        env,
        NapiStatus::GenericFailure,
        "Perry has no libuv event loop; direct uv_* use is unsupported",
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_fatal_exception(env: NapiEnv, error: NapiValue) -> NapiStatus {
    let bits = match value_bits(env, error) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "error is not a live handle"),
    };
    if with_env_mut(env, |env| env.pending_exception_bits = Some(bits)).is_none() {
        return NapiStatus::InvalidArg;
    }
    ok(env)
}

/// Idempotent owning-agent shutdown. Cleanup callbacks and finalizers run only
/// after all internal borrows have been released.
pub fn shutdown_current_env() {
    let env_pointer = current_env();
    let state = with_env_mut(env_pointer, |env| {
        if env.shutting_down {
            return None;
        }
        env.shutting_down = true;
        let async_hooks = env
            .async_cleanup_hooks
            .iter()
            .filter(|record| record.active)
            .map(|record| {
                (
                    record.callback,
                    record.argument,
                    (&**record) as *const _ as usize,
                )
            })
            .collect::<Vec<_>>();
        Some((
            async_hooks,
            std::mem::take(&mut env.cleanup_hooks),
            env.instance_data.take(),
        ))
    })
    .flatten();
    let Some((async_hooks, cleanup_hooks, instance_data)) = state else {
        return;
    };
    super::async_work::cancel_env_async_work(env_pointer);
    super::tsfn::shutdown_threadsafe_functions(env_pointer);
    super::process_pending();
    for (callback, argument, handle) in async_hooks {
        let callback: unsafe extern "C" fn(NapiAsyncCleanupHookHandle, *mut c_void) =
            unsafe { std::mem::transmute(callback) };
        unsafe {
            callback(
                handle as NapiAsyncCleanupHookHandle,
                argument as *mut c_void,
            );
        }
    }
    for hook in cleanup_hooks.into_iter().rev() {
        let callback: unsafe extern "C" fn(*mut c_void) =
            unsafe { std::mem::transmute(hook.callback) };
        unsafe {
            callback(hook.argument as *mut c_void);
        }
    }
    if let Some(instance_data) = instance_data {
        if let Some(finalizer) = instance_data.finalizer {
            super::metadata::enqueue_finalizer(finalizer);
        }
    }
    super::metadata::enqueue_all_object_finalizers();
    super::process_pending();
    super::loader::close_loaded_addons(env_pointer);
}
