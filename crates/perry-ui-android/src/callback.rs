use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_promise_run_microtasks() -> i32;
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Drain the promise microtask queue — must be called after each callback
/// so that async/await continuations (.then chains) execute.
fn pump_microtasks() {
    unsafe {
        let ran = js_promise_run_microtasks();
        if ran > 0 {
            __android_log_print(
                3,
                b"PerryCallback\0".as_ptr(),
                b"pump_microtasks: ran %d tasks\0".as_ptr(),
                ran,
            );
            // Keep pumping until no more tasks
            loop {
                let more = js_promise_run_microtasks();
                if more == 0 {
                    break;
                }
                __android_log_print(
                    3,
                    b"PerryCallback\0".as_ptr(),
                    b"pump_microtasks: ran %d more tasks\0".as_ptr(),
                    more,
                );
            }
        }
    }
}

/// Global callback store — callbacks are registered on the native thread but
/// invoked on the UI thread, so thread_local won't work.
static CALLBACKS: Mutex<Option<HashMap<i64, f64>>> = Mutex::new(None);
static NEXT_KEY: AtomicI64 = AtomicI64::new(1);

/// Register a NaN-boxed closure and return a unique key for it.
pub fn register(closure_f64: f64) -> i64 {
    let key = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    let mut guard = CALLBACKS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(key, closure_f64);
    unsafe {
        __android_log_print(
            3,
            b"PerryCallback\0".as_ptr(),
            b"register: key=%lld bits=0x%llx\0".as_ptr(),
            key,
            closure_f64.to_bits() as i64,
        );
    }
    key
}

/// Invoke a registered callback with 0 arguments.
/// IMPORTANT: Extract closure_f64 and DROP the Mutex guard BEFORE calling
/// js_closure_call0. The closure may re-enter callback::register() which needs
/// to lock CALLBACKS.
pub fn invoke0(key: i64) {
    let closure_f64 = {
        let guard = CALLBACKS.lock().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            __android_log_print(
                3,
                b"PerryCallback\0".as_ptr(),
                b"invoke0: key=%lld ptr=%p\0".as_ptr(),
                key,
                closure_ptr,
            );
            js_closure_call0(closure_ptr);
            __android_log_print(
                3,
                b"PerryCallback\0".as_ptr(),
                b"invoke0: closure returned for key=%lld\0".as_ptr(),
                key,
            );
        }
    } else {
        unsafe {
            __android_log_print(
                3,
                b"PerryCallback\0".as_ptr(),
                b"invoke0: key=%lld NOT FOUND\0".as_ptr(),
                key,
            );
        }
    }
}

/// Invoke a registered callback with 1 argument.
pub fn invoke1(key: i64, arg: f64) {
    let closure_f64 = {
        let guard = CALLBACKS.lock().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, arg);
        }
    }
}

/// Invoke a registered callback with 2 arguments.
pub fn invoke2(key: i64, arg1: f64, arg2: f64) {
    let closure_f64 = {
        let guard = CALLBACKS.lock().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            js_closure_call2(closure_ptr, arg1, arg2);
        }
    }
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback0(long key).
/// This runs on the UI thread. Pumps microtasks after to drive async/await.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback0(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
) {
    invoke0(key as i64);
    pump_microtasks();
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback1(long key, double arg).
/// This runs on the UI thread. Pumps microtasks after to drive async/await.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback1(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    arg: jni::sys::jdouble,
) {
    invoke1(key as i64, arg);
    pump_microtasks();
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback2(long key, double arg1, double arg2).
/// This runs on the UI thread. Pumps microtasks after to drive async/await.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback2(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    arg1: jni::sys::jdouble,
    arg2: jni::sys::jdouble,
) {
    invoke2(key as i64, arg1, arg2);
    pump_microtasks();
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallbackWithString(long key, String text).
/// Converts the Java String to a NaN-boxed Perry string and invokes the callback.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallbackWithString(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    text: jni::objects::JString,
) {
    let rust_str: String = env.get_string(&text).map(|s| s.into()).unwrap_or_default();
    let bytes = rust_str.as_bytes();
    let nanboxed = unsafe {
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(str_ptr as i64)
    };
    invoke1(key as i64, nanboxed);
    pump_microtasks();
}
