//! Combined `IncomingMessage.headers` view construction (#5079) — split out
//! of `lib.rs` to keep that file under the 2000-line file-size cap.
//!
//! Applies Node's `_http_incoming.js` `matchKnownFields` rules to the raw
//! `(name, value)` header pairs: `set-cookie` always becomes a string array,
//! a small set of single-value headers keep the first value, `cookie`
//! duplicates join with `"; "`, and everything else joins with `", "`.

use std::collections::HashMap;

use perry_ffi::{
    alloc_string, js_array_alloc, js_array_push, JsValue, ObjectHeader, TransientRootScope,
};

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

/// Build `res.rawHeaders` (#10467) — the flattened `[name, value, name,
/// value, ...]` array in wire arrival order, duplicates preserved (unlike
/// the combined `headers` view above, which merges/collapses per
/// `matchKnownFields`).
///
/// Caveat: header name casing here is whatever the transport captured. The
/// pooled reqwest path normalizes names to lower case before Perry ever
/// sees them (`http::HeaderName` only stores lower case), so this does not
/// reproduce Node's original wire casing on that path — only the raw-socket
/// paths (`plain_client`/`agent.createConnection`) could preserve it, and
/// today they lower-case on parse too. Tracked as a known gap, not silently
/// papered over.
pub(crate) fn build_raw_headers_array(raw: &[(String, String)]) -> f64 {
    let arr = unsafe { perry_ffi::js_array_alloc((raw.len() * 2) as u32) };
    if arr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #10668-followup: `arr` is a raw heap pointer. `alloc_string` below can
    // allocate (and therefore collect), which can move the array this
    // pointer refers to before the next `js_array_push` reads it back. Root
    // it through a `TransientRootScope` and re-derive the pointer via
    // `.get()` after every allocating call instead of reusing the pre-call
    // copy (see `docs/src/internals/gc-rooting-invariant.md`). Each pointer
    // is materialized on its own line and consumed immediately by the
    // `js_array_push` call on the very next line -- keep it that shape
    // (not folded into a multi-line call) so no raw pointer is ever bound
    // across the loop's next `alloc_string`.
    let scope = TransientRootScope::enter();
    let mut arr = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(arr).bits()));
    for (name, value) in raw {
        let name_s = alloc_string(name);
        let arr_ptr = JsValue::from_bits(arr.get().to_bits()).as_pointer();
        let name_value = JsValue::from_string_ptr(name_s.as_raw());
        let pushed = unsafe { js_array_push(arr_ptr, name_value) };
        arr = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(pushed).bits()));

        let value_s = alloc_string(value);
        let arr_ptr = JsValue::from_bits(arr.get().to_bits()).as_pointer();
        let value_value = JsValue::from_string_ptr(value_s.as_raw());
        let pushed = unsafe { js_array_push(arr_ptr, value_value) };
        arr = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(pushed).bits()));
    }
    arr.get()
}

/// Build the `set-cookie` array for [`build_response_headers_object`] --
/// always a string array, even for a single cookie (Node's
/// `matchKnownFields` never collapses `set-cookie`). Same GC-rooting shape
/// as [`build_raw_headers_array`]: `alloc_string` can collect and move
/// `arr` before `js_array_push` reads it back, so root and re-derive the
/// pointer at each use instead of reusing the pre-call copy. Kept as its
/// own top-level function (rather than nested inside the caller's
/// `if key == "set-cookie"` arm) so these lines stay short enough that
/// rustfmt doesn't wrap a `let` binding across lines and defeat the
/// ratchet scanner's line-oriented binding detection (#10668-followup).
fn build_set_cookie_array(set_cookie: &[String]) -> f64 {
    let arr = unsafe { js_array_alloc(set_cookie.len() as u32) };
    let scope = TransientRootScope::enter();
    let mut arr = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(arr).bits()));
    for cookie in set_cookie {
        let cookie_s = alloc_string(cookie);
        let arr_ptr = JsValue::from_bits(arr.get().to_bits()).as_pointer();
        let cookie_value = JsValue::from_string_ptr(cookie_s.as_raw());
        let pushed = unsafe { js_array_push(arr_ptr, cookie_value) };
        arr = scope.root_nanbox(f64::from_bits(JsValue::from_object_ptr(pushed).bits()));
    }
    arr.get()
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
                JsValue::from_bits(build_set_cookie_array(&set_cookie).to_bits())
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
