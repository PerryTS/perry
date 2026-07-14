use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_closure_call4(closure: *const u8, arg0: f64, arg1: f64, arg2: f64, arg3: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_array_alloc(capacity: u32) -> *mut std::ffi::c_void;
    fn js_array_push_f64(arr: *mut std::ffi::c_void, value: f64) -> *mut std::ffi::c_void;
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

/// Read the closure currently stored under `key`, or `None` if it's
/// been removed. Used by the pointer dispatcher to fetch the current
/// callback without going through `invoke*`.
pub fn get(key: i64) -> Option<f64> {
    let guard = CALLBACKS.lock().unwrap();
    guard.as_ref().and_then(|m| m.get(&key).copied())
}

/// Overwrite the closure stored under `key`. Used by the pointer
/// dispatcher (issue #1868) to swap the active callback per phase
/// without inventing a new key — the Kotlin OnTouchListener already
/// holds a stable `(downKey, moveKey, upKey)` triple per widget.
pub fn replace(key: i64, closure_f64: f64) {
    let mut guard = CALLBACKS.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(key, closure_f64);
}

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
        // Must unbox the NaN-boxed closure — same as iOS/macOS.
        // Using to_bits() as a raw pointer leaves the 0x7ffd tag in the
        // high bits and crashes (often later on RenderThread via heap corruption).
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
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
        // Unbox NaN-boxed closure — see invoke0 for details.
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
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
        // Unbox NaN-boxed closure — see invoke0 for details.
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call2(closure_ptr, arg1, arg2);
        }
    }
}

/// Invoke a registered callback with 4 arguments. Used by the issue #552
/// geolocation success callback `(lat, lng, accuracy, timestamp_ms)`.
pub fn invoke4(key: i64, arg0: f64, arg1: f64, arg2: f64, arg3: f64) {
    let closure_f64 = {
        let guard = CALLBACKS.lock().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(closure_f64) = closure_f64 {
        // Unbox NaN-boxed closure — see invoke0 for details.
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            js_closure_call4(closure_ptr, arg0, arg1, arg2, arg3);
        }
    }
}

/// Invoke a registered callback with a single string-array argument. Each
/// path is allocated as a NaN-boxed Perry string and pushed into a fresh
/// `js_array_alloc` array; the array pointer is then NaN-boxed and passed
/// as the sole argument. Used by the issue #552 image picker callback.
pub fn invoke_with_string_array(key: i64, paths: &[String]) {
    let closure_f64 = {
        let guard = CALLBACKS.lock().unwrap();
        guard.as_ref().and_then(|m| m.get(&key).copied())
    };
    if let Some(closure_f64) = closure_f64 {
        // Unbox NaN-boxed closure — see invoke0 for details.
        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
        unsafe {
            let mut arr = js_array_alloc(paths.len() as u32);
            for p in paths {
                let bytes = p.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let nb_str = js_nanbox_string(str_ptr as i64);
                arr = js_array_push_f64(arr, nb_str);
            }
            let nb_arr = js_nanbox_pointer(arr as i64);
            js_closure_call1(closure_ptr, nb_arr);
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

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback4(long key, double, double, double, double).
/// Issue #552 geolocation success callback: (lat, lng, accuracy, timestamp_ms).
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback4(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    arg0: jni::sys::jdouble,
    arg1: jni::sys::jdouble,
    arg2: jni::sys::jdouble,
    arg3: jni::sys::jdouble,
) {
    invoke4(key as i64, arg0, arg1, arg2, arg3);
    pump_microtasks();
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallbackWithStringArray(long key, String[] paths).
/// Issue #552 image picker callback: passes a NaN-boxed Perry array of NaN-boxed Perry strings.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallbackWithStringArray(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    paths: jni::objects::JObjectArray,
) {
    let mut rust_paths: Vec<String> = Vec::new();
    if let Ok(len) = env.get_array_length(&paths) {
        for i in 0..len {
            if let Ok(item) = env.get_object_array_element(&paths, i) {
                let jstr: jni::objects::JString = item.into();
                let s: Option<String> = env.get_string(&jstr).map(|j| j.into()).ok();
                if let Some(s) = s {
                    rust_paths.push(s);
                }
            }
        }
    }
    invoke_with_string_array(key as i64, &rust_paths);
    pump_microtasks();
}

/// JNI entry point for the issue #583 deep-link callback. Java signature:
/// `nativeInvokeDeepLinkCallback(long key, String url, String source)`.
/// Builds the NaN-boxed `(url, source)` argument pair and dispatches to
/// the registered Perry closure. `source` is `"cold-start"` or
/// `"foreground"`.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeDeepLinkCallback(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    url: jni::objects::JString,
    source: jni::objects::JString,
) {
    let url_str: String = env.get_string(&url).map(|s| s.into()).unwrap_or_default();
    let source_str: String = env
        .get_string(&source)
        .map(|s| s.into())
        .unwrap_or_default();
    let url_jsval = unsafe {
        let p = js_string_from_bytes(url_str.as_ptr(), url_str.len() as i64);
        js_nanbox_string(p as i64)
    };
    let source_jsval = unsafe {
        let p = js_string_from_bytes(source_str.as_ptr(), source_str.len() as i64);
        js_nanbox_string(p as i64)
    };
    invoke2(key as i64, url_jsval, source_jsval);
    pump_microtasks();
}

/// JNI entry point for the issue #582 network reachability callback. Java
/// signature: `nativeInvokeNetworkCallback(long key, boolean connected, String kind)`.
/// Builds the NaN-boxed `(connected, kind)` argument pair and dispatches to
/// the registered Perry closure. `kind` is one of `"wifi" | "cellular" |
/// "ethernet" | "none" | "unknown"`.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeNetworkCallback(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    connected: jni::sys::jboolean,
    kind: jni::objects::JString,
) {
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    let connected_jsval = f64::from_bits(if connected != 0 { TAG_TRUE } else { TAG_FALSE });
    let kind_str: String = env.get_string(&kind).map(|s| s.into()).unwrap_or_default();
    let bytes = kind_str.as_bytes();
    let kind_jsval = unsafe {
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(str_ptr as i64)
    };
    invoke2(key as i64, connected_jsval, kind_jsval);
    pump_microtasks();
}
