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
pub extern "C" fn js_util_types_is_array_buffer_view(value: f64) -> f64 {
    let addr = jsvalue_addr(value);
    nanbox_bool(
        crate::buffer::is_uint8array_buffer(addr) || jsvalue_typed_array_kind(value).is_some(),
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
pub extern "C" fn js_util_types_is_uint16_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_UINT16))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_int32_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_INT32))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_float64_array(value: f64) -> f64 {
    nanbox_bool(jsvalue_typed_array_kind(value) == Some(crate::typedarray::KIND_FLOAT64))
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
    nanbox_bool(crate::date::is_registered_date_bits(value.to_bits()))
}

#[no_mangle]
pub extern "C" fn js_util_types_is_reg_exp(value: f64) -> f64 {
    let v = JSValue::from_bits(value.to_bits());
    nanbox_bool(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
}
