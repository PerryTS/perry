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

#[inline]
fn value_is_closure(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return false;
    }
    let ptr = v.as_pointer::<crate::closure::ClosureHeader>();
    !crate::closure::get_valid_func_ptr(ptr).is_null()
}

#[inline]
fn object_field_is_closure(obj: *const ObjectHeader, key: &[u8]) -> bool {
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    let value = crate::object::js_object_get_field_by_name_f64(obj, key_ptr);
    value_is_closure(value)
}

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_0060;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_0061;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_0062;

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
pub extern "C" fn js_util_types_is_generator_function(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return nanbox_bool(false);
    }
    let closure = v.as_pointer::<crate::closure::ClosureHeader>();
    let func_ptr = crate::closure::get_valid_func_ptr(closure);
    nanbox_bool(!func_ptr.is_null() && crate::closure::is_registered_generator_function(func_ptr))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_generator_object(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return nanbox_bool(false);
    }
    let obj = v.as_pointer::<ObjectHeader>();
    if obj.is_null() || crate::closure::is_closure_ptr(obj as usize) {
        return nanbox_bool(false);
    }
    if !crate::object::is_valid_obj_ptr(obj as *const u8) {
        return nanbox_bool(false);
    }
    // Perry lowers generator calls to a plain iterator-shaped object with
    // closure-valued own next/return/throw methods. Match that generated shape.
    nanbox_bool(
        object_field_is_closure(obj, b"next")
            && object_field_is_closure(obj, b"return")
            && object_field_is_closure(obj, b"throw"),
    )
}

#[no_mangle]
pub extern "C" fn js_util_types_is_native_error(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    if !v.is_pointer() {
        return nanbox_bool(false);
    }
    let ptr = v.as_pointer::<u8>();
    if ptr.is_null() || !crate::object::is_valid_obj_ptr(ptr) {
        return nanbox_bool(false);
    }
    let matches = unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        match (*gc_header).obj_type {
            crate::gc::GC_TYPE_ERROR => true,
            crate::gc::GC_TYPE_OBJECT => {
                let obj = ptr as *const ObjectHeader;
                let class_id = (*obj).class_id;
                class_id != 0 && crate::object::extends_builtin_error(class_id)
            }
            _ => false,
        }
    };
    nanbox_bool(matches)
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
