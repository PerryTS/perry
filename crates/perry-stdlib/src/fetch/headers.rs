//! Web Fetch `Headers` FFI.
//!
//! Split out of `fetch/mod.rs` to keep that file under the 2,000-line lint
//! gate (mirrors the earlier `fetch_blob.rs` extraction). As a child module
//! of `fetch`, this sees `mod.rs`'s private items (`HeadersStore`,
//! `HEADERS_REGISTRY`, `alloc_headers`, `handle_id`, `handle_to_f64`,
//! `string_from_header`, the `TAG_*` consts, …) through the glob `use
//! super::*` — no extra visibility changes required.

use super::*;

/// new Headers() — returns NaN-boxed POINTER_TAG handle as f64.
/// See `handle_to_f64` / `handle_id` for the encoding contract.
#[no_mangle]
pub extern "C" fn js_headers_new() -> f64 {
    handle_to_f64(alloc_headers(HeadersStore::default()))
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_set(
    handle: f64,
    key_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> f64 {
    let id = handle_id(handle);
    let key = string_from_header(key_ptr).unwrap_or_default();
    let value = string_from_header(value_ptr).unwrap_or_default();
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&id) {
        store.set(&key, &value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// `headers.append(name, value)` — adds a value, combining with `", "` when
/// the name already exists (Web Fetch spec). Returns undefined. (#1649)
#[no_mangle]
pub unsafe extern "C" fn js_headers_append(
    handle: f64,
    key_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> f64 {
    let id = handle_id(handle);
    let key = string_from_header(key_ptr).unwrap_or_default();
    let value = string_from_header(value_ptr).unwrap_or_default();
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&id) {
        store.append(&key, &value);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_get(
    handle: f64,
    key_ptr: *const StringHeader,
) -> *mut StringHeader {
    let id = handle_id(handle);
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return std::ptr::null_mut(),
    };
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get(&id) {
        if let Some(v) = store.get(&key) {
            return js_string_from_bytes(v.as_ptr(), v.len() as u32);
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_has(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => return f64::from_bits(TAG_FALSE),
    };
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get(&id) {
        if store.has(&key) {
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_delete(handle: f64, key_ptr: *const StringHeader) -> f64 {
    let id = handle_id(handle);
    let key = string_from_header(key_ptr).unwrap_or_default();
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&id) {
        store.delete(&key);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Snapshot the headers store sorted by key (WHATWG spec: iteration order is
/// sorted lexicographically by name, regardless of insertion order). Used by
/// `forEach`, `keys`, `values`, `entries`, and `Symbol.iterator` so all five
/// surfaces agree byte-for-byte (refs #576).
fn snapshot_sorted(handle: f64) -> Vec<(String, String)> {
    let id = handle_id(handle);
    let mut entries: Vec<(String, String)> = match HEADERS_REGISTRY.lock().unwrap().get(&id) {
        Some(s) => s.entries.clone(),
        None => return Vec::new(),
    };
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[no_mangle]
pub extern "C" fn js_headers_for_each(handle: f64, callback: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    // Extract closure pointer from NaN-boxed callback
    let cb_bits = callback.to_bits();
    let cb_ptr = (cb_bits & 0x0000_FFFF_FFFF_FFFF) as i64;
    if cb_ptr == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let closure = cb_ptr as *const perry_runtime::ClosureHeader;
    for (k, v) in entries {
        let v_ptr = js_string_from_bytes(v.as_ptr(), v.len() as u32);
        let k_ptr = js_string_from_bytes(k.as_ptr(), k.len() as u32);
        let v_nan = JSValue::string_ptr(v_ptr).bits();
        let k_nan = JSValue::string_ptr(k_ptr).bits();
        perry_runtime::js_closure_call2(closure, f64::from_bits(v_nan), f64::from_bits(k_nan));
    }
    f64::from_bits(TAG_UNDEFINED)
}

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

#[inline]
fn nanbox_array_pointer(arr: *mut perry_runtime::ArrayHeader) -> f64 {
    let bits = POINTER_TAG | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF);
    f64::from_bits(bits)
}

/// `headers.keys()` — returns a sorted-by-key array of header names. The
/// returned array is itself iterable, so `for (const k of headers.keys())`,
/// spread, and `Array.from` all work via the array's existing `Symbol.iterator`
/// (refs #576).
#[no_mangle]
pub extern "C" fn js_headers_keys(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    let mut arr = perry_runtime::js_array_alloc(entries.len() as u32);
    for (k, _) in entries {
        let k_ptr = js_string_from_bytes(k.as_ptr(), k.len() as u32);
        let k_nan = JSValue::string_ptr(k_ptr).bits();
        arr = perry_runtime::js_array_push_f64(arr, f64::from_bits(k_nan));
    }
    nanbox_array_pointer(arr)
}

/// `headers.values()` — sorted-by-key array of header values. See `js_headers_keys`.
#[no_mangle]
pub extern "C" fn js_headers_values(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    let mut arr = perry_runtime::js_array_alloc(entries.len() as u32);
    for (_, v) in entries {
        let v_ptr = js_string_from_bytes(v.as_ptr(), v.len() as u32);
        let v_nan = JSValue::string_ptr(v_ptr).bits();
        arr = perry_runtime::js_array_push_f64(arr, f64::from_bits(v_nan));
    }
    nanbox_array_pointer(arr)
}

/// `headers.entries()` — sorted-by-key array of `[key, value]` pair arrays.
/// `for (const [k, v] of headers.entries())` and `for (const [k, v] of h)` both
/// route here (the latter via the `Symbol.iterator` alias, see #576).
#[no_mangle]
pub extern "C" fn js_headers_entries(handle: f64) -> f64 {
    let entries = snapshot_sorted(handle);
    let mut arr = perry_runtime::js_array_alloc(entries.len() as u32);
    for (k, v) in entries {
        let k_ptr = js_string_from_bytes(k.as_ptr(), k.len() as u32);
        let v_ptr = js_string_from_bytes(v.as_ptr(), v.len() as u32);
        let k_nan = JSValue::string_ptr(k_ptr).bits();
        let v_nan = JSValue::string_ptr(v_ptr).bits();
        let mut pair = perry_runtime::js_array_alloc(2);
        pair = perry_runtime::js_array_push_f64(pair, f64::from_bits(k_nan));
        pair = perry_runtime::js_array_push_f64(pair, f64::from_bits(v_nan));
        arr = perry_runtime::js_array_push_f64(arr, nanbox_array_pointer(pair));
    }
    nanbox_array_pointer(arr)
}
