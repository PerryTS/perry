//! Combined `IncomingMessage.headers` view construction (#5079) — split out
//! of `lib.rs` to keep that file under the 2000-line file-size cap.
//!
//! Applies Node's `_http_incoming.js` `matchKnownFields` rules to the raw
//! `(name, value)` header pairs: `set-cookie` always becomes a string array,
//! a small set of single-value headers keep the first value, `cookie`
//! duplicates join with `"; "`, and everything else joins with `", "`.

use std::collections::HashMap;

use perry_ffi::{alloc_string, JsValue, ObjectHeader};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

/// Single-value response headers: per Node's `_http_incoming.js`
/// `matchKnownFields`, a duplicate of any of these is discarded (the
/// first value wins) rather than joined with `, `. `set-cookie` is not
/// in this list — it always accumulates into an array.
fn is_single_value_header(name: &str) -> bool {
    matches!(
        name,
        "age"
            | "authorization"
            | "content-length"
            | "content-type"
            | "etag"
            | "expires"
            | "from"
            | "host"
            | "if-modified-since"
            | "if-unmodified-since"
            | "last-modified"
            | "location"
            | "max-forwards"
            | "proxy-authorization"
            | "referer"
            | "retry-after"
            | "server"
            | "user-agent"
    )
}

/// Build the combined `IncomingMessage.headers` object from the raw
/// `(name, value)` pairs, applying Node's `matchKnownFields` rules
/// (#5079):
///
/// * `set-cookie` → **always** a string array, even for one cookie;
/// * single-value fields ([`is_single_value_header`]) → first value wins;
/// * `cookie` → duplicates joined with `; `;
/// * everything else → duplicates joined with `, `.
///
/// Header names are lower-cased, matching Node's `headers` view.
pub(crate) fn build_response_headers_object(raw: &[(String, String)]) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);

    // Insertion-ordered accumulation. `set_cookie` is collected
    // separately so a single cookie still surfaces as an array.
    let mut order: Vec<String> = Vec::new();
    let mut combined: HashMap<String, String> = HashMap::new();
    let mut set_cookie: Vec<String> = Vec::new();
    let mut saw_set_cookie = false;

    for (name, value) in raw {
        let key = name.to_ascii_lowercase();
        if key == "set-cookie" {
            if !saw_set_cookie {
                saw_set_cookie = true;
                order.push(key);
            }
            set_cookie.push(value.clone());
            continue;
        }
        match combined.get_mut(&key) {
            Some(existing) => {
                if !is_single_value_header(&key) {
                    // Node's `matchKnownFields`: duplicate `cookie` headers join
                    // with "; ", everything else with ", ".
                    existing.push_str(if key == "cookie" { "; " } else { ", " });
                    existing.push_str(value);
                }
            }
            None => {
                order.push(key.clone());
                combined.insert(key, value.clone());
            }
        }
    }

    let count = order.len() as u32;
    let key_refs: Vec<&str> = order.iter().map(|s| s.as_str()).collect();
    let (packed, shape_id) = perry_ffi::build_object_shape(&key_refs);
    let obj: *mut ObjectHeader = unsafe {
        perry_ffi::js_object_alloc_with_shape(shape_id, count, packed.as_ptr(), packed.len() as u32)
    };
    if !obj.is_null() {
        for (i, key) in order.iter().enumerate() {
            let v = if key == "set-cookie" {
                let mut arr = perry_runtime::js_array_alloc(set_cookie.len() as u32);
                for cookie in &set_cookie {
                    let ptr =
                        perry_runtime::js_string_from_bytes(cookie.as_ptr(), cookie.len() as u32);
                    arr =
                        perry_runtime::js_array_push(arr, perry_runtime::JSValue::string_ptr(ptr));
                }
                JsValue::from_bits(perry_runtime::JSValue::array_ptr(arr).bits())
            } else if let Some(val) = combined.get(key) {
                let s = alloc_string(val);
                JsValue::from_string_ptr(s.as_raw())
            } else {
                continue;
            };
            unsafe {
                perry_ffi::js_object_set_field(obj, i as u32, v);
            }
        }
        let v = JsValue::from_object_ptr(obj as *mut u8);
        out = f64::from_bits(v.bits());
    }
    out
}
