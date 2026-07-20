//! Bun globals shim pack (#6560): `Bun.stringWidth`, `Bun.file`, `Bun.write`,
//! `Bun.stdin` / `Bun.stdout` / `Bun.stderr`, `Bun.hash`, plus the `"bun"`
//! module's `pathToFileURL` / `fileURLToPath` aliases (wired in codegen to the
//! `node:url` implementations).
//!
//! Design notes:
//! - `Bun.file(path)` returns a plain heap object whose async methods
//!   (`text` / `json` / `arrayBuffer` / `exists`) are closure-valued
//!   properties capturing the path, so calls dispatch through the ordinary
//!   dynamic property-call path with no per-class codegen support (same shape
//!   as `readline/promises` interfaces in `node_submodules/fs_promises.rs`).
//!   I/O is performed synchronously at call time and surfaced through an
//!   already-settled Promise (the `fs.promises` shims do the same).
//! - `.size` / `.type` are data properties computed at `Bun.file()` time
//!   (Bun stats lazily on access; a fresh handle — the common pattern —
//!   observes identical values).
//! - `Bun.stdin` builds a fresh BunFile-like object per property read
//!   (identity is not preserved across reads; `Bun.stdin.text()` — the
//!   driver-app usage — is unaffected).
//! - `Bun.hash` is Zig-std Wyhash (see `wyhash.rs`) returning a BigInt, so
//!   `.toString(16)` cache keys match bun-run installs.

mod string_width;
mod width_tables;
mod wyhash;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{js_jsvalue_to_string, JSValue};
use std::io::{Read, Write};

pub use string_width::bun_string_width;
pub use wyhash::wyhash;

const BUN_FILE_PATH_KEY: &[u8] = b"__perryBunFilePath";
const BUN_STD_FD_KEY: &[u8] = b"__perryBunStdFd";

// ---------------------------------------------------------------------------
// Small value helpers (same shapes as node_submodules/fs_promises.rs)
// ---------------------------------------------------------------------------

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn boxed_str(bytes: &[u8]) -> f64 {
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn key_ptr(key: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(key.as_ptr(), key.len() as u32)
}

fn promise_value(value: f64) -> f64 {
    let promise = crate::promise::js_promise_new();
    crate::promise::js_promise_resolve(promise, value);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn promise_rejected(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn string_header_to_string(ptr: *const StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

fn value_to_string(value: f64) -> String {
    string_header_to_string(js_jsvalue_to_string(value) as *const StringHeader)
}

fn is_undefined_or_null(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_undefined() || js.is_null()
}

fn is_string_value(value: f64) -> bool {
    let js = JSValue::from_bits(value.to_bits());
    js.is_string() || js.is_short_string()
}

fn heap_ptr_from_value(value: f64) -> Option<usize> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return None;
    }
    let addr = js.as_pointer::<u8>() as usize;
    // Reject the whole small-handle band, not just a hand-rolled floor: a
    // fetch/zlib/proxy handle id sits in [0x10000, 0x100000) and would be
    // dereferenced as an ObjectHeader here, segfaulting on Linux (#6279).
    if crate::value::addr_class::is_handle_band(addr) {
        None
    } else {
        Some(addr)
    }
}

fn object_field(value: f64, key: &[u8]) -> Option<f64> {
    let obj = heap_ptr_from_value(value)? as *const crate::object::ObjectHeader;
    let field = js_object_get_field_by_name_f64(obj, key_ptr(key));
    if JSValue::from_bits(field.to_bits()).is_undefined() {
        None
    } else {
        Some(field)
    }
}

fn set_field(obj: *mut crate::object::ObjectHeader, key: &[u8], value: f64) {
    js_object_set_field_by_name(obj, key_ptr(key), value);
}

fn bound_method0(func: extern "C" fn(*const ClosureHeader) -> f64, capture: f64) -> f64 {
    js_register_closure_arity(func as *const u8, 0);
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_f64(closure, 0, capture);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

fn captured(closure: *const ClosureHeader) -> f64 {
    js_closure_get_capture_f64(closure, 0)
}

/// Bytes for a hash/write payload: string → UTF-8 bytes, TypedArray /
/// DataView → element bytes, ArrayBuffer → backing bytes, BunFile → the
/// referenced file's contents (an I/O error surfaces via the `Err`), other
/// values → their string coercion (lenient, like a JS shim would behave).
fn payload_bytes(value: f64) -> Result<Vec<u8>, f64> {
    if is_string_value(value) {
        return Ok(value_to_string(value).into_bytes());
    }
    if let Some(addr) = heap_ptr_from_value(value) {
        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            let ta = addr as *const crate::typedarray::TypedArrayHeader;
            if let Some(bytes) = unsafe { crate::typedarray::typed_array_bytes(ta) } {
                return Ok(bytes.to_vec());
            }
        }
        if crate::buffer::is_array_buffer(addr) {
            let buf = addr as *const crate::buffer::BufferHeader;
            unsafe {
                let len = (*buf).length as usize;
                let data =
                    (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                return Ok(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
        if let Some(path_value) = object_field(value, BUN_FILE_PATH_KEY) {
            let path = value_to_string(path_value);
            return std::fs::read(&path)
                .map_err(|err| unsafe { crate::fs::build_fs_error_value(&err, "open", &path) });
        }
    }
    Ok(value_to_string(value).into_bytes())
}

// ---------------------------------------------------------------------------
// Bun.stringWidth
// ---------------------------------------------------------------------------

/// `Bun.stringWidth(input, options?)`. Non-string inputs are string-coerced
/// (Bun does the same: `Bun.stringWidth(123) === 3`).
#[no_mangle]
pub extern "C" fn js_bun_string_width(input: f64, options: f64) -> f64 {
    let text = value_to_string(input);
    let count_ansi = object_field(options, b"countAnsiEscapeCodes")
        .map(|v| crate::value::js_is_truthy(v) != 0)
        .unwrap_or(false);
    let ambiguous_is_narrow = object_field(options, b"ambiguousIsNarrow")
        .map(|v| crate::value::js_is_truthy(v) != 0)
        .unwrap_or(true);
    let cps: Vec<u32> = text.chars().map(u32::from).collect();
    bun_string_width(&cps, count_ansi, ambiguous_is_narrow) as f64
}

// ---------------------------------------------------------------------------
// Bun.hash
// ---------------------------------------------------------------------------

/// `Bun.hash(input, seed?)` — Wyhash64, returns a BigInt. `seed` may be a
/// number or a bigint (Bun accepts both; they hash identically).
#[no_mangle]
pub extern "C" fn js_bun_hash(input: f64, seed: f64) -> f64 {
    let seed = hash_seed(seed);
    let bytes = match payload_bytes(input) {
        Ok(bytes) => bytes,
        // Unreadable BunFile input — hash the empty payload rather than
        // throwing from a sync context (Bun.hash only accepts in-memory
        // payloads in practice).
        Err(_) => Vec::new(),
    };
    let h = wyhash(seed, &bytes);
    let ptr = crate::bigint::js_bigint_from_u64(h);
    crate::value::js_nanbox_bigint(ptr as i64)
}

fn hash_seed(seed: f64) -> u64 {
    if is_undefined_or_null(seed) {
        return 0;
    }
    if crate::value::js_nanbox_is_bigint(seed) != 0 {
        let ptr = crate::value::js_nanbox_get_bigint(seed) as *const crate::bigint::BigIntHeader;
        if !ptr.is_null() {
            // 1024-bit two's complement; the low limb is the wrapped u64.
            return unsafe { (*ptr).limbs[0] };
        }
        return 0;
    }
    let js = JSValue::from_bits(seed.to_bits());
    if js.is_int32() {
        return js.as_int32() as i64 as u64;
    }
    if seed.is_finite() {
        return seed as i64 as u64;
    }
    0
}

// ---------------------------------------------------------------------------
// Bun.file / BunFile methods
// ---------------------------------------------------------------------------

fn mime_type_for_path(path: &str) -> &'static str {
    let ext = path.rsplit('/').next().unwrap_or(path);
    let ext = match ext.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext,
        _ => "",
    };
    // Matches Bun's mapping for the common extensions (probed on v1.3.12).
    match ext.to_ascii_lowercase().as_str() {
        "txt" | "text" => "text/plain;charset=utf-8",
        "json" => "application/json;charset=utf-8",
        "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "tsx" | "jsx" => {
            "text/javascript;charset=utf-8"
        }
        "html" | "htm" => "text/html;charset=utf-8",
        "css" => "text/css;charset=utf-8",
        "md" => "text/markdown",
        "yml" | "yaml" => "text/yaml",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn read_file_or_reject(path: &str) -> Result<Vec<u8>, f64> {
    std::fs::read(path)
        .map_err(|err| unsafe { crate::fs::build_fs_error_value(&err, "open", path) })
}

extern "C" fn bun_file_text(closure: *const ClosureHeader) -> f64 {
    let path = value_to_string(captured(closure));
    match read_file_or_reject(&path) {
        Ok(bytes) => promise_value(boxed_str(String::from_utf8_lossy(&bytes).as_bytes())),
        Err(err) => promise_rejected(err),
    }
}

extern "C" fn bun_file_json(closure: *const ClosureHeader) -> f64 {
    let path = value_to_string(captured(closure));
    match read_file_or_reject(&path) {
        Ok(bytes) => json_parse_promise(&bytes),
        Err(err) => promise_rejected(err),
    }
}

extern "C" fn bun_file_array_buffer(closure: *const ClosureHeader) -> f64 {
    let path = value_to_string(captured(closure));
    match read_file_or_reject(&path) {
        Ok(bytes) => promise_value(array_buffer_from_bytes(&bytes)),
        Err(err) => promise_rejected(err),
    }
}

extern "C" fn bun_file_exists(closure: *const ClosureHeader) -> f64 {
    let path = value_to_string(captured(closure));
    promise_value(bool_value(
        std::fs::metadata(&path)
            .map(|m| m.is_file())
            .unwrap_or(false),
    ))
}

fn json_parse_promise(bytes: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(bytes);
    let text_ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    match unsafe { crate::json::js_json_parse_result(text_ptr) } {
        Ok(value) => promise_value(f64::from_bits(value.bits())),
        Err(err) => promise_rejected(err),
    }
}

fn array_buffer_from_bytes(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_array_buffer_new(bytes.len() as i32);
    unsafe {
        let data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    }
    f64::from_bits(JSValue::pointer(buf as *const u8).bits())
}

/// `Bun.file(path)` — BunFile-like lazy handle.
#[no_mangle]
pub extern "C" fn js_bun_file(path: f64) -> f64 {
    let path_string = value_to_string(path);
    let path_value = boxed_str(path_string.as_bytes());

    let obj = js_object_alloc(0, 8);
    set_field(obj, BUN_FILE_PATH_KEY, path_value);
    set_field(obj, b"name", path_value);
    let size = std::fs::metadata(&path_string)
        .map(|m| m.len())
        .unwrap_or(0);
    set_field(obj, b"size", size as f64);
    set_field(
        obj,
        b"type",
        boxed_str(mime_type_for_path(&path_string).as_bytes()),
    );
    set_field(obj, b"text", bound_method0(bun_file_text, path_value));
    set_field(obj, b"json", bound_method0(bun_file_json, path_value));
    set_field(
        obj,
        b"arrayBuffer",
        bound_method0(bun_file_array_buffer, path_value),
    );
    set_field(obj, b"exists", bound_method0(bun_file_exists, path_value));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

// ---------------------------------------------------------------------------
// Bun.write
// ---------------------------------------------------------------------------

/// `Bun.write(dest, data)` — resolves with the number of bytes written.
/// `dest`: path string, BunFile, or `Bun.stdout` / `Bun.stderr`.
/// `data`: string, TypedArray/DataView, ArrayBuffer, or BunFile.
/// Parent directories of a path destination are created (Bun parity).
#[no_mangle]
pub extern "C" fn js_bun_write(dest: f64, data: f64) -> f64 {
    let bytes = match payload_bytes(data) {
        Ok(bytes) => bytes,
        Err(err) => return promise_rejected(err),
    };

    // Bun.stdout / Bun.stderr destination.
    if let Some(fd_value) = object_field(dest, BUN_STD_FD_KEY) {
        let result = if fd_value as i32 == 2 {
            let mut out = std::io::stderr().lock();
            out.write_all(&bytes).and_then(|_| out.flush())
        } else {
            let mut out = std::io::stdout().lock();
            out.write_all(&bytes).and_then(|_| out.flush())
        };
        return match result {
            Ok(()) => promise_value(bytes.len() as f64),
            Err(err) => {
                promise_rejected(unsafe { crate::fs::build_fs_error_value(&err, "write", "") })
            }
        };
    }

    // Path (string) or BunFile destination.
    let path = if is_string_value(dest) {
        value_to_string(dest)
    } else if let Some(path_value) = object_field(dest, BUN_FILE_PATH_KEY) {
        value_to_string(path_value)
    } else {
        let msg = b"Bun.write: destination must be a path, BunFile, or Bun.stdout/stderr";
        let msg_ptr = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg_ptr);
        return promise_rejected(f64::from_bits(JSValue::pointer(err as *const u8).bits()));
    };

    if let Some(parent) = std::path::Path::new(&path).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    match std::fs::write(&path, &bytes) {
        Ok(()) => promise_value(bytes.len() as f64),
        Err(err) => {
            promise_rejected(unsafe { crate::fs::build_fs_error_value(&err, "open", &path) })
        }
    }
}

// ---------------------------------------------------------------------------
// Bun.stdin / Bun.stdout / Bun.stderr
// ---------------------------------------------------------------------------

extern "C" fn bun_stdin_text(_closure: *const ClosureHeader) -> f64 {
    let mut buf = Vec::new();
    match std::io::stdin().lock().read_to_end(&mut buf) {
        Ok(_) => promise_value(boxed_str(String::from_utf8_lossy(&buf).as_bytes())),
        Err(err) => promise_rejected(unsafe { crate::fs::build_fs_error_value(&err, "read", "") }),
    }
}

extern "C" fn bun_stdin_json(_closure: *const ClosureHeader) -> f64 {
    let mut buf = Vec::new();
    match std::io::stdin().lock().read_to_end(&mut buf) {
        Ok(_) => json_parse_promise(&buf),
        Err(err) => promise_rejected(unsafe { crate::fs::build_fs_error_value(&err, "read", "") }),
    }
}

extern "C" fn bun_stdin_array_buffer(_closure: *const ClosureHeader) -> f64 {
    let mut buf = Vec::new();
    match std::io::stdin().lock().read_to_end(&mut buf) {
        Ok(_) => promise_value(array_buffer_from_bytes(&buf)),
        Err(err) => promise_rejected(unsafe { crate::fs::build_fs_error_value(&err, "read", "") }),
    }
}

/// `Bun.stdin` — minimal BunFile-like read handle over process stdin.
#[no_mangle]
pub extern "C" fn js_bun_stdin() -> f64 {
    let obj = js_object_alloc(0, 8);
    set_field(obj, BUN_STD_FD_KEY, 0.0);
    set_field(obj, b"size", f64::INFINITY);
    set_field(obj, b"type", boxed_str(b"application/octet-stream"));
    set_field(obj, b"text", bound_method0(bun_stdin_text, undefined()));
    set_field(obj, b"json", bound_method0(bun_stdin_json, undefined()));
    set_field(
        obj,
        b"arrayBuffer",
        bound_method0(bun_stdin_array_buffer, undefined()),
    );
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

fn bun_std_out_like(fd: f64) -> f64 {
    let obj = js_object_alloc(0, 4);
    set_field(obj, BUN_STD_FD_KEY, fd);
    set_field(obj, b"type", boxed_str(b"application/octet-stream"));
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

/// `Bun.stdout` — write target for `Bun.write(Bun.stdout, ...)`.
#[no_mangle]
pub extern "C" fn js_bun_stdout() -> f64 {
    bun_std_out_like(1.0)
}

/// `Bun.stderr` — write target for `Bun.write(Bun.stderr, ...)`.
#[no_mangle]
pub extern "C" fn js_bun_stderr() -> f64 {
    bun_std_out_like(2.0)
}
