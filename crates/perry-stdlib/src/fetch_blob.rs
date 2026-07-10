//! Issue #1211: `node:buffer` Blob/File constructors + object-URL
//! registry.  Split out of `fetch.rs` to keep that file under the
//! 2,000-line lint gate.
//!
//! Hand-offs into `fetch.rs`:
//!   - `BLOB_REGISTRY`/`alloc_blob`/`BlobData` (storage + record shape)
//!   - `handle_id` / `handle_to_f64` (handle <-> NaN-boxed f64)
//!   - `string_from_header` (`*StringHeader` → `Option<String>`)
//!   - `TAG_UNDEFINED` constant
//! All exposed as `pub(crate)` in fetch.rs so this module can build on
//! the same registry without re-implementing the ABI.

use std::collections::HashMap;
use std::sync::Mutex;

use perry_runtime::string::{js_string_from_bytes, StringHeader};

use crate::fetch::{
    alloc_blob, blob_bytes_clone, handle_id, handle_to_f64, string_from_header, BlobData,
    BLOB_REGISTRY, TAG_UNDEFINED,
};

// Object URLs: `URL.createObjectURL(blob)` returns a
// `blob:nodedata:<uuid-shaped-id>` URL and `URL.revokeObjectURL(url)` removes it.
// `resolveObjectURL(url)` returns the same blob handle (or undefined
// after revoke).  The registry is process-global; entries live until
// `revokeObjectURL` clears them.
lazy_static::lazy_static! {
    static ref OBJECT_URL_REGISTRY: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    static ref NEXT_OBJECT_URL_ID: Mutex<u64> = Mutex::new(1);
}

fn throw_invalid_object_url_blob(value: f64) -> ! {
    let message = format!(
        "The \"obj\" argument must be an instance of Blob. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn object_url_uuid(n: u64) -> String {
    let group1 = (n >> 32) as u32;
    let group2 = ((n >> 16) & 0xFFFF) as u16;
    let group3 = 0x4000u16 | (n as u16 & 0x0FFF);
    let group4 = 0x8000u16 | ((n >> 12) as u16 & 0x3FFF);
    let group5 = n & 0x0000_FFFF_FFFF_FFFF;
    format!("{group1:08x}-{group2:04x}-{group3:04x}-{group4:04x}-{group5:012x}")
}

/// Walk the `parts` argument of `new Blob([...])` / `new File([...], name)`.
///
/// Per the WHATWG Blob spec, `parts` is iterated at the TOP level only.
/// Each element is then coerced by [`append_one_blob_part`]: BufferSource
/// (Buffer/Uint8Array/ArrayBuffer) and Blob/File handles contribute their
/// raw bytes, and EVERY other value — including nested arrays, numbers,
/// booleans, null, undefined, and objects — is run through `ToString`
/// (USVString) and UTF-8 encoded. So `new Blob([["a","b"]])` produces
/// `"a,b"` (array `ToString`), not the recursive-flatten `"ab"`.
unsafe fn append_blob_parts(parts: f64, out: &mut Vec<u8>) {
    let bits = parts.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFD {
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // A real heap pointer (array) lives above the synthetic handle band;
        // handle-band ids (blobs/headers/… start at 0x40000) must NOT be read
        // as an ArrayHeader or we dereference a fake pointer.
        if perry_runtime::value::addr_class::is_above_handle_band(addr)
            && !perry_runtime::buffer::is_registered_buffer(addr)
        {
            let arr_ptr = addr as *const perry_runtime::array::ArrayHeader;
            if !arr_ptr.is_null() {
                let gc_header = (arr_ptr as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
                    as *const perry_runtime::gc::GcHeader;
                let obj_type = (*gc_header).obj_type;
                if obj_type == perry_runtime::gc::GC_TYPE_ARRAY
                    || obj_type == perry_runtime::gc::GC_TYPE_LAZY_ARRAY
                {
                    let len = perry_runtime::array::js_array_length(arr_ptr);
                    for i in 0..len {
                        let elem = perry_runtime::array::js_array_get(arr_ptr, i);
                        append_one_blob_part(f64::from_bits(elem.bits()), out);
                    }
                    return;
                }
            }
        }
    }
    // Non-array `parts` (or empty/undefined): Node throws TypeError, but
    // we tolerate by treating the single value as one coerced part.
    if parts.to_bits() != TAG_UNDEFINED {
        append_one_blob_part(parts, out);
    }
}

/// Coerce ONE Blob part. Recognized binary shapes contribute raw bytes;
/// anything else is stringified via `ToString` and UTF-8 encoded. Does
/// NOT recurse into nested arrays (those stringify like any other value).
unsafe fn append_one_blob_part(part: f64, out: &mut Vec<u8>) {
    let bits = part.to_bits();
    let top16 = bits >> 48;
    // POINTER_TAG ─ either a Blob handle or a Buffer/Uint8Array.
    if top16 == 0x7FFD {
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // Handle-band id (a Blob handle or another synthetic fetch handle) —
        // NOT a real pointer. #6231: this previously tested `addr < 0x10000`,
        // but blob handle ids start at 0x40000 (FETCH_HANDLE_BAND_START), so a
        // `Blob` used as a BlobPart never matched here and fell through to
        // `js_jsvalue_to_string`, which dereferenced the id as a pointer and
        // SIGSEGV'd. Recognize the whole handle band and copy the blob's bytes.
        if perry_runtime::value::addr_class::is_handle_band(addr) {
            if let Some(body) = blob_bytes_clone(addr) {
                out.extend_from_slice(&body);
            }
            // A non-blob handle as a BlobPart: skip it rather than deref a
            // synthetic id. (Spec would ToString it; skipping is crash-safe.)
            return;
        }
        // BufferHeader (Buffer / Uint8Array / ArrayBuffer)?
        if perry_runtime::buffer::is_registered_buffer(addr) {
            let buf = addr as *const perry_runtime::buffer::BufferHeader;
            let len = (*buf).length as usize;
            let data = perry_runtime::buffer::buffer_data(buf);
            out.extend_from_slice(std::slice::from_raw_parts(data, len));
            return;
        }
    }
    // Everything else (strings, numbers, booleans, null, undefined, plain
    // objects, nested arrays): ToString → UTF-8. `js_jsvalue_to_string`
    // handles array comma-join, object `toString`, number/bool/null
    // formatting identically to JS `String(x)`.
    let str_ptr = perry_runtime::value::js_jsvalue_to_string(part) as *const StringHeader;
    if let Some(s) = string_from_header(str_ptr) {
        out.extend_from_slice(s.as_bytes());
    }
}

/// Normalize a Blob/File `type` per the WHATWG Blob spec: if every
/// character is in the printable ASCII range U+0020–U+007E it is
/// lowercased; otherwise the type is treated as the empty string.
fn normalize_blob_type(raw: &str) -> String {
    if raw.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
        raw.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// `ToNumber(value)` for the File `lastModified` option. Numbers and
/// int32s pass through; strings are parsed per JS `Number(string)`
/// (trim, empty -> 0, unparsable -> NaN); booleans/null map to 0/1/0;
/// undefined and other objects -> NaN.
unsafe fn blob_to_number(value: f64) -> f64 {
    let bits = value.to_bits();
    // Heap string?
    if (bits >> 48) == 0x7FFF {
        let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if let Some(s) = string_from_header(p) {
            let t = s.trim();
            if t.is_empty() {
                return 0.0;
            }
            return t.parse::<f64>().unwrap_or(f64::NAN);
        }
        return f64::NAN;
    }
    // SSO / non-string: materialize via ToString then parse only if it was
    // a string-like value; otherwise use the numeric coercion of the value.
    let jsval = perry_runtime::value::JSValue::from_bits(bits);
    if jsval.is_short_string() {
        let str_ptr = perry_runtime::value::js_jsvalue_to_string(value) as *const StringHeader;
        if let Some(s) = string_from_header(str_ptr) {
            let t = s.trim();
            if t.is_empty() {
                return 0.0;
            }
            return t.parse::<f64>().unwrap_or(f64::NAN);
        }
    }
    jsval.to_number()
}

/// Coerce a `type`/`name` option that arrives NaN-boxed as f64 into a
/// Rust `String` via `ToString` (USVString). Undefined becomes "".
unsafe fn blob_string_option(value: f64) -> String {
    if value.to_bits() == TAG_UNDEFINED {
        return String::new();
    }
    let str_ptr = perry_runtime::value::js_jsvalue_to_string(value) as *const StringHeader;
    string_from_header(str_ptr).unwrap_or_default()
}

/// `new Blob(parts, { type })` — allocate a Blob handle from the
/// flattened bytes of `parts`.  Returns a NaN-boxed POINTER_TAG
/// handle identical to the one `response.blob()` produces, so all
/// subsequent `blob.size` / `blob.type` / `blob.text()` /
/// `blob.arrayBuffer()` / `blob.slice()` dispatch flows through the
/// existing `module == "blob"` arm in codegen.
#[no_mangle]
pub unsafe extern "C" fn js_blob_new(parts: f64, content_type: f64) -> f64 {
    let mut body: Vec<u8> = Vec::new();
    append_blob_parts(parts, &mut body);
    let type_str = normalize_blob_type(&blob_string_option(content_type));
    handle_to_f64(alloc_blob(BlobData::blob(body, type_str)))
}

/// `new File(parts, name, { type, lastModified })` — same registry as
/// Blob, with `name` / `last_modified_ms` populated so
/// `js_file_name` / `js_file_last_modified` can read them back. The
/// returned handle is `instanceof Blob` (same registry); dispatch
/// routes File-specific property reads via `module == "blob",
/// class_name == "File"` in codegen.
#[no_mangle]
pub unsafe extern "C" fn js_file_new(
    parts: f64,
    name: f64,
    content_type: f64,
    last_modified: f64,
) -> f64 {
    let mut body: Vec<u8> = Vec::new();
    append_blob_parts(parts, &mut body);
    // `name` is coerced via `ToString` per the WHATWG File spec — a
    // numeric name like `new File(parts, 123)` becomes `"123"`.
    let name_str = blob_string_option(name);
    let type_str = normalize_blob_type(&blob_string_option(content_type));
    // The codegen passes a bare `f64::NAN` sentinel when the
    // `lastModified` option is ABSENT (use `Date.now()`). A present
    // option arrives NaN-boxed (string/number/bool/etc.) and is coerced
    // via `ToNumber` — distinct bit pattern from the sentinel.
    let lm = if last_modified.to_bits() == f64::NAN.to_bits() {
        // Cheap stamp: wall clock in ms.  Same source the codegen
        // uses for `Date.now()` so two consecutive `new File()` calls
        // produce a monotonic-ish sequence.
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)
    } else {
        blob_to_number(last_modified)
    };
    let data = BlobData {
        body,
        content_type: type_str,
        file_name: Some(name_str),
        last_modified_ms: Some(lm),
    };
    handle_to_f64(alloc_blob(data))
}

/// `file.name` — empty string for plain Blob handles.
#[no_mangle]
pub unsafe extern "C" fn js_file_name(handle: f64) -> *mut StringHeader {
    let id = handle_id(handle);
    let name = BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|b| b.file_name.clone())
        .unwrap_or_default();
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

/// `file.lastModified` — Date-now-style timestamp; 0 for plain Blobs.
#[no_mangle]
pub extern "C" fn js_file_last_modified(handle: f64) -> f64 {
    let id = handle_id(handle);
    BLOB_REGISTRY
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|b| b.last_modified_ms)
        .unwrap_or(0.0)
}

/// `URL.createObjectURL(blob)` — register the Blob handle under a
/// fresh `blob:nodedata:<uuid-shaped-id>` URL and return the URL string.
#[no_mangle]
pub unsafe extern "C" fn js_url_create_object_url(blob_handle: f64) -> *mut StringHeader {
    let id = handle_id(blob_handle);
    if id == 0 || !BLOB_REGISTRY.lock().unwrap().contains_key(&id) {
        throw_invalid_object_url_blob(blob_handle);
    }
    let url = {
        let mut counter = NEXT_OBJECT_URL_ID.lock().unwrap();
        let n = *counter;
        *counter += 1;
        // Node's published shape is UUID-based. Keep deterministic
        // monotonic identity while matching the visible UUID layout.
        format!("blob:nodedata:{}", object_url_uuid(n))
    };
    OBJECT_URL_REGISTRY.lock().unwrap().insert(url.clone(), id);
    js_string_from_bytes(url.as_ptr(), url.len() as u32)
}

/// `URL.revokeObjectURL(url)` — drop the registry entry, if any.
#[no_mangle]
pub unsafe extern "C" fn js_url_revoke_object_url(url: f64) {
    let bits = url.to_bits();
    if (bits >> 48) != 0x7FFF {
        return;
    }
    let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
    let s = match string_from_header(p) {
        Some(s) => s,
        None => return,
    };
    OBJECT_URL_REGISTRY.lock().unwrap().remove(&s);
}

/// `import { resolveObjectURL } from "node:buffer"` — return the
/// registered Blob handle for `url`, or `undefined` after revoke.
#[no_mangle]
pub unsafe extern "C" fn js_buffer_resolve_object_url(url: f64) -> f64 {
    let bits = url.to_bits();
    if (bits >> 48) != 0x7FFF {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
    let s = match string_from_header(p) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    match OBJECT_URL_REGISTRY.lock().unwrap().get(&s).copied() {
        Some(id) => handle_to_f64(id),
        None => f64::from_bits(TAG_UNDEFINED),
    }
}
