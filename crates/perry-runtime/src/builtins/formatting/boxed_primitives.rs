use super::*;

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_0060;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_0061;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_0062;

#[inline]
pub(super) fn boxed_primitive_payload(value: f64) -> Option<(u32, f64)> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let class_id = (*ptr).class_id;
        if !matches!(
            class_id,
            CLASS_ID_BOXED_NUMBER | CLASS_ID_BOXED_STRING | CLASS_ID_BOXED_BOOLEAN
        ) {
            return None;
        }
        let payload = crate::object::js_object_get_field_f64(ptr, 0);
        Some((class_id, payload))
    }
}

#[no_mangle]
pub extern "C" fn js_boxed_number_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_NUMBER, 1);
    // `new Number()` (no args) is spec'd to box +0, not NaN. js_number_coerce
    // would map undefined to NaN, so detect the missing-arg sentinel first.
    let payload = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        0.0
    } else {
        js_number_coerce(value)
    };
    crate::object::js_object_set_field_f64(obj, 0, payload);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_string_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_STRING, 1);
    // `new String()` (no args) is spec'd to box "", not "undefined".
    let ptr = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        crate::string::js_string_from_bytes(std::ptr::null(), 0)
    } else {
        js_string_coerce(value)
    };
    let boxed = f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits());
    crate::object::js_object_set_field_f64(obj, 0, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_boolean_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_BOOLEAN, 1);
    let boxed =
        f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value) != 0).bits());
    crate::object::js_object_set_field_f64(obj, 0, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}
