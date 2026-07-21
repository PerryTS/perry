use super::*;

use crate::closure::{js_closure_get_capture_ptr, ClosureHeader};
use crate::object::{
    js_implicit_this_get, js_object_get_field_by_name_f64, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

#[inline]
pub(crate) fn cp_undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED_BITS)
}

#[inline]
pub(crate) fn cp_box_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

/// Recover the host object value captured in closure slot 0 by `cp_build_object`.
#[inline]
pub(crate) fn cp_this(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return js_implicit_this_get();
    }
    f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64)
}

/// Resolve a NaN-boxed value to an `ObjectHeader*` iff it is a heap object.
pub(crate) fn cp_object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

/// Resolve a NaN-boxed value to an `ArrayHeader*` iff it is a heap array.
pub(crate) fn cp_array_ptr(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let bits = value.to_bits();
    if !JSValue::from_bits(bits).is_pointer() {
        return None;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let t = (*header).obj_type;
        if t == crate::gc::GC_TYPE_ARRAY || t == crate::gc::GC_TYPE_LAZY_ARRAY {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

#[inline]
pub(crate) fn cp_str_key(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

pub(crate) fn cp_get_field(value: f64, name: &[u8]) -> f64 {
    match cp_object_ptr(value) {
        Some(obj) => js_object_get_field_by_name_f64(obj, cp_str_key(name)),
        None => cp_undefined(),
    }
}

pub(crate) fn cp_set_field(value: f64, name: &[u8], field_value: f64) {
    if let Some(obj) = cp_object_ptr(value) {
        js_object_set_field_by_name(obj, cp_str_key(name), field_value);
    }
}

#[inline]
pub(crate) fn cp_box_string(s: &str) -> f64 {
    let sh = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    crate::value::js_nanbox_string(sh as i64)
}

/// SSO-safe extraction of a JS string value to an owned Rust string. The fixed
/// child_process event names (`data`/`end`/`exit`/`close`/`spawn`/`error`) and
/// many argv entries are ≤5 bytes — i.e. SSO short strings — which the file's
/// `extract_string_from_nanboxed` (STRING_TAG + StringHeader only) misses, so
/// route through the unified accessor which materializes SSO bytes.
pub(crate) fn cp_value_to_string(value: f64) -> Option<String> {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len))
            .ok()
            .map(|s| s.to_string())
    }
}

/// Best-effort decode of a `write()` chunk (Buffer or string) to raw bytes.
pub(crate) fn cp_value_to_bytes(value: f64) -> Vec<u8> {
    // Buffer fast-path.
    let bits = value.to_bits();
    if JSValue::from_bits(bits).is_pointer() {
        let raw = (bits & crate::value::POINTER_MASK) as usize;
        if raw >= 0x10000 {
            if crate::buffer::is_registered_buffer(raw) {
                let buf = raw as *const crate::buffer::BufferHeader;
                unsafe {
                    let len = (*buf).length as usize;
                    let data =
                        (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                    return std::slice::from_raw_parts(data, len).to_vec();
                }
            }
            if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
                let ta = raw as *const crate::typedarray::TypedArrayHeader;
                unsafe {
                    if let Some(bytes) = crate::typedarray::typed_array_bytes(ta) {
                        return bytes.to_vec();
                    }
                }
            }
        }
    }
    // Otherwise stringify.
    cp_value_to_string(value)
        .or_else(|| Some(cp_coerce_string(value)))
        .unwrap_or_default()
        .into_bytes()
}

/// NaN-boxed `Buffer` value holding `bytes`.
pub(crate) fn cp_make_buffer(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return cp_undefined();
    }
    unsafe {
        let data = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        (*buf).length = bytes.len() as u32;
    }
    cp_box_ptr(buf as *const u8)
}

pub(crate) unsafe fn cp_read_string_header(ptr: i64) -> String {
    if ptr == 0 {
        return String::new();
    }
    let sh = ptr as *const StringHeader;
    let len = (*sh).byte_len as usize;
    let data = (sh as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

pub(crate) unsafe fn cp_read_arg_strings(args_ptr: i64) -> Vec<String> {
    let mut out = Vec::new();
    // `args_ptr` is the unboxed lower-48-bit pointer. Codegen strips the NaN-box
    // tag, so `null`/`undefined`/a non-array object arrive here as a small or
    // non-array pointer (e.g. masked `null` == 2). #3079: only dereference it as
    // an array when it is a real heap array — otherwise treat it as an empty
    // args list (Node accepts `null`/`undefined`/`{}` as no args). Without this
    // guard `spawnSync("echo", null)` dereferences a bogus pointer and crashes.
    let raw = args_ptr as usize;
    if raw < 0x10000 {
        return out;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let t = (*header).obj_type;
    if t != crate::gc::GC_TYPE_ARRAY && t != crate::gc::GC_TYPE_LAZY_ARRAY {
        return out;
    }
    let arr = args_ptr as *const crate::array::ArrayHeader;
    let n = (*arr).length as usize;
    let data =
        (arr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    for i in 0..n {
        if let Some(s) = cp_value_to_string(*data.add(i)) {
            out.push(s);
        }
    }
    out
}

/// Collect a NaN-boxed args value (array of strings) into owned Rust strings.
pub(crate) fn cp_args_from_value(value: f64) -> Vec<String> {
    match cp_array_ptr(value) {
        Some(arr) => {
            let n = unsafe { (*arr).length };
            let mut out = Vec::with_capacity(n as usize);
            for i in 0..n {
                if let Some(s) = cp_value_to_string(crate::array::js_array_get_f64(arr, i)) {
                    out.push(s);
                }
            }
            out
        }
        None => Vec::new(),
    }
}

/// Coerce any JS value to an owned Rust string — string fast-path, else
/// `js_jsvalue_to_string`. Used for `env` values, which Node stringifies.
pub(crate) fn cp_coerce_string(value: f64) -> String {
    if let Some(s) = cp_value_to_string(value) {
        return s;
    }
    let p = crate::value::js_jsvalue_to_string(value);
    if p.is_null() {
        return String::new();
    }
    unsafe { cp_read_string_header(p as i64) }
}

#[inline]
pub(crate) fn cp_box_string_bytes(bytes: &[u8]) -> f64 {
    let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(p as i64)
}
