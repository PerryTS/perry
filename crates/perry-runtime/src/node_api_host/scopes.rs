use super::*;
use std::ffi::{c_char, c_void};

fn open_scope(env: NapiEnv, escapable: bool, result: *mut *mut c_void) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let scope = with_env_mut(env, |env| {
        let depth = env.scopes.len() as u32 + 1;
        let mut token = Box::new(ScopeToken {
            env_serial: env.serial,
            depth,
            escapable,
            escaped: false,
            closed: false,
        });
        let ptr = (&mut *token) as *mut ScopeToken;
        env.scopes.push(env.scope_tokens.len());
        env.scope_tokens.push(token);
        ptr.cast::<c_void>()
    });
    let Some(scope) = scope else {
        return NapiStatus::InvalidArg;
    };
    unsafe { *result = scope };
    ok(env)
}

fn close_scope(env: NapiEnv, scope: *mut c_void, escapable: bool) -> NapiStatus {
    if scope.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "scope must not be null");
    }
    with_env_mut(env, |env| {
        let Some(&top) = env.scopes.last() else {
            return env.set_status(
                NapiStatus::HandleScopeMismatch,
                "handle scopes must close in LIFO order",
            );
        };
        let Some(token) = env.scope_tokens.get_mut(top).map(Box::as_mut) else {
            return env.set_status(NapiStatus::InvalidArg, "unknown handle scope");
        };
        if !std::ptr::eq(token, scope.cast::<ScopeToken>()) {
            return env.set_status(
                NapiStatus::HandleScopeMismatch,
                "handle scopes must close in LIFO order",
            );
        }
        if token.closed || token.env_serial != env.serial || token.escapable != escapable {
            return env.set_status(
                NapiStatus::HandleScopeMismatch,
                "handle scope kind mismatch",
            );
        }
        let depth = token.depth;
        token.closed = true;
        env.scopes.pop();
        env.invalidate_scope(depth);
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_open_handle_scope(
    env: NapiEnv,
    result: *mut NapiHandleScope,
) -> NapiStatus {
    open_scope(env, false, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_close_handle_scope(
    env: NapiEnv,
    scope: NapiHandleScope,
) -> NapiStatus {
    close_scope(env, scope, false)
}

#[no_mangle]
pub unsafe extern "C" fn napi_open_escapable_handle_scope(
    env: NapiEnv,
    result: *mut NapiEscapableHandleScope,
) -> NapiStatus {
    open_scope(env, true, result)
}

#[no_mangle]
pub unsafe extern "C" fn napi_close_escapable_handle_scope(
    env: NapiEnv,
    scope: NapiEscapableHandleScope,
) -> NapiStatus {
    close_scope(env, scope, true)
}

#[no_mangle]
pub unsafe extern "C" fn napi_escape_handle(
    env: NapiEnv,
    scope: NapiEscapableHandleScope,
    escapee: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    if scope.is_null() || result.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "scope and result must not be null",
        );
    }
    let Ok(bits) = value_bits(env, escapee) else {
        return set_status(env, NapiStatus::InvalidArg, "escapee is not a live handle");
    };
    let escaped = with_env_mut(env, |env| {
        let Some(&top) = env.scopes.last() else {
            return Err(NapiStatus::HandleScopeMismatch);
        };
        let Some(token) = env.scope_tokens.get_mut(top).map(Box::as_mut) else {
            return Err(NapiStatus::InvalidArg);
        };
        if !std::ptr::eq(token, scope.cast::<ScopeToken>()) {
            return Err(NapiStatus::HandleScopeMismatch);
        }
        if !token.escapable || token.closed {
            return Err(NapiStatus::HandleScopeMismatch);
        }
        if token.escaped {
            return Err(NapiStatus::EscapeCalledTwice);
        }
        token.escaped = true;
        let parent_depth = token.depth.saturating_sub(1);
        Ok(env.add_handle_at_depth(bits, parent_depth))
    });
    match escaped {
        Some(Ok(handle)) => {
            *result = handle;
            ok(env)
        }
        Some(Err(NapiStatus::EscapeCalledTwice)) => set_status(
            env,
            NapiStatus::EscapeCalledTwice,
            "an escapable handle scope may escape only once",
        ),
        Some(Err(status)) => set_status(env, status, "handle scope mismatch"),
        None => NapiStatus::InvalidArg,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_reference(
    env: NapiEnv,
    value: NapiValue,
    initial_refcount: u32,
    result: *mut NapiRef,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    if initial_refcount == 0 {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "weak Node-API references are not enabled in this host core",
        );
    }
    let Ok(bits) = value_bits(env, value) else {
        return set_status(env, NapiStatus::InvalidArg, "value is not a live handle");
    };
    let reference = with_env_mut(env, |env| {
        let mut record = Box::new(ReferenceRecord {
            env_serial: env.serial,
            value_bits: bits,
            refcount: initial_refcount,
            deleted: false,
        });
        let ptr = (&mut *record) as *mut ReferenceRecord as NapiRef;
        env.reference_lookup
            .insert(ptr as usize, env.references.len());
        env.references.push(record);
        ptr
    });
    let Some(reference) = reference else {
        return NapiStatus::InvalidArg;
    };
    *result = reference;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_delete_reference(env: NapiEnv, reference: NapiRef) -> NapiStatus {
    with_env_mut(env, |env| {
        let Some(reference) = env.reference_mut(reference) else {
            return env.set_status(NapiStatus::InvalidArg, "reference is not live");
        };
        reference.deleted = true;
        reference.value_bits = crate::value::TAG_UNDEFINED;
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_reference_ref(
    env: NapiEnv,
    reference: NapiRef,
    result: *mut u32,
) -> NapiStatus {
    with_env_mut(env, |env| {
        let Some(reference) = env.reference_mut(reference) else {
            return env.set_status(NapiStatus::InvalidArg, "reference is not live");
        };
        reference.refcount = reference.refcount.saturating_add(1);
        if !result.is_null() {
            *result = reference.refcount;
        }
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_reference_unref(
    env: NapiEnv,
    reference: NapiRef,
    result: *mut u32,
) -> NapiStatus {
    with_env_mut(env, |env| {
        let Some(reference) = env.reference_mut(reference) else {
            return env.set_status(NapiStatus::InvalidArg, "reference is not live");
        };
        if reference.refcount <= 1 {
            return env.set_status(
                NapiStatus::GenericFailure,
                "weak Node-API references are not enabled in this host core",
            );
        }
        reference.refcount -= 1;
        if !result.is_null() {
            *result = reference.refcount;
        }
        env.set_status(NapiStatus::Ok, "napi_ok")
    })
    .unwrap_or(NapiStatus::InvalidArg)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_reference_value(
    env: NapiEnv,
    reference: NapiRef,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let bits = with_env(env, |env| {
        env.reference(reference).map(|record| record.value_bits)
    });
    let Some(Some(bits)) = bits else {
        return set_status(env, NapiStatus::InvalidArg, "reference is not live");
    };
    let Ok(handle) = add_handle(env, bits) else {
        return NapiStatus::InvalidArg;
    };
    *result = handle;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_throw(env: NapiEnv, error: NapiValue) -> NapiStatus {
    let Ok(bits) = value_bits(env, error) else {
        return set_status(env, NapiStatus::InvalidArg, "error is not a live handle");
    };
    if with_env_mut(env, |env| env.pending_exception_bits = Some(bits)).is_none() {
        return NapiStatus::InvalidArg;
    }
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_exception_pending(env: NapiEnv, result: *mut bool) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    *result = pending_exception(env).is_some();
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_and_clear_last_exception(
    env: NapiEnv,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let pending = with_env_mut(env, |env| env.pending_exception_bits.take());
    let Some(pending) = pending else {
        return NapiStatus::InvalidArg;
    };
    *result = match pending {
        Some(bits) => add_handle(env, bits).expect("validated environment disappeared"),
        None => std::ptr::null_mut(),
    };
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_fatal_error(
    location: *const c_char,
    location_len: usize,
    message: *const c_char,
    message_len: usize,
) -> ! {
    fn bytes(ptr: *const c_char, len: usize) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let len = if len == NAPI_AUTO_LENGTH {
            unsafe { std::ffi::CStr::from_ptr(ptr).to_bytes().len() }
        } else {
            len
        };
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) })
            .into_owned()
    }
    eprintln!(
        "Perry Node-API fatal error at {}: {}",
        bytes(location, location_len),
        bytes(message, message_len)
    );
    std::process::abort()
}
