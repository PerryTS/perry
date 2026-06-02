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

unsafe fn header_init_string(value: f64) -> String {
    let ptr = perry_runtime::value::js_jsvalue_to_string(value);
    string_from_header(ptr as *const StringHeader).unwrap_or_default()
}

unsafe fn headers_init_type_error(message: &str) -> ! {
    throw_fetch_type_error(message)
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_method_value(
    handle: f64,
    method_name_ptr: *const u8,
    method_name_len: usize,
) -> f64 {
    let id = handle_id(handle);
    if !HEADERS_REGISTRY.lock().unwrap().contains_key(&id) || method_name_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let name =
        match std::str::from_utf8(std::slice::from_raw_parts(method_name_ptr, method_name_len)) {
            Ok(name) => name,
            Err(_) => return f64::from_bits(TAG_UNDEFINED),
        };
    let method_name: &'static str = match name {
        "append" => "append",
        "delete" => "delete",
        "entries" | "Symbol.iterator" | "@@iterator" => "entries",
        "forEach" => "forEach",
        "get" => "get",
        "getSetCookie" => "getSetCookie",
        "has" => "has",
        "keys" => "keys",
        "set" => "set",
        "values" => "values",
        _ => return f64::from_bits(TAG_UNDEFINED),
    };
    headers_bound_method_value(id, method_name)
}

fn gc_type_for_raw_ptr(raw: i64) -> Option<u8> {
    if raw <= 0 {
        return None;
    }
    let addr = raw as usize;
    if addr < 0x100000 {
        return None;
    }
    unsafe { Some(*(raw as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)) }
}

fn has_sync_iterator(value: f64) -> bool {
    let iter_wk = perry_runtime::symbol::well_known_symbol("iterator");
    if iter_wk.is_null() {
        return false;
    }
    let sym = f64::from_bits(JSValue::pointer(iter_wk as *const u8).bits());
    let iter_fn = unsafe { perry_runtime::symbol::js_object_get_symbol_property(value, sym) };
    if iter_fn.to_bits() == TAG_UNDEFINED {
        return false;
    }
    let raw = perry_runtime::js_nanbox_get_pointer(iter_fn);
    raw != 0 && perry_runtime::closure::is_closure_ptr(raw as usize)
}

fn is_headers_init_iterable(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        return true;
    }
    if perry_runtime::js_array_is_array(value).to_bits() == TAG_TRUE {
        return true;
    }
    let raw = perry_runtime::js_nanbox_get_pointer(value);
    if raw == 0 {
        return false;
    }
    let addr = raw as usize;
    perry_runtime::map::is_registered_map(addr)
        || perry_runtime::set::is_registered_set(addr)
        || has_sync_iterator(value)
}

fn read_headers_record_entries(
    value: f64,
    scope: &perry_runtime::gc::RuntimeHandleScope,
) -> Option<Vec<(String, String)>> {
    if has_sync_iterator(value) {
        return None;
    }
    let raw = perry_runtime::js_nanbox_get_pointer(value);
    if gc_type_for_raw_ptr(raw) != Some(perry_runtime::gc::GC_TYPE_OBJECT) {
        return None;
    }

    unsafe {
        let obj_handle = scope.root_raw_const_ptr(raw as *const perry_runtime::ObjectHeader);
        let keys = perry_runtime::js_object_keys(
            obj_handle.get_raw_const_ptr::<perry_runtime::ObjectHeader>(),
        );
        let keys_handle = scope.root_raw_const_ptr(keys);
        let len = perry_runtime::js_array_length(
            keys_handle.get_raw_const_ptr::<perry_runtime::ArrayHeader>(),
        );
        let mut entries = Vec::with_capacity(len as usize);
        for i in 0..len {
            let keys_now = keys_handle.get_raw_const_ptr::<perry_runtime::ArrayHeader>();
            let key_value = perry_runtime::array::js_array_get_f64(keys_now, i);
            let key_ptr = perry_runtime::builtins::js_string_coerce(key_value);
            if key_ptr.is_null() {
                continue;
            }
            let key = string_from_header(key_ptr as *const StringHeader).unwrap_or_default();
            let val_value = perry_runtime::js_object_get_field_by_name_f64(
                obj_handle.get_raw_const_ptr::<perry_runtime::ObjectHeader>(),
                key_ptr,
            );
            let val = header_init_string(val_value);
            entries.push((key, val));
        }
        Some(entries)
    }
}

unsafe fn materialize_headers_init_iterable(
    value: f64,
    scope: &perry_runtime::gc::RuntimeHandleScope,
) -> *const perry_runtime::ArrayHeader {
    if !is_headers_init_iterable(value) {
        headers_init_type_error("Headers constructor: init is not iterable");
    }
    let arr_value = perry_runtime::array::js_for_of_to_array(value);
    let arr_handle = scope.root_nanbox_f64(arr_value);
    let raw = perry_runtime::js_nanbox_get_pointer(arr_handle.get_nanbox_f64());
    if raw == 0 {
        headers_init_type_error("Headers constructor: init is not iterable");
    }
    raw as *const perry_runtime::ArrayHeader
}

unsafe fn materialize_header_pair(
    pair_value: f64,
    scope: &perry_runtime::gc::RuntimeHandleScope,
) -> *const perry_runtime::ArrayHeader {
    if JSValue::from_bits(pair_value.to_bits()).is_any_string() {
        headers_init_type_error("Headers constructor: expected name/value pair");
    }
    if !is_headers_init_iterable(pair_value) {
        headers_init_type_error("Headers constructor: expected name/value pair");
    }
    let pair_array_value = if perry_runtime::js_array_is_array(pair_value).to_bits() == TAG_TRUE {
        pair_value
    } else {
        perry_runtime::array::js_for_of_to_array(pair_value)
    };
    let pair_handle = scope.root_nanbox_f64(pair_array_value);
    let raw = perry_runtime::js_nanbox_get_pointer(pair_handle.get_nanbox_f64());
    if raw == 0 {
        headers_init_type_error("Headers constructor: expected name/value pair");
    }
    raw as *const perry_runtime::ArrayHeader
}

unsafe fn read_headers_iterable_entries(
    arr: *const perry_runtime::ArrayHeader,
    scope: &perry_runtime::gc::RuntimeHandleScope,
) -> Vec<(String, String)> {
    if arr.is_null() {
        return Vec::new();
    }
    let arr_handle = scope.root_raw_const_ptr(arr);
    let len = perry_runtime::js_array_length(
        arr_handle.get_raw_const_ptr::<perry_runtime::ArrayHeader>(),
    );
    let mut entries = Vec::with_capacity(len as usize);
    for i in 0..len {
        let arr_now = arr_handle.get_raw_const_ptr::<perry_runtime::ArrayHeader>();
        let pair_value = perry_runtime::array::js_array_get_f64(arr_now, i);
        let pair = materialize_header_pair(pair_value, scope);
        let pair_handle = scope.root_raw_const_ptr(pair);
        let pair_now = pair_handle.get_raw_const_ptr::<perry_runtime::ArrayHeader>();
        if perry_runtime::js_array_length(pair_now) != 2 {
            headers_init_type_error("Headers constructor: expected name/value pair to be length 2");
        }
        let key_value = perry_runtime::array::js_array_get_f64(pair_now, 0);
        let val_value = perry_runtime::array::js_array_get_f64(pair_now, 1);
        entries.push((header_init_string(key_value), header_init_string(val_value)));
    }
    entries
}

fn append_header_entries(target_id: usize, entries: Vec<(String, String)>) {
    if let Some(store) = HEADERS_REGISTRY.lock().unwrap().get_mut(&target_id) {
        for (key, value) in entries {
            store.append(&key, &value);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_headers_init_from_value(handle: f64, init: f64) -> f64 {
    let init_value = JSValue::from_bits(init.to_bits());
    if init_value.is_undefined() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if init_value.is_null() {
        headers_init_type_error("Headers constructor: init must not be null");
    }

    let target_id = handle_id(handle);
    if !HEADERS_REGISTRY.lock().unwrap().contains_key(&target_id) {
        return f64::from_bits(TAG_UNDEFINED);
    }

    let source_id = handle_id(init);
    let cloned = HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get(&source_id)
        .map(|store| store.entries.clone());
    if let Some(entries) = cloned {
        append_header_entries(target_id, entries);
        return f64::from_bits(TAG_UNDEFINED);
    }

    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let init_handle = scope.root_nanbox_f64(init);
    let init_now = init_handle.get_nanbox_f64();

    if let Some(entries) = read_headers_record_entries(init_now, &scope) {
        append_header_entries(target_id, entries);
        return f64::from_bits(TAG_UNDEFINED);
    }

    let arr = materialize_headers_init_iterable(init_now, &scope);
    let entries = read_headers_iterable_entries(arr, &scope);
    append_header_entries(target_id, entries);
    f64::from_bits(TAG_UNDEFINED)
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

/// `headers.getSetCookie()` — returns all preserved Set-Cookie values.
#[no_mangle]
pub extern "C" fn js_headers_get_set_cookie(handle: f64) -> f64 {
    let id = handle_id(handle);
    let values = HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .map(HeadersStore::set_cookie_values)
        .unwrap_or_default();
    let mut arr = perry_runtime::js_array_alloc(values.len() as u32);
    for v in values {
        let v_ptr = js_string_from_bytes(v.as_ptr(), v.len() as u32);
        let v_nan = JSValue::string_ptr(v_ptr).bits();
        arr = perry_runtime::js_array_push_f64(arr, f64::from_bits(v_nan));
    }
    nanbox_array_pointer(arr)
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
