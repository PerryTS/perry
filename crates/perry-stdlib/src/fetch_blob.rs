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
// `blob:nodedata:<id>` URL and `URL.revokeObjectURL(url)` removes it.
// `resolveObjectURL(url)` returns the same blob handle (or undefined
// after revoke).  The registry is process-global; entries live until
// `revokeObjectURL` clears them.
lazy_static::lazy_static! {
    static ref OBJECT_URL_REGISTRY: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
    static ref NEXT_OBJECT_URL_ID: Mutex<u64> = Mutex::new(1);
}

/// Walk a JS value passed as the `parts` argument of
/// `new Blob([...])` / `new File([...], name)` and append its bytes
/// to `out`. Supported part shapes:
///   - String (NaN-boxed STRING_TAG): UTF-8 encode the characters.
///   - Buffer / Uint8Array (NaN-boxed POINTER_TAG → BufferHeader):
///     copy the raw bytes.
///   - Blob handle (NaN-boxed POINTER_TAG → small int id):
///     fetch the registered body bytes.
///   - Array (NaN-boxed POINTER_TAG → ArrayHeader): recurse so
///     `[["a", "b"], "c"]` flattens like Node's behavior.
/// Anything else is silently dropped; matches Node's relaxed
/// behavior for stringifying non-recognized inputs to "".
unsafe fn append_blob_part_bytes(part: f64, out: &mut Vec<u8>) {
    let bits = part.to_bits();
    let top16 = bits >> 48;
    // STRING_TAG ─ NaN-box a *mut StringHeader.
    if top16 == 0x7FFF {
        let str_ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        if let Some(s) = string_from_header(str_ptr) {
            out.extend_from_slice(s.as_bytes());
        }
        return;
    }
    // POINTER_TAG ─ either a buffer, an array (recurse), or a small-id Blob.
    if top16 == 0x7FFD {
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // Small id → registered Blob handle. The Blob FFI carries
        // ids well below page-size so the cutoff is safe.
        if addr != 0 && addr < 0x10000 {
            if let Some(body) = blob_bytes_clone(addr) {
                out.extend_from_slice(&body);
                return;
            }
        }
        // BufferHeader?
        if addr >= 0x1000 && perry_runtime::buffer::is_registered_buffer(addr) {
            let buf = addr as *const perry_runtime::buffer::BufferHeader;
            let len = (*buf).length as usize;
            let data = perry_runtime::buffer::buffer_data(buf);
            out.extend_from_slice(std::slice::from_raw_parts(data, len));
            return;
        }
        // Array? Walk elements and recurse.
        let arr_ptr = addr as *const perry_runtime::array::ArrayHeader;
        if !arr_ptr.is_null() && addr >= 0x1000 {
            let gc_header = (arr_ptr as *const u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
                as *const perry_runtime::gc::GcHeader;
            let obj_type = (*gc_header).obj_type;
            if obj_type == perry_runtime::gc::GC_TYPE_ARRAY
                || obj_type == perry_runtime::gc::GC_TYPE_LAZY_ARRAY
            {
                let len = perry_runtime::array::js_array_length(arr_ptr);
                for i in 0..len {
                    let elem = perry_runtime::array::js_array_get(arr_ptr, i);
                    append_blob_part_bytes(f64::from_bits(elem.bits()), out);
                }
            }
        }
    }
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
    append_blob_part_bytes(parts, &mut body);
    let type_str = {
        let bits = content_type.to_bits();
        if (bits >> 48) == 0x7FFF {
            let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            string_from_header(p).unwrap_or_default()
        } else {
            String::new()
        }
    };
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
    append_blob_part_bytes(parts, &mut body);
    let name_str = {
        let bits = name.to_bits();
        if (bits >> 48) == 0x7FFF {
            let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            string_from_header(p).unwrap_or_default()
        } else {
            String::new()
        }
    };
    let type_str = {
        let bits = content_type.to_bits();
        if (bits >> 48) == 0x7FFF {
            let p = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
            string_from_header(p).unwrap_or_default()
        } else {
            String::new()
        }
    };
    // `last_modified` is a plain numeric f64 argument (callers pass
    // NaN to signal "use Date.now()" — match Node's default).
    let lm = if last_modified.is_nan() {
        // Cheap stamp: wall clock in ms.  Same source the codegen
        // uses for `Date.now()` so two consecutive `new File()` calls
        // produce a monotonic-ish sequence.
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0)
    } else {
        last_modified
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
/// fresh `blob:nodedata:<n>` URL and return the URL string.
#[no_mangle]
pub unsafe extern "C" fn js_url_create_object_url(blob_handle: f64) -> *mut StringHeader {
    let id = handle_id(blob_handle);
    if id == 0 {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }
    let url = {
        let mut counter = NEXT_OBJECT_URL_ID.lock().unwrap();
        let n = *counter;
        *counter += 1;
        // Node's published shape: `blob:nodedata:<uuid>`. We use a
        // monotonic counter — the actual identity bytes don't matter
        // to `resolveObjectURL` so long as they round-trip.
        format!("blob:nodedata:{:032x}", n)
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
