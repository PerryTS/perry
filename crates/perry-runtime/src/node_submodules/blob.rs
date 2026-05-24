//! Blob value construction for `node:stream/consumers` (`blob`) and the
//! Blob-instance method thunks (`text`, `arrayBuffer`, `bytes`, `slice`,
//! `stream`).
//!
//! Extracted from `mod.rs` so the parent module stays under the file-size
//! gate. Pure code movement — no logic changes.

use super::consumers::{
    buffer_from_bytes, bytes_to_array_buffer_value, bytes_to_text_value, bytes_to_uint8_array_value,
};
use super::fs_promises::promise_value;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_register_closure_arity, ClosureHeader,
};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

const CLASS_ID_BLOB: u32 = 0xFFFF0026;

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
