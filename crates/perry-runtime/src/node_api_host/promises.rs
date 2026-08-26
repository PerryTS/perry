use super::*;
use crate::value::JSValue;

#[no_mangle]
pub unsafe extern "C" fn napi_create_promise(
    env: NapiEnv,
    deferred: *mut NapiDeferred,
    promise: *mut NapiValue,
) -> NapiStatus {
    if deferred.is_null() || promise.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "deferred and promise must not be null",
        );
    }
    let promise_pointer = crate::promise::js_promise_new();
    let promise_bits = JSValue::pointer(promise_pointer.cast()).bits();
    let promise_handle = match add_handle(env, promise_bits) {
        Ok(handle) => handle,
        Err(status) => return status,
    };
    let deferred_pointer = with_env_mut(env, |env| {
        let mut record = Box::new(DeferredRecord {
            env_serial: env.serial,
            promise_bits,
            settled: false,
        });
        let pointer = (&mut *record) as *mut DeferredRecord as NapiDeferred;
        env.deferred_lookup
            .insert(pointer as usize, env.deferreds.len());
        env.deferreds.push(record);
        pointer
    });
    let Some(deferred_pointer) = deferred_pointer else {
        return NapiStatus::InvalidArg;
    };
    *deferred = deferred_pointer;
    *promise = promise_handle;
    ok(env)
}

unsafe fn settle_deferred(
    env: NapiEnv,
    deferred: NapiDeferred,
    value: NapiValue,
    reject: bool,
) -> NapiStatus {
    let value_bits = match value_bits(env, value) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "settlement value is not a live handle"),
    };
    let promise_bits = with_env(env, |env| {
        env.deferred(deferred).map(|record| record.promise_bits)
    })
    .flatten();
    let Some(promise_bits) = promise_bits else {
        return set_status(env, NapiStatus::InvalidArg, "deferred is not live");
    };
    let marked = with_env_mut(env, |env| {
        let Some(record) = env.deferred_mut(deferred) else {
            return false;
        };
        record.settled = true;
        record.promise_bits = crate::value::TAG_UNDEFINED;
        true
    });
    if marked != Some(true) {
        return set_status(env, NapiStatus::InvalidArg, "deferred is not live");
    }
    let promise = JSValue::from_bits(promise_bits).as_pointer::<crate::promise::Promise>()
        as *mut crate::promise::Promise;
    match catch_value_call(env, || {
        if reject {
            crate::promise::js_promise_reject(promise, f64::from_bits(value_bits));
        } else {
            crate::promise::js_promise_resolve(promise, f64::from_bits(value_bits));
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }) {
        Ok(_) => ok(env),
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_resolve_deferred(
    env: NapiEnv,
    deferred: NapiDeferred,
    resolution: NapiValue,
) -> NapiStatus {
    settle_deferred(env, deferred, resolution, false)
}

#[no_mangle]
pub unsafe extern "C" fn napi_reject_deferred(
    env: NapiEnv,
    deferred: NapiDeferred,
    rejection: NapiValue,
) -> NapiStatus {
    settle_deferred(env, deferred, rejection, true)
}

#[no_mangle]
pub unsafe extern "C" fn napi_is_promise(
    env: NapiEnv,
    value: NapiValue,
    result: *mut bool,
) -> NapiStatus {
    if result.is_null() {
        return set_status(env, NapiStatus::InvalidArg, "result must not be null");
    }
    let bits = match value_bits(env, value) {
        Ok(bits) => bits,
        Err(status) => return set_status(env, status, "value is not a live handle"),
    };
    *result = crate::promise::js_value_is_promise(f64::from_bits(bits)) != 0;
    ok(env)
}

#[no_mangle]
pub unsafe extern "C" fn napi_run_script(
    env: NapiEnv,
    _script: NapiValue,
    result: *mut NapiValue,
) -> NapiStatus {
    if !result.is_null() {
        *result = std::ptr::null_mut();
    }
    set_status(
        env,
        NapiStatus::GenericFailure,
        "napi_run_script is unsupported by Perry's ahead-of-time runtime",
    )
}

#[no_mangle]
pub unsafe extern "C" fn napi_adjust_external_memory(
    env: NapiEnv,
    change_in_bytes: i64,
    adjusted_value: *mut i64,
) -> NapiStatus {
    if adjusted_value.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "adjusted value must not be null",
        );
    }
    let next = with_env(env, |env| env.external_memory.checked_add(change_in_bytes)).flatten();
    let Some(next) = next else {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "external memory adjustment overflow",
        );
    };
    if change_in_bytes > 0 {
        crate::gc::gc_note_external_side_alloc(change_in_bytes as usize);
    } else if change_in_bytes < 0 {
        crate::gc::gc_note_external_side_free(change_in_bytes.unsigned_abs() as usize);
    }
    if with_env_mut(env, |env| env.external_memory = next).is_none() {
        return NapiStatus::InvalidArg;
    }
    *adjusted_value = next;
    ok(env)
}
