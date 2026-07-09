//! Blob value construction for `node:stream/consumers` (`blob`) and the
//! Blob-instance method thunks (`text`, `arrayBuffer`, `bytes`, `slice`,
//! `stream`).
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.

use super::consumers::{
    buffer_from_bytes, bytes_to_array_buffer_value, bytes_to_text_value, bytes_to_uint8_array_value,
};
use super::fs_promises::{promise_rejected, promise_value};
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(not(unix))]
use std::time::UNIX_EPOCH;

const CLASS_ID_BLOB: u32 = 0xFFFF0026;

#[derive(Clone)]
struct FileBlobState {
    path: String,
    fingerprint: FileBlobFingerprint,
    offset: u64,
    length: u64,
    content_type: String,
}

#[derive(Clone, PartialEq, Eq)]
struct FileBlobFingerprint {
    len: u64,
    is_file: bool,
    is_dir: bool,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    mode: u32,
    #[cfg(unix)]
    mtime: i64,
    #[cfg(unix)]
    mtime_nsec: i64,
    #[cfg(unix)]
    ctime: i64,
    #[cfg(unix)]
    ctime_nsec: i64,
    #[cfg(not(unix))]
    modified_ns: u128,
}

#[derive(Clone, Copy)]
struct FileBlobStreamState {
    blob_id: usize,
    consumed: bool,
}

thread_local! {
    static FILE_BLOBS: RefCell<HashMap<usize, FileBlobState>> = RefCell::new(HashMap::new());
    static NEXT_FILE_BLOB_ID: RefCell<usize> = const { RefCell::new(1) };
    static FILE_BLOB_STREAMS: RefCell<HashMap<usize, FileBlobStreamState>> = RefCell::new(HashMap::new());
    static NEXT_FILE_BLOB_STREAM_ID: RefCell<usize> = const { RefCell::new(1) };
}

extern "C" fn blob_text_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_text_value(&bytes))
}

extern "C" fn blob_array_buffer_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_array_buffer_value(&bytes))
}

extern "C" fn blob_bytes_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    promise_value(bytes_to_uint8_array_value(&bytes))
}

extern "C" fn blob_slice_method(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
    content_type: f64,
) -> f64 {
    let bytes = captured_blob_bytes(closure);
    let len = bytes.len() as i64;
    let normalize = |value: f64, default: i64| -> i64 {
        if value.is_nan() || value.to_bits() == crate::value::TAG_UNDEFINED {
            return default;
        }
        let n = value as i64;
        if n < 0 {
            (len + n).max(0)
        } else {
            n.min(len)
        }
    };
    let lo = normalize(start, 0);
    let hi = normalize(end, len);
    let (lo, hi) = if hi < lo { (lo, lo) } else { (lo, hi) };
    let content_type = string_from_value(content_type).unwrap_or_default();
    blob_value_from_bytes_and_type(&bytes[lo as usize..hi as usize], &content_type)
}

extern "C" fn blob_stream_method(closure: *const ClosureHeader) -> f64 {
    let bytes = captured_blob_bytes(closure);
    crate::node_stream::js_node_stream_readable_from(bytes_to_uint8_array_value(&bytes))
}

extern "C" fn file_blob_text_method(closure: *const ClosureHeader) -> f64 {
    match read_file_blob_bytes(captured_file_blob_id(closure)) {
        Ok(bytes) => promise_value(bytes_to_text_value(&bytes)),
        Err(reason) => promise_rejected(reason),
    }
}

extern "C" fn file_blob_array_buffer_method(closure: *const ClosureHeader) -> f64 {
    match read_file_blob_bytes(captured_file_blob_id(closure)) {
        Ok(bytes) => promise_value(bytes_to_array_buffer_value(&bytes)),
        Err(reason) => promise_rejected(reason),
    }
}

extern "C" fn file_blob_bytes_method(closure: *const ClosureHeader) -> f64 {
    match read_file_blob_bytes(captured_file_blob_id(closure)) {
        Ok(bytes) => promise_value(bytes_to_uint8_array_value(&bytes)),
        Err(reason) => promise_rejected(reason),
    }
}

extern "C" fn file_blob_slice_method(
    closure: *const ClosureHeader,
    start: f64,
    end: f64,
    content_type: f64,
) -> f64 {
    let id = captured_file_blob_id(closure);
    let Some(state) = file_blob_state(id) else {
        return blob_value_from_bytes_and_type(&[], "");
    };
    let len = state.length as i64;
    let normalize = |value: f64, default: i64| -> i64 {
        if value.is_nan() || value.to_bits() == crate::value::TAG_UNDEFINED {
            return default;
        }
        let n = value as i64;
        if n < 0 {
            (len + n).max(0)
        } else {
            n.min(len)
        }
    };
    let lo = normalize(start, 0);
    let hi = normalize(end, len);
    let (lo, hi) = if hi < lo { (lo, lo) } else { (lo, hi) };
    let content_type = string_from_value(content_type)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let child = FileBlobState {
        path: state.path,
        fingerprint: state.fingerprint,
        offset: state.offset.saturating_add(lo as u64),
        length: (hi - lo) as u64,
        content_type,
    };
    blob_value_from_file_state(child)
}

extern "C" fn file_blob_stream_method(closure: *const ClosureHeader) -> f64 {
    file_blob_stream_value(captured_file_blob_id(closure))
}

extern "C" fn file_blob_stream_get_reader_method(closure: *const ClosureHeader) -> f64 {
    let stream_id = captured_file_blob_stream_id(closure);
    let obj = js_object_alloc(0, 4);
    set_named_value(
        obj,
        b"read",
        file_blob_stream_method_value(file_blob_stream_next_method as *const u8, 0, stream_id),
    );
    set_named_value(
        obj,
        b"releaseLock",
        file_blob_stream_method_value(file_blob_stream_undefined_method as *const u8, 0, stream_id),
    );
    set_named_value(
        obj,
        b"cancel",
        file_blob_stream_method_value(file_blob_stream_cancel_method as *const u8, 1, stream_id),
    );
    set_named_value(
        obj,
        b"closed",
        promise_value(f64::from_bits(crate::value::TAG_UNDEFINED)),
    );
    object_value(obj)
}

extern "C" fn file_blob_stream_next_method(closure: *const ClosureHeader) -> f64 {
    file_blob_stream_next(captured_file_blob_stream_id(closure))
}

extern "C" fn file_blob_stream_return_method(closure: *const ClosureHeader) -> f64 {
    let stream_id = captured_file_blob_stream_id(closure);
    FILE_BLOB_STREAMS.with(|streams| {
        if let Some(state) = streams.borrow_mut().get_mut(&stream_id) {
            state.consumed = true;
        }
    });
    resolved_iterator_promise(f64::from_bits(crate::value::TAG_UNDEFINED), true)
}

extern "C" fn file_blob_stream_values_method(closure: *const ClosureHeader, _options: f64) -> f64 {
    let _ = closure;
    crate::object::js_implicit_this_get()
}

extern "C" fn file_blob_stream_async_iterator_method(_closure: *const ClosureHeader) -> f64 {
    crate::object::js_implicit_this_get()
}

extern "C" fn file_blob_stream_undefined_method(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn file_blob_stream_cancel_method(closure: *const ClosureHeader, _reason: f64) -> f64 {
    let stream_id = captured_file_blob_stream_id(closure);
    FILE_BLOB_STREAMS.with(|streams| {
        if let Some(state) = streams.borrow_mut().get_mut(&stream_id) {
            state.consumed = true;
        }
    });
    promise_value(f64::from_bits(crate::value::TAG_UNDEFINED))
}

fn captured_file_blob_id(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

fn captured_file_blob_stream_id(closure: *const ClosureHeader) -> usize {
    js_closure_get_capture_ptr(closure, 0) as usize
}

fn captured_blob_bytes(closure: *const ClosureHeader) -> Vec<u8> {
    let raw = js_closure_get_capture_ptr(closure, 0) as usize;
    if raw < 0x10000 || !crate::buffer::is_registered_buffer(raw) {
        return Vec::new();
    }
    unsafe {
        let buf = raw as *const crate::buffer::BufferHeader;
        let len = (*buf).length as usize;
        let data = crate::buffer::buffer_data(buf);
        std::slice::from_raw_parts(data, len).to_vec()
    }
}

fn set_named_value(obj: *mut ObjectHeader, name: &[u8], value: f64) {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(obj, key, value);
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[allow(clippy::missing_transmute_annotations)]
fn blob_method_value(
    func: *const u8,
    arity: u32,
    backing: *mut crate::buffer::BufferHeader,
) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(closure, 0, backing as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

#[allow(clippy::missing_transmute_annotations)]
fn file_blob_method_value(func: *const u8, arity: u32, id: usize) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

#[allow(clippy::missing_transmute_annotations)]
fn file_blob_stream_method_value(func: *const u8, arity: u32, id: usize) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(closure, 0, id as i64);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

#[allow(clippy::missing_transmute_annotations)]
fn file_blob_stream_self_method_value(func: *const u8, arity: u32) -> f64 {
    js_register_closure_arity(func, arity);
    let closure = js_closure_alloc(func, 0);
    f64::from_bits(JSValue::pointer(closure as *const u8).bits())
}

pub(crate) fn blob_value_from_bytes(bytes: &[u8]) -> f64 {
    blob_value_from_bytes_and_type(bytes, "")
}

fn blob_value_from_bytes_and_type(bytes: &[u8], content_type: &str) -> f64 {
    let backing = buffer_from_bytes(bytes, false, false);
    let obj = js_object_alloc(CLASS_ID_BLOB, 7);
    set_named_value(obj, b"size", bytes.len() as f64);
    set_named_value(obj, b"type", bytes_to_text_value(content_type.as_bytes()));
    set_named_value(
        obj,
        b"text",
        blob_method_value(blob_text_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"arrayBuffer",
        blob_method_value(blob_array_buffer_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"bytes",
        blob_method_value(blob_bytes_method as *const u8, 0, backing),
    );
    set_named_value(
        obj,
        b"slice",
        blob_method_value(blob_slice_method as *const u8, 3, backing),
    );
    set_named_value(
        obj,
        b"stream",
        blob_method_value(blob_stream_method as *const u8, 0, backing),
    );
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

pub(crate) fn blob_value_from_file_path(
    path: &str,
    metadata: &fs::Metadata,
    content_type: String,
) -> f64 {
    blob_value_from_file_state(FileBlobState {
        path: path.to_string(),
        fingerprint: FileBlobFingerprint::from_metadata(metadata),
        offset: 0,
        length: metadata.len(),
        content_type,
    })
}

fn blob_value_from_file_state(state: FileBlobState) -> f64 {
    let size = state.length as f64;
    let content_type = state.content_type.clone();
    let id = register_file_blob_state(state);
    let obj = js_object_alloc(CLASS_ID_BLOB, 7);
    set_named_value(obj, b"size", size);
    set_named_value(obj, b"type", bytes_to_text_value(content_type.as_bytes()));
    set_named_value(
        obj,
        b"text",
        file_blob_method_value(file_blob_text_method as *const u8, 0, id),
    );
    set_named_value(
        obj,
        b"arrayBuffer",
        file_blob_method_value(file_blob_array_buffer_method as *const u8, 0, id),
    );
    set_named_value(
        obj,
        b"bytes",
        file_blob_method_value(file_blob_bytes_method as *const u8, 0, id),
    );
    set_named_value(
        obj,
        b"slice",
        file_blob_method_value(file_blob_slice_method as *const u8, 3, id),
    );
    set_named_value(
        obj,
        b"stream",
        file_blob_method_value(file_blob_stream_method as *const u8, 0, id),
    );
    object_value(obj)
}

pub(crate) fn string_from_value(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

impl FileBlobFingerprint {
    fn from_metadata(metadata: &fs::Metadata) -> Self {
        Self {
            len: metadata.len(),
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            #[cfg(unix)]
            dev: metadata.dev(),
            #[cfg(unix)]
            ino: metadata.ino(),
            #[cfg(unix)]
            mode: metadata.mode(),
            #[cfg(unix)]
            mtime: metadata.mtime(),
            #[cfg(unix)]
            mtime_nsec: metadata.mtime_nsec(),
            #[cfg(unix)]
            ctime: metadata.ctime(),
            #[cfg(unix)]
            ctime_nsec: metadata.ctime_nsec(),
            #[cfg(not(unix))]
            modified_ns: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or(0),
        }
    }
}

fn register_file_blob_state(state: FileBlobState) -> usize {
    let id = NEXT_FILE_BLOB_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1).max(1);
        id
    });
    FILE_BLOBS.with(|blobs| {
        blobs.borrow_mut().insert(id, state);
    });
    id
}

fn file_blob_state(id: usize) -> Option<FileBlobState> {
    FILE_BLOBS.with(|blobs| blobs.borrow().get(&id).cloned())
}

fn validate_file_blob_state(state: &FileBlobState) -> Result<(), f64> {
    let metadata = fs::metadata(&state.path).map_err(|_| not_readable_error_value())?;
    if FileBlobFingerprint::from_metadata(&metadata) != state.fingerprint {
        return Err(not_readable_error_value());
    }
    if !metadata.is_file() {
        return Err(not_readable_error_value());
    }
    if fs::File::open(&state.path).is_err() {
        return Err(not_readable_error_value());
    }
    Ok(())
}

fn read_file_blob_bytes(id: usize) -> Result<Vec<u8>, f64> {
    let state = file_blob_state(id).ok_or_else(not_readable_error_value)?;
    validate_file_blob_state(&state)?;
    let len = usize::try_from(state.length).map_err(|_| not_readable_error_value())?;
    let mut file = fs::File::open(&state.path).map_err(|_| not_readable_error_value())?;
    file.seek(SeekFrom::Start(state.offset))
        .map_err(|_| not_readable_error_value())?;
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes)
        .map_err(|_| not_readable_error_value())?;
    Ok(bytes)
}

fn not_readable_error_value() -> f64 {
    let obj = js_object_alloc(0, 4);
    set_named_value(obj, b"name", bytes_to_text_value(b"NotReadableError"));
    set_named_value(
        obj,
        b"message",
        bytes_to_text_value(b"The blob could not be read"),
    );
    set_named_value(obj, b"code", 0.0);
    set_named_value(obj, b"stack", bytes_to_text_value(b""));
    object_value(obj)
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn iterator_result(value: f64, done: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let obj = js_object_alloc(0, 2);
    set_named_value(obj, b"value", value.get_nanbox_f64());
    set_named_value(obj, b"done", bool_value(done));
    object_value(obj)
}

fn resolved_iterator_promise(value: f64, done: bool) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = iterator_result(value, done);
    let result = scope.root_nanbox_f64(result);
    let promise = crate::promise::js_promise_resolved(result.get_nanbox_f64());
    f64::from_bits(JSValue::pointer(promise as *const u8).bits())
}

fn file_blob_stream_value(blob_id: usize) -> f64 {
    let stream_id = NEXT_FILE_BLOB_STREAM_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next = next.saturating_add(1).max(1);
        id
    });
    FILE_BLOB_STREAMS.with(|streams| {
        streams.borrow_mut().insert(
            stream_id,
            FileBlobStreamState {
                blob_id,
                consumed: false,
            },
        );
    });

    let obj = js_object_alloc(0, 8);
    set_named_value(
        obj,
        b"getReader",
        file_blob_stream_method_value(
            file_blob_stream_get_reader_method as *const u8,
            0,
            stream_id,
        ),
    );
    set_named_value(
        obj,
        b"next",
        file_blob_stream_method_value(file_blob_stream_next_method as *const u8, 0, stream_id),
    );
    set_named_value(
        obj,
        b"return",
        file_blob_stream_method_value(file_blob_stream_return_method as *const u8, 0, stream_id),
    );
    set_named_value(
        obj,
        b"values",
        file_blob_stream_self_method_value(file_blob_stream_values_method as *const u8, 1),
    );
    set_named_value(
        obj,
        b"cancel",
        file_blob_stream_method_value(file_blob_stream_cancel_method as *const u8, 1, stream_id),
    );
    set_named_value(obj, b"locked", bool_value(false));
    let obj_value = object_value(obj);
    let sym = crate::symbol::well_known_symbol("asyncIterator");
    if !sym.is_null() {
        let sym_value = f64::from_bits(JSValue::pointer(sym as *const u8).bits());
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                obj_value,
                sym_value,
                file_blob_stream_self_method_value(
                    file_blob_stream_async_iterator_method as *const u8,
                    0,
                ),
            );
        }
    }
    obj_value
}

fn file_blob_stream_next(stream_id: usize) -> f64 {
    let action = FILE_BLOB_STREAMS.with(|streams| {
        let mut streams = streams.borrow_mut();
        let Some(state) = streams.get_mut(&stream_id) else {
            return None;
        };
        if state.consumed {
            return Some((state.blob_id, true));
        }
        state.consumed = true;
        Some((state.blob_id, false))
    });
    let Some((blob_id, already_done)) = action else {
        return resolved_iterator_promise(f64::from_bits(crate::value::TAG_UNDEFINED), true);
    };
    if already_done {
        return resolved_iterator_promise(f64::from_bits(crate::value::TAG_UNDEFINED), true);
    }
    let Some(state) = file_blob_state(blob_id) else {
        return promise_rejected(not_readable_error_value());
    };
    if state.length == 0 {
        return resolved_iterator_promise(f64::from_bits(crate::value::TAG_UNDEFINED), true);
    }
    match read_file_blob_bytes(blob_id) {
        Ok(bytes) => resolved_iterator_promise(bytes_to_uint8_array_value(&bytes), false),
        Err(reason) => promise_rejected(reason),
    }
}
