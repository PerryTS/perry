//! Minimal public `node:v8` compatibility surface.
//!
//! Perry is not backed by V8, but packages commonly use `node:v8` for
//! feature detection, structured-clone persistence, and heap-stat shape reads.
//! The serialization path reuses the child_process advanced-IPC wire codec,
//! which already speaks the V8 `ValueSerializer` byte format for the covered
//! value families.

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use std::cell::RefCell;
use std::collections::HashMap;

use crate::buffer::{buffer_alloc, buffer_data, buffer_data_mut, BufferHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::{JSValue, POINTER_MASK};
use crate::{ObjectHeader, SetHeader};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const V8_WIRE_FORMAT_VERSION: f64 = 15.0;

thread_local! {
    static INSTANCE_BUFFERS: RefCell<HashMap<usize, Vec<u8>>> = RefCell::new(HashMap::new());
}

#[inline]
fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

#[inline]
fn boxed_ptr(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

fn key(name: &str) -> *mut StringHeader {
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    js_object_set_field_by_name(obj, key(name), value);
}

fn object_from_fields(fields: &[(&str, f64)]) -> f64 {
    let obj = js_object_alloc(0, 0);
    for (name, value) in fields {
        set_field(obj, name, *value);
    }
    boxed_ptr(obj as *const u8)
}

fn array_from_values(values: &[f64]) -> f64 {
    let mut arr = js_array_alloc(values.len() as u32);
    for value in values {
        arr = js_array_push_f64(arr, *value);
    }
    boxed_ptr(arr as *const u8)
}

fn ptr_addr(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() {
        return Some((bits & POINTER_MASK) as usize);
    }
    if bits >= 0x10000 && (bits >> 48) == 0 {
        return Some(bits as usize);
    }
    None
}

fn object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    let raw = ptr_addr(value)?;
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

fn buffer_value(bytes: &[u8]) -> f64 {
    let buf = buffer_alloc(bytes.len() as u32);
    unsafe {
        if !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer_data_mut(buf), bytes.len());
        }
        (*buf).length = bytes.len() as u32;
    }
    boxed_ptr(buf as *const u8)
}

fn buffer_like_bytes(value: f64) -> Option<Vec<u8>> {
    let raw = ptr_addr(value)?;
    if crate::buffer::is_registered_buffer(raw) {
        let buf = raw as *const BufferHeader;
        let len = unsafe { (*buf).length as usize };
        if len == 0 {
            return Some(Vec::new());
        }
        let data = buffer_data(buf);
        return Some(unsafe { std::slice::from_raw_parts(data, len).to_vec() });
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        let ta = raw as *const crate::typedarray::TypedArrayHeader;
        return unsafe { crate::typedarray::typed_array_bytes(ta).map(|bytes| bytes.to_vec()) };
    }
    None
}

fn throw_invalid_buffer_arg() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView",
        "ERR_INVALID_ARG_TYPE",
    )
}

fn throw_data_clone_error() -> ! {
    let msg = key("function, symbol, and weak collection values cannot be cloned");
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn is_cloneable(value: f64, depth: u32) -> bool {
    if depth > 512 {
        return false;
    }
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return false;
    }
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined()
        || jsval.is_null()
        || jsval.is_bool()
        || jsval.is_int32()
        || jsval.is_number()
        || jsval.is_any_string()
        || jsval.is_bigint()
    {
        return true;
    }
    let Some(raw) = ptr_addr(value) else {
        return true;
    };
    if crate::closure::is_closure_ptr(raw) {
        return false;
    }
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || crate::date::is_date_value(value)
    {
        return true;
    }
    if crate::map::is_registered_map(raw) {
        let map = raw as *const crate::map::MapHeader;
        for i in 0..crate::map::js_map_size(map) {
            if !is_cloneable(crate::map::js_map_entry_key_at(map, i), depth + 1)
                || !is_cloneable(crate::map::js_map_entry_value_at(map, i), depth + 1)
            {
                return false;
            }
        }
        return true;
    }
    if crate::set::is_registered_set(raw) {
        let set = raw as *const SetHeader;
        for i in 0..crate::set::js_set_size(set) {
            if !is_cloneable(crate::set::js_set_value_at(set, i), depth + 1) {
                return false;
            }
        }
        return true;
    }
    if let Some(arr) = array_ptr(value) {
        for i in 0..js_array_length(arr) {
            if !is_cloneable(js_array_get_f64(arr, i), depth + 1) {
                return false;
            }
        }
        return true;
    }
    if let Some(obj) = object_ptr(value) {
        if crate::weakref::weak_wrapper_kind(obj).is_some() {
            return false;
        }
        return object_fields_cloneable(obj, depth + 1);
    }
    true
}

fn array_ptr(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let raw = ptr_addr(value)?;
    if raw < 0x10000 {
        return None;
    }
    unsafe {
        let header =
            (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let kind = (*header).obj_type;
        if kind == crate::gc::GC_TYPE_ARRAY || kind == crate::gc::GC_TYPE_LAZY_ARRAY {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

fn object_fields_cloneable(obj: *const ObjectHeader, depth: u32) -> bool {
    unsafe {
        let keys = (*obj).keys_array;
        if keys.is_null() {
            return true;
        }
        let key_count = (*keys).length;
        let num_fields = (*obj).field_count;
        let alloc_limit = std::cmp::max(num_fields, 8);
        let fields_ptr = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const f64;
        for i in 0..key_count {
            let value = if i < alloc_limit {
                *fields_ptr.add(i as usize)
            } else {
                f64::from_bits(crate::object::js_object_get_field(obj, i).bits())
            };
            if !is_cloneable(value, depth + 1) {
                return false;
            }
        }
    }
    true
}

#[no_mangle]
pub extern "C" fn js_v8_serialize(value: f64) -> f64 {
    if !is_cloneable(value, 0) {
        throw_data_clone_error();
    }
    let bytes = crate::child_process::v8_serde::v8_serialize(value);
    buffer_value(&bytes)
}

#[no_mangle]
pub extern "C" fn js_v8_deserialize(buffer: f64) -> f64 {
    let Some(bytes) = buffer_like_bytes(buffer) else {
        throw_invalid_buffer_arg();
    };
    crate::child_process::v8_serde::v8_deserialize(&bytes)
}

#[no_mangle]
pub extern "C" fn js_v8_cached_data_version_tag(_ignored: f64) -> f64 {
    15.0
}

#[no_mangle]
pub extern "C" fn js_v8_get_heap_statistics(_ignored: f64) -> f64 {
    object_from_fields(&[
        ("total_heap_size", 0.0),
        ("total_heap_size_executable", 0.0),
        ("total_physical_size", 0.0),
        ("total_available_size", 0.0),
        ("used_heap_size", 0.0),
        ("heap_size_limit", 512.0 * 1024.0 * 1024.0),
        ("malloced_memory", 0.0),
        ("peak_malloced_memory", 0.0),
        ("does_zap_garbage", 0.0),
        ("number_of_native_contexts", 1.0),
        ("number_of_detached_contexts", 0.0),
        ("total_global_handles_size", 0.0),
        ("used_global_handles_size", 0.0),
        ("external_memory", 0.0),
        ("total_allocated_bytes", 0.0),
    ])
}

#[no_mangle]
pub extern "C" fn js_v8_get_heap_code_statistics(_a: f64, _b: f64) -> f64 {
    object_from_fields(&[
        ("code_and_metadata_size", 0.0),
        ("bytecode_and_metadata_size", 0.0),
        ("external_script_source_size", 0.0),
        ("cpu_profiler_metadata_size", 0.0),
    ])
}

#[no_mangle]
pub extern "C" fn js_v8_get_heap_space_statistics(_ignored: f64) -> f64 {
    let names = [
        "read_only_space",
        "new_space",
        "old_space",
        "code_space",
        "map_space",
        "large_object_space",
    ];
    let values: Vec<f64> = names
        .iter()
        .map(|name| {
            object_from_fields(&[
                ("space_name", string_value(name)),
                ("space_size", 0.0),
                ("space_used_size", 0.0),
                ("space_available_size", 0.0),
                ("physical_space_size", 0.0),
            ])
        })
        .collect();
    array_from_values(&values)
}

fn namespace(name: &str) -> f64 {
    crate::object::js_create_native_module_namespace(name.as_ptr(), name.len())
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_new() -> f64 {
    namespace("v8.Serializer")
}

#[no_mangle]
pub extern "C" fn js_v8_default_serializer_new() -> f64 {
    namespace("v8.DefaultSerializer")
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_new(buffer: f64) -> f64 {
    let Some(bytes) = buffer_like_bytes(buffer) else {
        throw_invalid_buffer_arg();
    };
    let obj_value = namespace("v8.Deserializer");
    let obj = object_ptr(obj_value).unwrap_or(std::ptr::null_mut());
    store_instance_buffer(obj, bytes);
    obj_value
}

#[no_mangle]
pub extern "C" fn js_v8_default_deserializer_new(buffer: f64) -> f64 {
    let Some(bytes) = buffer_like_bytes(buffer) else {
        throw_invalid_buffer_arg();
    };
    let obj_value = namespace("v8.DefaultDeserializer");
    let obj = object_ptr(obj_value).unwrap_or(std::ptr::null_mut());
    store_instance_buffer(obj, bytes);
    obj_value
}

fn instance(handle: i64) -> *mut ObjectHeader {
    let bits = handle as u64;
    let raw = if (bits >> 48) >= 0x7FF8 {
        bits & POINTER_MASK
    } else {
        bits
    };
    raw as *mut ObjectHeader
}

fn instance_buffer(obj: *const ObjectHeader) -> Option<Vec<u8>> {
    if obj.is_null() {
        return None;
    }
    INSTANCE_BUFFERS.with(|buffers| buffers.borrow().get(&(obj as usize)).cloned())
}

fn store_instance_buffer(obj: *mut ObjectHeader, bytes: Vec<u8>) {
    if !obj.is_null() {
        INSTANCE_BUFFERS.with(|buffers| {
            buffers.borrow_mut().insert(obj as usize, bytes);
        });
    }
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_header(_handle: i64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_value(handle: i64, value: f64) -> f64 {
    if !is_cloneable(value, 0) {
        throw_data_clone_error();
    }
    let bytes = crate::child_process::v8_serde::v8_serialize(value);
    store_instance_buffer(instance(handle), bytes);
    bool_value(true)
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_release_buffer(handle: i64) -> f64 {
    let bytes = instance_buffer(instance(handle))
        .unwrap_or_else(|| crate::child_process::v8_serde::v8_serialize(undefined()));
    buffer_value(&bytes)
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_transfer_array_buffer(
    _handle: i64,
    _id: f64,
    _buffer: f64,
) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_uint32(_handle: i64, _value: f64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_uint64(_handle: i64, _hi: f64, _lo: f64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_double(_handle: i64, _value: f64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_write_raw_bytes(_handle: i64, _buffer: f64) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_get_data_clone_error(_handle: i64, message: f64) -> f64 {
    let msg = crate::builtins::js_string_coerce(message);
    let err = crate::error::js_error_new_with_message(msg);
    crate::value::js_nanbox_pointer(err as i64)
}

#[no_mangle]
pub extern "C" fn js_v8_serializer_set_treat_array_buffer_views_as_host_objects(
    _handle: i64,
    _flag: f64,
) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_header(_handle: i64) -> f64 {
    bool_value(true)
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_value(handle: i64) -> f64 {
    let Some(bytes) = instance_buffer(instance(handle)) else {
        return undefined();
    };
    crate::child_process::v8_serde::v8_deserialize(&bytes)
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_transfer_array_buffer(
    _handle: i64,
    _id: f64,
    _buffer: f64,
) -> f64 {
    undefined()
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_get_wire_format_version(_handle: i64) -> f64 {
    V8_WIRE_FORMAT_VERSION
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_uint32(_handle: i64) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_uint64(_handle: i64) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_double(_handle: i64) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn js_v8_deserializer_read_raw_bytes(_handle: i64, _length: f64) -> f64 {
    buffer_value(&[])
}
