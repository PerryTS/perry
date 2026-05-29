//! `node:util/types` predicate runtime entry points
//! (`util.types.isPromise`, `isMap`, `isDate`, `isRegExp`, etc.).
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

#[inline]
fn nanbox_bool(v: bool) -> f64 {
    f64::from_bits(
        if v {
            JSValue::bool(true)
        } else {
            JSValue::bool(false)
        }
        .bits(),
    )
}

#[inline]
fn jsvalue_addr(v: f64) -> usize {
    let bits = v.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

#[inline]
fn jsvalue_typed_array_kind(v: f64) -> Option<u8> {
    let addr = jsvalue_addr(v);
    if crate::buffer::is_uint8array_buffer(addr) {
        Some(crate::typedarray::KIND_UINT8)
    } else {
        crate::typedarray::lookup_typed_array_kind(addr)
    }
}

#[inline]
fn object_class_id(value: f64) -> Option<u32> {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let ptr = v.as_pointer::<ObjectHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    Some(unsafe { (*ptr).class_id })
}

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_0060;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_0061;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_0062;

#[inline]
fn closure_func_ptr(value: f64) -> Option<*const u8> {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return None;
    }
    let ptr = crate::closure::clean_closure_ptr(v.as_pointer::<crate::closure::ClosureHeader>());
    let func_ptr = crate::closure::get_valid_func_ptr(ptr);
    if func_ptr.is_null() {
        None
    } else {
        Some(func_ptr)
    }
}

#[inline]
fn closure_kind(value: f64) -> Option<u32> {
    closure_func_ptr(value).and_then(crate::closure::lookup_closure_function_kind)
}

#[inline]
unsafe fn object_own_field_by_bytes(obj: *const ObjectHeader, key: &[u8]) -> Option<JSValue> {
    if obj.is_null() || (obj as usize) < 0x10000 {
        return None;
    }
    let keys = (*obj).keys_array;
    let keys_ptr = keys as usize;
    if keys.is_null() || (keys_ptr as u64) >> 48 != 0 || keys_ptr < 0x10000 {
        return None;
    }
    let keys_gc = (keys as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*keys_gc).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let key_count = crate::array::js_array_length(keys) as usize;
    if key_count > 65536 {
        return None;
    }
    let alloc_limit = std::cmp::max((*obj).field_count, 8) as usize;
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(keys, i as u32);
        if crate::string::js_string_key_matches_bytes(key_val, key) && i < alloc_limit {
            return Some(js_object_get_field(obj, i as u32));
        }
    }
    None
}

#[inline]
fn is_closure_jsvalue(value: JSValue) -> bool {
    if !value.is_pointer() {
        return false;
    }
    let ptr =
        crate::closure::clean_closure_ptr(value.as_pointer::<crate::closure::ClosureHeader>());
    !crate::closure::get_valid_func_ptr(ptr).is_null()
}

#[no_mangle]
pub extern "C" fn js_util_types_is_number_object(value: f64) -> f64 {
    nanbox_bool(object_class_id(value) == Some(CLASS_ID_BOXED_NUMBER))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_string_object(value: f64) -> f64 {
    nanbox_bool(object_class_id(value) == Some(CLASS_ID_BOXED_STRING))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_boolean_object(value: f64) -> f64 {
    nanbox_bool(object_class_id(value) == Some(CLASS_ID_BOXED_BOOLEAN))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_boxed_primitive(value: f64) -> f64 {
    nanbox_bool(matches!(
        object_class_id(value),
        Some(CLASS_ID_BOXED_NUMBER | CLASS_ID_BOXED_STRING | CLASS_ID_BOXED_BOOLEAN)
    ))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_promise(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    nanbox_bool(
        v.is_pointer()
            && unsafe {
                crate::promise::js_is_promise(
                    v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                ) != 0
            },
    )
}

#[no_mangle]
pub extern "C" fn js_util_types_is_async_function(value: f64) -> f64 {
    nanbox_bool(closure_kind(value) == Some(crate::closure::CLOSURE_FUNCTION_KIND_ASYNC))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_generator_function(value: f64) -> f64 {
    nanbox_bool(closure_kind(value) == Some(crate::closure::CLOSURE_FUNCTION_KIND_GENERATOR))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_generator_object(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() || is_closure_jsvalue(v) {
        return nanbox_bool(false);
    }
    let obj = v.as_pointer::<ObjectHeader>();
    let has_generator_methods = unsafe {
        [
            b"next".as_slice(),
            b"return".as_slice(),
            b"throw".as_slice(),
        ]
        .iter()
        .all(|key| object_own_field_by_bytes(obj, key).is_some_and(is_closure_jsvalue))
    };
    nanbox_bool(has_generator_methods)
}

#[no_mangle]
pub extern "C" fn js_util_types_is_native_error(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return nanbox_bool(false);
    }
    let ptr = v.as_pointer::<crate::error::ErrorHeader>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return nanbox_bool(false);
    }
    let header =
        unsafe { (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader };
    nanbox_bool(unsafe { (*header).obj_type == crate::gc::GC_TYPE_ERROR })
}

#[no_mangle]
pub extern "C" fn js_util_types_is_array_buffer(value: f64) -> f64 {
    nanbox_bool(crate::buffer::is_array_buffer(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_shared_array_buffer(value: f64) -> f64 {
    nanbox_bool(crate::buffer::is_shared_array_buffer(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_any_array_buffer(value: f64) -> f64 {
    nanbox_bool(crate::buffer::is_any_array_buffer(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_array_buffer_view(value: f64) -> f64 {
    let addr = jsvalue_addr(value);
    nanbox_bool(
        crate::buffer::is_uint8array_buffer(addr)
            || crate::buffer::is_data_view(addr)
            || jsvalue_typed_array_kind(value).is_some(),
    )
}

#[no_mangle]
pub extern "C" fn js_util_types_is_typed_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value).is_some())
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint8_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT8))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_int8_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_INT8))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_int16_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_INT16))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint16_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT16))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_int32_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_INT32))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint32_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT32))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_float32_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_FLOAT32))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_float64_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_FLOAT64))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_uint8_clamped_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT8_CLAMPED))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_map(value: f64) -> f64 {
    nanbox_bool(crate::map::is_registered_map(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_set(value: f64) -> f64 {
    nanbox_bool(crate::set::is_registered_set(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_date(value: f64) -> f64 {
    nanbox_bool(crate::date::is_date_value(value))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_reg_exp(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    nanbox_bool(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_proxy(value: f64) -> f64 {
    nanbox_bool(crate::proxy::js_proxy_is_proxy(value) != 0)
}

#[no_mangle]
pub extern "C" fn js_util_types_is_map_iterator(value: f64) -> f64 {
    nanbox_bool(crate::map::is_registered_map_iterator(jsvalue_addr(value)))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_set_iterator(value: f64) -> f64 {
    nanbox_bool(crate::set::is_registered_set_iterator(jsvalue_addr(value)))
}
