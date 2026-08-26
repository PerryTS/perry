use super::*;
use crate::value::JSValue;
use std::ffi::{c_char, c_void};

pub type NapiCallback = Option<unsafe extern "C" fn(NapiEnv, NapiCallbackInfo) -> NapiValue>;

fn callback_info<R>(
    env: NapiEnv,
    info: NapiCallbackInfo,
    f: impl FnOnce(&CallbackInfoRecord) -> R,
) -> Option<R> {
    if info.is_null() {
        return None;
    }
    with_env(env, |env| {
        let address = info as usize;
        if !env.active_callback_infos.contains(&address) {
            return None;
        }
        let info = unsafe { &*info.cast::<CallbackInfoRecord>() };
        (info.env_serial == env.serial).then(|| f(info))
    })
    .flatten()
}

fn current_callback_record(index: usize) -> Option<NativeCallbackRecord> {
    let env = current_env();
    with_env(env, |env| {
        env.callbacks.get(index).map(|record| NativeCallbackRecord {
            callback: record.callback,
            data: record.data,
        })
    })
    .flatten()
}

extern "C" fn napi_callback_thunk(
    closure: *const crate::closure::ClosureHeader,
    arguments: f64,
) -> f64 {
    let env = current_env();
    let callback_index = crate::closure::js_closure_get_capture_ptr(closure, 0).max(0) as usize;
    let Some(callback) = current_callback_record(callback_index) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    let callback_data = callback.data;
    let native_callback: unsafe extern "C" fn(NapiEnv, NapiCallbackInfo) -> NapiValue =
        unsafe { std::mem::transmute(callback.callback) };

    let mut scope = std::ptr::null_mut();
    if unsafe { napi_open_handle_scope(env, &mut scope) } != NapiStatus::Ok {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let mut argument_handles = Vec::new();
    let arguments_ptr =
        crate::value::js_nanbox_get_pointer(arguments) as *const crate::array::ArrayHeader;
    if !arguments_ptr.is_null() {
        let length = crate::array::js_array_length(arguments_ptr);
        argument_handles.reserve(length as usize);
        for index in 0..length {
            let bits = crate::array::js_array_get_f64(arguments_ptr, index).to_bits();
            if let Ok(handle) = add_handle(env, bits) {
                argument_handles.push(handle);
            }
        }
    }
    let this_bits = crate::object::js_implicit_this_get().to_bits();
    let this_value = add_handle(env, this_bits).unwrap_or(std::ptr::null_mut());

    let mut info = Box::new(CallbackInfoRecord {
        env_serial: with_env(env, |env| env.serial).unwrap_or_default(),
        args: argument_handles,
        this_value,
        data: callback_data,
        new_target: std::ptr::null_mut(),
    });
    let info_ptr = (&mut *info) as *mut CallbackInfoRecord as NapiCallbackInfo;
    with_env_mut(env, |env| env.active_callback_infos.push(info_ptr as usize));

    let returned = unsafe { native_callback(env, info_ptr) };
    let returned_bits = if returned.is_null() {
        crate::value::TAG_UNDEFINED
    } else {
        value_bits(env, returned).unwrap_or(crate::value::TAG_UNDEFINED)
    };

    with_env_mut(env, |env| {
        if let Some(position) = env
            .active_callback_infos
            .iter()
            .rposition(|address| *address == info_ptr as usize)
        {
            env.active_callback_infos.remove(position);
        }
    });
    unsafe { napi_close_handle_scope(env, scope) };
    drop(info);

    let exception = with_env_mut(env, |env| env.pending_exception_bits.take()).flatten();
    if let Some(exception) = exception {
        crate::exception::js_throw(f64::from_bits(exception));
    }
    f64::from_bits(returned_bits)
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_function(
    env: NapiEnv,
    utf8name: *const c_char,
    length: usize,
    callback: NapiCallback,
    data: *mut c_void,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || callback.is_none() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and callback must not be null",
        );
    }
    let name = if utf8name.is_null() {
        Vec::new()
    } else {
        let length = if length == NAPI_AUTO_LENGTH {
            std::ffi::CStr::from_ptr(utf8name).to_bytes().len()
        } else {
            length
        };
        std::slice::from_raw_parts(utf8name.cast::<u8>(), length).to_vec()
    };
    let callback = callback.unwrap() as usize;
    let callback_index = match with_env_mut(env, |env| {
        let index = env.callbacks.len();
        env.callbacks.push(NativeCallbackRecord {
            callback,
            data: data as usize,
        });
        index
    }) {
        Some(index) => index,
        None => return NapiStatus::InvalidArg,
    };

    let function_pointer = napi_callback_thunk as *const u8;
    crate::closure::js_register_closure_synthetic_arguments(function_pointer, 0);
    crate::closure::js_register_closure_arity(function_pointer, 0);
    crate::closure::js_register_closure_length(function_pointer, 0);
    let closure = crate::closure::js_closure_alloc(function_pointer, 1);
    crate::closure::js_closure_set_capture_ptr(closure, 0, callback_index as i64);
    let handle = match add_handle(env, JSValue::pointer(closure.cast()).bits()) {
        Ok(handle) => handle,
        Err(status) => return status,
    };

    if !name.is_empty() {
        let name_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_value = f64::from_bits(JSValue::string_ptr(name_ptr).bits());
        let closure_bits = match value_bits(env, handle) {
            Ok(bits) => bits,
            Err(status) => return status,
        };
        let closure_ptr = JSValue::from_bits(closure_bits).as_pointer::<u8>() as usize;
        crate::closure::closure_set_dynamic_prop(closure_ptr, "name", name_value);
    }
    *result = handle;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_cb_info(
    env: NapiEnv,
    info: NapiCallbackInfo,
    argc: *mut usize,
    argv: *mut NapiValue,
    this_arg: *mut NapiValue,
    data: *mut *mut c_void,
) -> NapiStatus {
    if argc.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "argc must not be null");
    }
    let capacity = *argc;
    let Some((args, this_value, callback_data)) = callback_info(env, info, |info| {
        (info.args.clone(), info.this_value, info.data)
    }) else {
        return set_status(env, NapiStatus::InvalidArg, "callback info is not active");
    };
    if !argv.is_null() {
        for (index, argument) in args.iter().take(capacity).enumerate() {
            *argv.add(index) = *argument;
        }
    }
    *argc = args.len();
    if !this_arg.is_null() {
        *this_arg = this_value;
    }
    if !data.is_null() {
        *data = callback_data as *mut c_void;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_new_target(
    env: NapiEnv,
    info: NapiCallbackInfo,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let Some(new_target) = callback_info(env, info, |info| info.new_target) else {
        return set_status(env, NapiStatus::InvalidArg, "callback info is not active");
    };
    *result = new_target;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_call_function(
    env: NapiEnv,
    recv: NapiValue,
    function: NapiValue,
    argc: usize,
    argv: *const NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    if pending_exception(env).is_some() {
        return set_status(env, NapiStatus::PendingException, "an exception is pending");
    }
    if recv.is_null() || (argc != 0 && argv.is_null()) {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "invalid receiver or argument vector",
        );
    }
    let Ok(receiver_bits) = value_bits(env, recv) else {
        return set_status(env, NapiStatus::InvalidArg, "receiver is not a live handle");
    };
    let Ok(function_bits) = value_bits(env, function) else {
        return set_status(env, NapiStatus::InvalidArg, "function is not a live handle");
    };
    let function_value = JSValue::from_bits(function_bits);
    if !function_value.is_pointer()
        || !crate::closure::is_closure_ptr(function_value.as_pointer::<u8>() as usize)
    {
        return set_status(env, NapiStatus::FunctionExpected, "value must be callable");
    }
    let mut arguments = Vec::with_capacity(argc);
    for index in 0..argc {
        let handle = *argv.add(index);
        let Ok(bits) = value_bits(env, handle) else {
            return set_status(env, NapiStatus::InvalidArg, "argument is not a live handle");
        };
        arguments.push(f64::from_bits(bits));
    }
    let previous_this = crate::object::js_implicit_this_set(f64::from_bits(receiver_bits));
    let call_result = catch_value_call(env, || {
        crate::closure::js_native_call_value(
            f64::from_bits(function_bits),
            arguments.as_ptr(),
            arguments.len(),
        )
    });
    crate::object::js_implicit_this_set(previous_this);
    match call_result {
        Ok(value) => {
            if !result.is_null() {
                let Ok(handle) = add_handle(env, value.to_bits()) else {
                    return NapiStatus::InvalidArg;
                };
                *result = handle;
            }
            ok(env)
        }
        Err(status) => status,
    }
}
