//! Fetch implementation for Android using Java HttpURLConnection via JNI.
//! Runs HTTP on a background thread (via Kotlin Executor) to avoid
//! NetworkOnMainThreadException.

use crate::jni_bridge;
use jni::objects::JValue;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

extern "C" {
    // Perry codegen expects Promise* (i64) return types
    fn js_promise_resolved(value: f64) -> i64;
    fn js_promise_rejected(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

struct FetchResponse {
    status: u16,
    status_text: String,
    body: Vec<u8>,
}

static RESPONSES: Mutex<Option<HashMap<usize, FetchResponse>>> = Mutex::new(None);
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

fn store_response(resp: FetchResponse) -> usize {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut guard = RESPONSES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    map.insert(id, resp);
    id
}

fn with_response<T>(handle: i64, f: impl FnOnce(&FetchResponse) -> T) -> Option<T> {
    let guard = RESPONSES.lock().unwrap();
    guard
        .as_ref()
        .and_then(|m| m.get(&(handle as usize)))
        .map(f)
}

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

/// Perform synchronous HTTP request via Java HttpURLConnection.
fn do_fetch(
    url: &str,
    method: &str,
    body: &str,
    headers_json: &str,
) -> Result<FetchResponse, String> {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let jurl = env
        .new_string(url)
        .map_err(|e| format!("url string: {e}"))?;
    let jmethod = env
        .new_string(method)
        .map_err(|e| format!("method string: {e}"))?;
    let jbody = env
        .new_string(body)
        .map_err(|e| format!("body string: {e}"))?;
    let jheaders = env
        .new_string(headers_json)
        .map_err(|e| format!("headers string: {e}"))?;

    let bridge_class =
        jni_bridge::with_cache(|c| env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap());
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();

    let result = env.call_static_method(
        bridge_cls,
        "performFetchSync",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[
            JValue::Object(&jurl),
            JValue::Object(&jmethod),
            JValue::Object(&jbody),
            JValue::Object(&jheaders),
        ],
    );

    let result_obj = match result {
        Ok(v) => v.l().map_err(|e| format!("result object: {e}"))?,
        Err(e) => {
            // Clear any pending JNI exception
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_clear();
            }
            unsafe {
                env.pop_local_frame(&jni::objects::JObject::null());
            }
            return Err(format!("JNI call failed: {e}"));
        }
    };

    // Parse result: "STATUS_CODE\nSTATUS_TEXT\nBODY"
    let result_str: String = env
        .get_string((&result_obj).into())
        .map(|s| s.into())
        .unwrap_or_default();

    unsafe {
        env.pop_local_frame(&jni::objects::JObject::null());
    }

    let mut lines = result_str.splitn(3, '\n');
    let status: u16 = lines.next().unwrap_or("0").parse().unwrap_or(0);
    let status_text = lines.next().unwrap_or("").to_string();
    let body = lines.next().unwrap_or("").as_bytes().to_vec();

    Ok(FetchResponse {
        status,
        status_text,
        body,
    })
}

// ── FFI entry points ─────────────────────────────────────────
// All signatures match Perry codegen ABI: i64 params, i64 returns for Promise

#[no_mangle]
pub unsafe extern "C" fn js_fetch_with_options(
    url_ptr: i64,
    method_ptr: i64,
    body_ptr: i64,
    headers_ptr: i64,
) -> i64 {
    let url = str_from_header(url_ptr as *const u8);
    let method = if method_ptr == 0 {
        "GET"
    } else {
        str_from_header(method_ptr as *const u8)
    };
    let body = if body_ptr == 0 {
        ""
    } else {
        str_from_header(body_ptr as *const u8)
    };
    let headers = if headers_ptr == 0 {
        "{}"
    } else {
        str_from_header(headers_ptr as *const u8)
    };

    let method_c = format!("{}\0", method);
    let url_c = format!("{}\0", url);
    __android_log_print(
        3,
        b"PerryFetch\0".as_ptr(),
        b"fetch: %s %s\0".as_ptr(),
        method_c.as_ptr(),
        url_c.as_ptr(),
    );

    match do_fetch(url, method, body, headers) {
        Ok(resp) => {
            let id = store_response(resp);
            __android_log_print(
                3,
                b"PerryFetch\0".as_ptr(),
                b"fetch: success, response_id=%d\0".as_ptr(),
                id as i32,
            );
            js_promise_resolved(id as f64)
        }
        Err(_e) => {
            __android_log_print(6, b"PerryFetch\0".as_ptr(), b"fetch: error\0".as_ptr());
            let err_msg = b"Fetch failed";
            let s = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as i64);
            let val = js_nanbox_string(s as i64);
            js_promise_rejected(val)
        }
    }
}

#[no_mangle]
pub extern "C" fn js_fetch_response_status(handle: i64) -> f64 {
    with_response(handle, |r| r.status as f64).unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_fetch_response_status_text(handle: i64) -> i64 {
    with_response(handle, |r| {
        let bytes = r.status_text.as_bytes();
        unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) as i64 }
    })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_fetch_response_ok(handle: i64) -> f64 {
    with_response(handle, |r| {
        if r.status >= 200 && r.status < 300 {
            1.0
        } else {
            0.0
        }
    })
    .unwrap_or(0.0)
}

#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_text(handle: i64) -> i64 {
    __android_log_print(
        3,
        b"PerryFetch\0".as_ptr(),
        b"response_text: handle=%d\0".as_ptr(),
        handle as i32,
    );
    let body = with_response(handle, |r| r.body.clone());
    match body {
        Some(b) => {
            let text = String::from_utf8_lossy(&b);
            let preview = if text.len() > 200 {
                &text[..200]
            } else {
                &text
            };
            let preview_c = format!("{}\0", preview);
            __android_log_print(
                3,
                b"PerryFetch\0".as_ptr(),
                b"response_text: len=%d body=%s\0".as_ptr(),
                text.len() as i32,
                preview_c.as_ptr(),
            );
            let s = js_string_from_bytes(text.as_ptr(), text.len() as i64);
            let val = js_nanbox_string(s as i64);
            js_promise_resolved(val)
        }
        None => {
            __android_log_print(
                6,
                b"PerryFetch\0".as_ptr(),
                b"response_text: handle not found\0".as_ptr(),
            );
            let err = b"Invalid response handle";
            let s = js_string_from_bytes(err.as_ptr(), err.len() as i64);
            let val = js_nanbox_string(s as i64);
            js_promise_rejected(val)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_fetch_response_json(handle: i64) -> i64 {
    js_fetch_response_text(handle)
}

// Convenience aliases — match codegen signatures
#[no_mangle]
pub unsafe extern "C" fn js_fetch_get(url_ptr: i64) -> i64 {
    js_fetch_with_options(url_ptr, 0, 0, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_fetch_post(url_ptr: i64, body_ptr: i64, _content_type_ptr: i64) -> i64 {
    js_fetch_with_options(url_ptr, 0, body_ptr, 0)
}

#[no_mangle]
pub unsafe extern "C" fn js_fetch_text(url_ptr: i64) -> i64 {
    js_fetch_with_options(url_ptr, 0, 0, 0)
}
