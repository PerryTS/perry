//! `URLSearchParams` class — query-string entries collection + FFI surface.

use super::*;

use super::parse::rebuild_url_href;

// ============================================================================
// URLSearchParams implementation
// ============================================================================

/// Field indices for URLSearchParams object
pub(crate) const URL_SEARCH_PARAMS_ENTRIES: u32 = 0; // Array of [key, value] pairs
/// When set to a NaN-boxed URL pointer, mutations on this params object
/// propagate back to the URL's `search` field and re-derive `href`. Empty
/// (TAG_UNDEFINED) for free-standing URLSearchParams created via
/// `new URLSearchParams(...)`.
pub(crate) const URL_SEARCH_PARAMS_OWNER: u32 = 1;
pub(crate) const URL_SEARCH_PARAMS_FIELD_COUNT: u32 = 2;

fn throw_invalid_query_pair_tuple() -> ! {
    let msg = b"Each query pair must be an iterable [name, value] tuple";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code_pub(s, "ERR_INVALID_TUPLE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

fn throw_missing_args(name_and_value: bool) -> ! {
    let message: &[u8] = if name_and_value {
        b"The \"name\" and \"value\" arguments must be specified"
    } else {
        b"The \"name\" argument must be specified"
    };
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_MISSING_ARGS");
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub extern "C" fn js_url_search_params_throw_missing_args(kind: i32) -> f64 {
    throw_missing_args(kind == 2)
}

fn gc_object_type(raw_ptr: *const u8) -> Option<u8> {
    if raw_ptr.is_null() || (raw_ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe { Some(*raw_ptr.sub(crate::gc::GC_HEADER_SIZE)) }
}

fn known_iterable_to_array(raw_ptr: i64) -> Option<*mut ArrayHeader> {
    if raw_ptr == 0 {
        return None;
    }
    let addr = raw_ptr as usize;
    if crate::map::is_registered_map(addr) {
        return Some(crate::map::js_map_entries(
            raw_ptr as *const crate::map::MapHeader,
        ));
    }
    if crate::set::is_registered_set(addr) {
        return Some(crate::set::js_set_to_array(
            raw_ptr as *const crate::set::SetHeader,
        ));
    }
    let raw = raw_ptr as *const u8;
    match gc_object_type(raw) {
        Some(t) if t == crate::gc::GC_TYPE_ARRAY => Some(raw as *mut ArrayHeader),
        _ => None,
    }
}

fn query_pair_iterable_to_array(pair_f64: f64) -> *mut ArrayHeader {
    let pair_jsval = crate::value::JSValue::from_bits(pair_f64.to_bits());
    if !pair_jsval.is_pointer() {
        throw_invalid_query_pair_tuple();
    }
    let pair_ptr_i64 = crate::value::js_nanbox_get_pointer(pair_f64);
    if let Some(pair) = known_iterable_to_array(pair_ptr_i64) {
        return pair;
    }
    throw_invalid_query_pair_tuple();
}

// URL field constant alias — we only need URL_SEARCH from `super::parse` for
// the params→owner-URL sync path.
use super::parse::URL_SEARCH;

/// Serialize the current entries of `params` back into a URL query string
/// (with leading `?`), then write it to the owning URL's `search` field and
/// re-derive `href`. No-op when the params object has no owner URL.
pub(crate) unsafe fn maybe_sync_params_to_owner(params: *mut ObjectHeader) {
    let owner_f = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_OWNER);
    let Some(owner) = object_from_f64(owner_f) else {
        return;
    };
    let entries = get_url_search_params_entries(params);
    let parts: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect();
    let search = if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    };
    js_object_set_field_f64(owner, URL_SEARCH, create_string_f64(&search));
    rebuild_url_href(owner);
}

/// Parse a query string into key-value pairs
/// Handles formats like "?foo=bar&baz=qux" or "foo=bar&baz=qux"
pub(crate) fn parse_query_string(query: &str) -> Vec<(String, String)> {
    let query = query.strip_prefix('?').unwrap_or(query);
    if query.is_empty() {
        return Vec::new();
    }

    query
        .split('&')
        .filter_map(|pair| {
            if pair.is_empty() {
                return None;
            }
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");
            // URL decode the key and value
            Some((url_decode(key), url_decode(value)))
        })
        .collect()
}

/// Simple URL decoding (handles %XX sequences and + as space)
pub(crate) fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                decoded.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded.push(byte);
                    i += 3;
                } else {
                    decoded.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                decoded.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

/// URL encode a string using the WHATWG application/x-www-form-urlencoded
/// encode set used by URLSearchParams serialization. Unlike URL path
/// encoding, this escapes `~` and leaves `*` literal.
pub(crate) fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '*' => {
                result.push(c);
            }
            ' ' => result.push('+'),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}

fn coerce_search_param_arg(value: f64) -> String {
    string_from_header(js_url_coerce_string(value))
}

/// Create a URLSearchParams object from entries
pub(crate) fn create_url_search_params_object(entries: Vec<(String, String)>) -> *mut ObjectHeader {
    let obj = js_object_alloc(0, URL_SEARCH_PARAMS_FIELD_COUNT);

    // Create keys array
    let mut keys = js_array_alloc(URL_SEARCH_PARAMS_FIELD_COUNT);
    keys = js_array_push_f64(keys, create_string_f64("_entries"));
    keys = js_array_push_f64(keys, create_string_f64("_owner"));
    js_object_set_keys(obj, keys);
    // Owner starts as undefined; the URL constructor sets it when it adopts
    // this params object as its `.searchParams`.
    js_object_set_field_f64(
        obj,
        URL_SEARCH_PARAMS_OWNER,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );

    // Create entries array - each entry is a 2-element array [key, value]
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, value) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&value));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }

    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(obj, URL_SEARCH_PARAMS_ENTRIES, entries_f64);

    obj
}

/// Get entries from a URLSearchParams object
pub(crate) fn get_url_search_params_entries(params: *mut ObjectHeader) -> Vec<(String, String)> {
    if params.is_null() {
        return Vec::new();
    }

    let entries_f64 = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_ENTRIES);
    let entries_ptr: *mut ArrayHeader = f64::to_bits(entries_f64).cast_signed() as *mut ArrayHeader;

    if entries_ptr.is_null() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let len = unsafe { (*entries_ptr).length } as usize;

    for i in 0..len {
        let pair_f64 = crate::array::js_array_get_f64(entries_ptr, i as u32);
        let pair_ptr: *mut ArrayHeader = f64::to_bits(pair_f64).cast_signed() as *mut ArrayHeader;

        if !pair_ptr.is_null() {
            let key_f64 = crate::array::js_array_get_f64(pair_ptr, 0);
            let value_f64 = crate::array::js_array_get_f64(pair_ptr, 1);

            let key = get_string_content(key_f64);
            let value = get_string_content(value_f64);
            result.push((key, value));
        }
    }

    result
}

/// Create a new URLSearchParams from a string
/// js_url_search_params_new(init: *mut StringHeader) -> *mut ObjectHeader
#[no_mangle]
pub extern "C" fn js_url_search_params_new(
    init_str: *mut crate::StringHeader,
) -> *mut ObjectHeader {
    let init_string = if init_str.is_null() {
        String::new()
    } else {
        unsafe {
            let len = (*init_str).byte_len as usize;
            let data_ptr = (init_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let slice = std::slice::from_raw_parts(data_ptr, len);
            String::from_utf8_lossy(slice).into_owned()
        }
    };

    let entries = parse_query_string(&init_string);
    create_url_search_params_object(entries)
}

/// Create an empty URLSearchParams
/// js_url_search_params_new_empty() -> *mut ObjectHeader
#[no_mangle]
pub extern "C" fn js_url_search_params_new_empty() -> *mut ObjectHeader {
    create_url_search_params_object(Vec::new())
}

/// Create a URLSearchParams from any NaN-boxed init value.
///
/// Spec init shapes (`new URLSearchParams(init)`):
/// - `undefined` / `null`        → empty
/// - `string` (with or without `?`)→ parse as query string
/// - record `{ k: v, ... }`      → use property names + stringified values
/// - another `URLSearchParams`   → copy entries
/// - array of `[k, v]` pairs     → use as-is (spec-conformant; rarely used)
///
/// Pre-fix, codegen routed every init through `js_url_search_params_new`
/// (which only handles strings) — object inits got `js_get_string_pointer_unified`'d
/// into an interpret-pointer-as-string read of garbage bytes (typed-local
/// repro printed `"%00="`). Refs #575.
#[no_mangle]
pub extern "C" fn js_url_search_params_new_any(init: f64) -> *mut ObjectHeader {
    let bits = init.to_bits();
    let jsval = crate::value::JSValue::from_bits(bits);

    if jsval.is_undefined() || jsval.is_null() {
        return create_url_search_params_object(Vec::new());
    }

    // String — common path. Includes both STRING_TAG and SHORT_STRING (SSO).
    if jsval.is_string() || jsval.is_short_string() {
        let s = get_string_content(init);
        return create_url_search_params_object(parse_query_string(&s));
    }

    if jsval.is_pointer() {
        let ptr_i64 = crate::value::js_nanbox_get_pointer(init);
        if ptr_i64 == 0 {
            return create_url_search_params_object(Vec::new());
        }

        // Iterable form: arrays, Maps, and Sets are consumed as sequences of
        // query-pair iterables before falling back to record enumeration.
        if let Some(iterable_entries) = known_iterable_to_array(ptr_i64) {
            return create_url_search_params_object(read_iterable_pair_entries(iterable_entries));
        }

        let obj_ptr = ptr_i64 as *mut ObjectHeader;

        // Detect another URLSearchParams: its `_entries` field holds the
        // ArrayHeader of [k, v] pair arrays. We can't tell apart by class
        // (both are class_id 0), so peek at the keys array's first entry.
        // Simpler heuristic: try to read it as an entries-table; fall back
        // to record enumeration if shape doesn't match.
        let copied = try_read_as_search_params(obj_ptr);
        if let Some(entries) = copied {
            return create_url_search_params_object(entries);
        }

        // Treat as record `{ k: v }`. Iterate keys and read each field.
        let entries = read_record_entries(obj_ptr);
        return create_url_search_params_object(entries);
    }

    // Numbers / booleans / etc. — coerce with `String(init)`, then parse.
    let s = stringify_field_value(init);
    create_url_search_params_object(parse_query_string(&s))
}

/// Iterable URLSearchParams init — each element must itself be an iterable
/// two-item `[key, value]` tuple. Node throws `ERR_INVALID_TUPLE` when an
/// entry is not iterable or does not produce exactly two items.
pub(crate) fn read_iterable_pair_entries(arr: *const ArrayHeader) -> Vec<(String, String)> {
    if arr.is_null() {
        return Vec::new();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = scope.root_raw_const_ptr(arr);
    let len = crate::array::js_array_length(arr_handle.get_raw_const_ptr::<ArrayHeader>()) as usize;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        let arr = arr_handle.get_raw_const_ptr::<ArrayHeader>();
        let pair_f64 = crate::array::js_array_get_f64(arr, i as u32);
        let pair = query_pair_iterable_to_array(pair_f64);
        let pair_len = crate::array::js_array_length(pair);
        if pair_len != 2 {
            throw_invalid_query_pair_tuple();
        }
        let key_f64 = crate::array::js_array_get_f64(pair, 0);
        let value_f64 = crate::array::js_array_get_f64(pair, 1);
        let k = stringify_field_value(key_f64);
        let v = stringify_field_value(value_f64);
        out.push((k, v));
    }
    out
}

/// Walk an object as if it were a URLSearchParams, returning `Some(entries)`
/// only if the shape matches: a single field whose value is a top-level array
/// of 2-element [string, string] pair arrays. Returns None for any other shape
/// (the caller falls back to record enumeration).
pub(crate) fn try_read_as_search_params(
    params: *mut ObjectHeader,
) -> Option<Vec<(String, String)>> {
    if params.is_null() {
        return None;
    }
    unsafe {
        // arm64_32 watchOS hardening: validate `params` is a real heap
        // `GC_TYPE_OBJECT` *before* dereferencing `class_id` / `keys_array`
        // below. Callers guard only with the magnitude check
        // `!is_handle_band(ptr)`, which on 32-bit pointers cannot distinguish a
        // low heap address from a misclassified non-pointer (e.g. a closure
        // whose `CLOSURE_MAGIC` probe missed — see `CLOSURE_TYPE_TAG_OFFSET`).
        // Without this, the raw field reads below dereference garbage → SIGSEGV
        // (the documented watchOS startup crash, stage 2). `try_read_gc_header`
        // rejects the handle band and implausible addresses without touching
        // memory; a genuine URLSearchParams is an ordinary `GC_TYPE_OBJECT`
        // allocation, so this is a no-op for every value that legitimately
        // reaches here (mirrors the guard `is_url_object_shape` already applies
        // to the sibling `js_url_href_if_url` probe).
        match crate::value::addr_class::try_read_gc_header(params as usize) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
            _ => return None,
        }
        // A genuine URLSearchParams is always allocated with `class_id == 0`
        // (an ordinary object, see `create_url_search_params`). Other native
        // classes — notably `util.MIMEParams` — ALSO store their data in a
        // leading `_entries` slot but carry a distinct registered class id and
        // a different field layout (no `_owner` slot). Without this guard such
        // an object is mis-detected below, then read with the URLSearchParams
        // layout (an out-of-bounds `_owner` field read) → segfault when e.g.
        // `String(mimeParams)` / `mimeParams.toString()` routes through
        // `js_jsvalue_to_string`. Bail for any non-zero class id.
        if (*params).class_id != 0 {
            return None;
        }
        // URLSearchParams stores entries in field index 0 (URL_SEARCH_PARAMS_ENTRIES).
        // If this isn't a URLSearchParams, that slot likely holds a string or
        // is missing — we detect by checking the keys array shape.
        let keys_arr = (*params).keys_array;
        if keys_arr.is_null() {
            return None;
        }
        let keys_len = (*keys_arr).length;
        // URLSearchParams objects carry the `_entries` slot (and now `_owner`
        // for URL-adopted instances). The first slot is always `_entries`;
        // any extra field beyond that is fine as long as `_entries` leads.
        if keys_len == 0 {
            return None;
        }
        let key0 = crate::array::js_array_get_f64(keys_arr, 0);
        let key0_str = get_string_content(key0);
        if key0_str != "_entries" {
            return None;
        }
    }
    Some(get_url_search_params_entries(params))
}

/// Enumerate an object's own enumerable keys as `(name, String(value))` pairs.
/// Used for `new URLSearchParams({ a: "1", b: "2" })` — order matches the
/// keys array (insertion order, like Node).
pub(crate) fn read_record_entries(obj: *mut ObjectHeader) -> Vec<(String, String)> {
    if obj.is_null() {
        return Vec::new();
    }
    unsafe {
        let keys_arr = (*obj).keys_array;
        if keys_arr.is_null() {
            return Vec::new();
        }
        let len = (*keys_arr).length as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let key_f64 = crate::array::js_array_get_f64(keys_arr, i as u32);
            let key = get_string_content(key_f64);
            if key.is_empty() {
                continue;
            }
            let val_f64 = crate::object::js_object_get_field_f64(obj, i as u32);
            let val = stringify_field_value(val_f64);
            out.push((key, val));
        }
        out
    }
}

/// Coerce a NaN-boxed field value through the same `String(value)` path used
/// by URLSearchParams constructors and methods. Symbols throw.
pub(crate) fn stringify_field_value(v: f64) -> String {
    coerce_search_param_arg(v)
}

/// Get a value by name
/// js_url_search_params_get(params, name) -> *mut StringHeader (string or null)
#[no_mangle]
pub extern "C" fn js_url_search_params_get(
    params: *mut ObjectHeader,
    name_value: f64,
) -> *mut crate::StringHeader {
    let name = coerce_search_param_arg(name_value);

    let entries = get_url_search_params_entries(params);
    for (key, value) in entries {
        if key == name {
            let bytes = value.as_bytes();
            return js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        }
    }

    // Return null pointer
    std::ptr::null_mut()
}

/// Check if a name exists
/// js_url_search_params_has(params, name) -> f64 (boolean)
#[no_mangle]
pub extern "C" fn js_url_search_params_has(params: *mut ObjectHeader, name_value: f64) -> f64 {
    let name = coerce_search_param_arg(name_value);

    let entries = get_url_search_params_entries(params);
    let found = entries.iter().any(|(key, _)| key == &name);
    if found {
        1.0
    } else {
        0.0
    }
}

/// Set a value (replaces existing or adds new)
/// js_url_search_params_set(params, name, value) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_set(
    params: *mut ObjectHeader,
    name_value: f64,
    value_value: f64,
) {
    let name = coerce_search_param_arg(name_value);
    let value = coerce_search_param_arg(value_value);

    let entries = get_url_search_params_entries(params);

    // Node replaces the first existing entry in place and removes the
    // remaining duplicates; if absent, it appends at the end.
    let mut replaced = false;
    let mut next = Vec::with_capacity(entries.len().max(1));
    for (key, val) in entries {
        if key == name {
            if !replaced {
                next.push((name.clone(), value.clone()));
                replaced = true;
            }
        } else {
            next.push((key, val));
        }
    }
    if !replaced {
        next.push((name, value));
    }
    let entries = next;

    // Update the object with new entries
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, val) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&val));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }
    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    unsafe { maybe_sync_params_to_owner(params) };
}

/// Append a value (adds even if name already exists)
/// js_url_search_params_append(params, name, value) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_append(
    params: *mut ObjectHeader,
    name_value: f64,
    value_value: f64,
) {
    let name = coerce_search_param_arg(name_value);
    let value = coerce_search_param_arg(value_value);

    let mut entries = get_url_search_params_entries(params);
    entries.push((name, value));

    // Update the object with new entries
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, val) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&val));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }
    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    unsafe { maybe_sync_params_to_owner(params) };
}

/// Delete all entries with a name
/// js_url_search_params_delete(params, name) -> void
#[no_mangle]
pub extern "C" fn js_url_search_params_delete(params: *mut ObjectHeader, name_value: f64) {
    let name = coerce_search_param_arg(name_value);

    let mut entries = get_url_search_params_entries(params);
    entries.retain(|(key, _)| key != &name);

    // Update the object with new entries
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, val) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&val));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }
    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    unsafe { maybe_sync_params_to_owner(params) };
}

/// Node 19+: `URLSearchParams.has(name, value)` returns true only when both
/// the name and value match (exact string equality). The lowering only calls
/// this helper when the second argument was actually present.
#[no_mangle]
pub extern "C" fn js_url_search_params_has2(
    params: *mut ObjectHeader,
    name_value: f64,
    value_value: f64,
) -> f64 {
    let name = coerce_search_param_arg(name_value);
    let value = coerce_search_param_arg(value_value);
    let entries = get_url_search_params_entries(params);
    let found = entries.iter().any(|(k, v)| k == &name && v == &value);
    if found {
        1.0
    } else {
        0.0
    }
}

/// Node 19+: `URLSearchParams.delete(name, value)` — drops only entries
/// matching BOTH the name and value (exact string equality). The lowering only
/// calls this helper when the second argument was actually present.
#[no_mangle]
pub extern "C" fn js_url_search_params_delete2(
    params: *mut ObjectHeader,
    name_value: f64,
    value_value: f64,
) {
    let name = coerce_search_param_arg(name_value);
    let value_filter = coerce_search_param_arg(value_value);
    let mut entries = get_url_search_params_entries(params);
    entries.retain(|(k, v)| {
        if k != &name {
            return true;
        }
        v != &value_filter
    });
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, val) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&val));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }
    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    unsafe { maybe_sync_params_to_owner(params) };
}

/// Issue #650: `URLSearchParams.size` getter — returns the number of
/// entries (key/value pairs) currently stored. Reads the length of the
/// `_entries` ArrayHeader directly; null receiver / missing array
/// returns 0.
#[no_mangle]
pub extern "C" fn js_url_search_params_size(params: *mut ObjectHeader) -> i32 {
    if params.is_null() {
        return 0;
    }
    let entries_f64 = crate::object::js_object_get_field_f64(params, URL_SEARCH_PARAMS_ENTRIES);
    let entries_ptr: *const ArrayHeader =
        f64::to_bits(entries_f64).cast_signed() as *const ArrayHeader;
    if entries_ptr.is_null() {
        return 0;
    }
    unsafe { (*entries_ptr).length as i32 }
}

/// Convert to query string
/// js_url_search_params_to_string(params: *mut ObjectHeader) -> *mut StringHeader (raw string pointer)
#[no_mangle]
pub extern "C" fn js_url_search_params_to_string(
    params: *mut ObjectHeader,
) -> *mut crate::StringHeader {
    let entries = get_url_search_params_entries(params);

    if entries.is_empty() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }

    let result: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect();

    let joined = result.join("&");
    let bytes = joined.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// `params.entries()` — returns an array of `[key, value]` pair arrays. Used
/// to lower direct iteration `for (const [k, v] of params)` (refs #575). The
/// pair arrays expose strings, so the destructure `[k, v]` reads them with
/// the standard array-element path.
#[no_mangle]
pub extern "C" fn js_url_search_params_entries_arr(params: *mut ObjectHeader) -> f64 {
    let entries = get_url_search_params_entries(params);
    let mut arr = js_array_alloc(entries.len() as u32);
    for (k, v) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&k));
        pair = js_array_push_f64(pair, create_string_f64(&v));
        // Inline NaN-box the pair pointer with POINTER_TAG so for-of
        // destructure reads the array via `js_array_get_f64` correctly.
        let pair_bits = 0x7FFD_0000_0000_0000u64 | ((pair as u64) & 0x0000_FFFF_FFFF_FFFF);
        arr = js_array_push_f64(arr, f64::from_bits(pair_bits));
    }
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub extern "C" fn js_url_search_params_keys_arr(params: *mut ObjectHeader) -> f64 {
    let entries = get_url_search_params_entries(params);
    let mut arr = js_array_alloc(entries.len() as u32);
    for (k, _) in entries {
        arr = js_array_push_f64(arr, create_string_f64(&k));
    }
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub extern "C" fn js_url_search_params_values_arr(params: *mut ObjectHeader) -> f64 {
    let entries = get_url_search_params_entries(params);
    let mut arr = js_array_alloc(entries.len() as u32);
    for (_, v) in entries {
        arr = js_array_push_f64(arr, create_string_f64(&v));
    }
    f64::from_bits(0x7FFD_0000_0000_0000u64 | ((arr as u64) & 0x0000_FFFF_FFFF_FFFF))
}

#[no_mangle]
pub extern "C" fn js_url_search_params_sort(params: *mut ObjectHeader) {
    let mut entries = get_url_search_params_entries(params);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut entries_array = js_array_alloc(entries.len() as u32);
    for (key, val) in entries {
        let mut pair = js_array_alloc(2);
        pair = js_array_push_f64(pair, create_string_f64(&key));
        pair = js_array_push_f64(pair, create_string_f64(&val));
        let pair_f64 = f64::from_bits(i64::cast_unsigned(pair as i64));
        entries_array = js_array_push_f64(entries_array, pair_f64);
    }
    let entries_f64 = f64::from_bits(i64::cast_unsigned(entries_array as i64));
    js_object_set_field_f64(params, URL_SEARCH_PARAMS_ENTRIES, entries_f64);
    unsafe { maybe_sync_params_to_owner(params) };
}

#[no_mangle]
pub extern "C" fn js_url_search_params_for_each(
    params: *mut ObjectHeader,
    callback: f64,
    this_arg: f64,
) {
    // #3058: Node validates the callback before iterating — a missing or
    // non-function argument throws `TypeError [ERR_INVALID_ARG_TYPE]`.
    crate::fs::validate::validate_function("callback", callback);
    let entries = get_url_search_params_entries(params);
    let this_value = crate::value::js_nanbox_pointer(params as i64);
    for (key, value) in entries {
        let args = [
            create_string_f64(&value),
            create_string_f64(&key),
            this_value,
        ];
        unsafe {
            let prev_this = crate::object::js_implicit_this_set(this_arg);
            let _ = crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
            crate::object::js_implicit_this_set(prev_this);
        }
    }
}

/// Get all values for a name
/// js_url_search_params_get_all(params, name) -> f64 (array)
#[no_mangle]
pub extern "C" fn js_url_search_params_get_all(params: *mut ObjectHeader, name_value: f64) -> f64 {
    let name = coerce_search_param_arg(name_value);

    let entries = get_url_search_params_entries(params);
    let values: Vec<String> = entries
        .iter()
        .filter(|(key, _)| key == &name)
        .map(|(_, value)| value.clone())
        .collect();

    let mut result = js_array_alloc(values.len() as u32);
    for value in values {
        result = js_array_push_f64(result, create_string_f64(&value));
    }
    f64::from_bits(i64::cast_unsigned(result as i64))
}

// ---- #5961: dynamic method dispatch for type-erased receivers ----
//
// The native URLSearchParams is an ordinary object (class_id == 0, leading
// `_entries` slot — see `create_url_search_params_object` /
// `try_read_as_search_params`); its method surface normally exists only via
// static type-directed lowering (perry-codegen url_main.rs). When the
// receiver's static type is lost (`any`, heterogeneous containers, minified
// bundles), the generic dynamic member read used to find nothing and
// `sp.append(...)` threw "append is not a function". These bound-method
// thunks mirror the webcrypto_method_value pattern
// (object/global_this/ctor_thunks.rs) with the receiver carried in a
// GC-traced nanboxed capture slot.

/// True when `obj` has the native URLSearchParams shape: an ordinary object
/// (class_id == 0) whose leading own key is `_entries`. Mirrors the guard in
/// [`try_read_as_search_params`] without materializing the entries.
pub(crate) fn shape_is_url_search_params(obj: *const ObjectHeader) -> bool {
    if obj.is_null() {
        return false;
    }
    unsafe {
        // The receiver may be *any* pointer-tagged value -- a Date/Temporal
        // cell, buffer, closure, small handle, etc. -- because callers reach
        // this probe on a type-erased receiver before validating the object
        // kind (#5964's dynamic-toString arm in js_native_call_method faults on
        // a Date cell otherwise: date.toString() twice segfaulted while reading
        // a Date cell's bytes at ObjectHeader offsets). Only ordinary objects
        // carry the ObjectHeader layout, so classify the address and gate on the
        // GC header type first -- a Date cell is GC_TYPE_DATE_CELL, an array
        // GC_TYPE_ARRAY, and so on. `try_read_gc_header` also rejects small
        // handles / slab buffers that would otherwise deref a fake header.
        match crate::value::addr_class::try_read_gc_header(obj as usize) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
            _ => return false,
        }
        if (*obj).class_id != 0 {
            return false;
        }
        let keys_arr = (*obj).keys_array;
        if keys_arr.is_null() {
            return false;
        }
        // #5989: `keys_array` itself must be validated before deref — a
        // GC_TYPE_OBJECT receiver reached mid-transition (or with a typed
        // layout) can carry a non-heap word here; reading `(*keys_arr).length`
        // on it SIGSEGV'd during Next.js request handling (config.js method
        // dispatch probing an arbitrary receiver through this shape check).
        // Same try_read_gc_header gate as the receiver above. Require the
        // EAGER `GC_TYPE_ARRAY` layout specifically: an object's own key list
        // is always eager, and `(*keys_arr).length` / `js_array_get_f64` below
        // read the eager `ArrayHeader` fields — a `GC_TYPE_LAZY_ARRAY` doesn't
        // share that layout, so reject it (a real URLSearchParams shape never
        // has a lazy keys_array; returning false is correct).
        match crate::value::addr_class::try_read_gc_header(keys_arr as usize) {
            Some(h) if h.obj_type == crate::gc::GC_TYPE_ARRAY => {}
            _ => return false,
        }
        if (*keys_arr).length == 0 {
            return false;
        }
        let key0 = crate::array::js_array_get_f64(keys_arr, 0);
        get_string_content(key0) == "_entries"
    }
}

fn usp_thunk_receiver(closure: *const crate::closure::ClosureHeader) -> *mut ObjectHeader {
    let recv = crate::closure::js_closure_get_capture_f64(closure, 0);
    (recv.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
}

extern "C" fn usp_append_thunk(
    closure: *const crate::closure::ClosureHeader,
    name: f64,
    value: f64,
) -> f64 {
    js_url_search_params_append(usp_thunk_receiver(closure), name, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn usp_set_thunk(
    closure: *const crate::closure::ClosureHeader,
    name: f64,
    value: f64,
) -> f64 {
    js_url_search_params_set(usp_thunk_receiver(closure), name, value);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn usp_get_thunk(closure: *const crate::closure::ClosureHeader, name: f64) -> f64 {
    // js_url_search_params_get returns a raw `*mut StringHeader` for the
    // static-lowering path; rebuild the boxed value here instead.
    let wanted = coerce_search_param_arg(name);
    let entries = get_url_search_params_entries(usp_thunk_receiver(closure));
    match entries.into_iter().find(|(k, _)| *k == wanted) {
        Some((_, v)) => create_string_f64(&v),
        None => f64::from_bits(crate::value::TAG_NULL),
    }
}

extern "C" fn usp_has_thunk(closure: *const crate::closure::ClosureHeader, name: f64) -> f64 {
    if js_url_search_params_has(usp_thunk_receiver(closure), name) != 0.0 {
        f64::from_bits(crate::value::TAG_TRUE)
    } else {
        f64::from_bits(crate::value::TAG_FALSE)
    }
}

extern "C" fn usp_delete_thunk(closure: *const crate::closure::ClosureHeader, name: f64) -> f64 {
    js_url_search_params_delete(usp_thunk_receiver(closure), name);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Fused dynamic method call (`sp.append(...)` through `js_native_call_method`
/// on a type-erased receiver): dispatch the covered URLSearchParams surface to
/// the natives. Returns `None` for uncovered names so the generic dispatch
/// keeps its existing behavior.
pub(crate) fn url_search_params_dynamic_call(
    params: *mut ObjectHeader,
    name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let arg = |i: usize| -> f64 {
        if !args_ptr.is_null() && i < args_len {
            unsafe { *args_ptr.add(i) }
        } else {
            undefined
        }
    };
    Some(match name {
        "append" => {
            js_url_search_params_append(params, arg(0), arg(1));
            undefined
        }
        "set" => {
            js_url_search_params_set(params, arg(0), arg(1));
            undefined
        }
        "get" => {
            let wanted = coerce_search_param_arg(arg(0));
            match get_url_search_params_entries(params)
                .into_iter()
                .find(|(k, _)| *k == wanted)
            {
                Some((_, v)) => create_string_f64(&v),
                None => f64::from_bits(crate::value::TAG_NULL),
            }
        }
        "has" => {
            // Node 19+: `has(name, value)` matches on BOTH when a second
            // argument is present.
            let hit = if args_len >= 2 && arg(1).to_bits() != crate::value::TAG_UNDEFINED {
                js_url_search_params_has2(params, arg(0), arg(1)) != 0.0
            } else {
                js_url_search_params_has(params, arg(0)) != 0.0
            };
            if hit {
                f64::from_bits(crate::value::TAG_TRUE)
            } else {
                f64::from_bits(crate::value::TAG_FALSE)
            }
        }
        "delete" => {
            if args_len >= 2 && arg(1).to_bits() != crate::value::TAG_UNDEFINED {
                js_url_search_params_delete2(params, arg(0), arg(1));
            } else {
                js_url_search_params_delete(params, arg(0));
            }
            undefined
        }
        "toString" => {
            let entries = get_url_search_params_entries(params);
            let joined = entries
                .iter()
                .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
                .collect::<Vec<_>>()
                .join("&");
            create_string_f64(&joined)
        }
        // Iteration helpers surface as eager arrays, matching the convention
        // the static lowering uses (`js_get_iterator` wraps an eager array in
        // the runtime array iterator when driven through for-of). mysql2's
        // `parseUrl` iterates `url.searchParams` and Node code commonly calls
        // `.entries()` / `.getAll()` on dynamic receivers.
        "entries" => js_url_search_params_entries_arr(params),
        "keys" => js_url_search_params_keys_arr(params),
        "values" => js_url_search_params_values_arr(params),
        "getAll" => {
            // The FFI helper returns RAW pointer bits (the static lowering
            // boxes them); re-box for the dynamic call path.
            let raw = js_url_search_params_get_all(params, arg(0));
            crate::value::js_nanbox_pointer(f64::to_bits(raw).cast_signed())
        }
        "sort" => {
            js_url_search_params_sort(params);
            undefined
        }
        "forEach" => {
            js_url_search_params_for_each(params, arg(0), arg(1));
            undefined
        }
        _ => return None,
    })
}

/// Bound-method value for a dynamic property read on a native URLSearchParams
/// object. Returns `None` for names outside the covered surface so unknown
/// properties still read as `undefined`.
pub(crate) fn url_search_params_method_value(obj: *const ObjectHeader, name: &str) -> Option<f64> {
    let (func_ptr, arity): (*const u8, u32) = match name {
        "append" => (usp_append_thunk as *const u8, 2),
        "set" => (usp_set_thunk as *const u8, 2),
        "get" => (usp_get_thunk as *const u8, 1),
        "has" => (usp_has_thunk as *const u8, 1),
        "delete" => (usp_delete_thunk as *const u8, 1),
        _ => return None,
    };
    crate::closure::js_register_closure_arity(func_ptr, arity);
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    if closure.is_null() {
        return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
    }
    // Nanboxed (not raw-ptr) capture so the GC traces the receiver.
    crate::closure::js_closure_set_capture_f64(
        closure,
        0,
        crate::value::js_nanbox_pointer(obj as i64),
    );
    Some(crate::value::js_nanbox_pointer(closure as i64))
}
