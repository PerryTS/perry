use super::*;

thread_local! {
    static BOXED_PRIMITIVE_PAYLOADS: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, u64>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_0060;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_0061;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_0062;

pub(super) unsafe fn boxed_primitive_base_for_object(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<String> {
    let (class_id, payload) = boxed_primitive_payload_for_object(obj_ptr)?;
    match class_id {
        CLASS_ID_BOXED_STRING => {
            let s = jsvalue_string_content(payload).unwrap_or_default();
            Some(format!("[String: '{}']", escape_string(&s)))
        }
        CLASS_ID_BOXED_NUMBER => Some(format!("[Number: {}]", format_util_number(payload))),
        CLASS_ID_BOXED_BOOLEAN => {
            let value = crate::value::JSValue::from_bits(payload.to_bits());
            Some(format!(
                "[Boolean: {}]",
                if value.is_bool() && value.as_bool() {
                    "true"
                } else {
                    "false"
                }
            ))
        }
        _ => None,
    }
}

unsafe fn boxed_primitive_payload_for_object(
    obj_ptr: *const crate::object::ObjectHeader,
) -> Option<(u32, f64)> {
    if obj_ptr.is_null() {
        return None;
    }
    let class_id = (*obj_ptr).class_id;
    if !matches!(
        class_id,
        CLASS_ID_BOXED_NUMBER | CLASS_ID_BOXED_STRING | CLASS_ID_BOXED_BOOLEAN
    ) {
        return None;
    }
    let ptr_key = obj_ptr as usize;
    let payload = BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        m.borrow()
            .get(&ptr_key)
            .copied()
            .map(f64::from_bits)
            .unwrap_or_else(|| crate::object::js_object_get_field_f64(obj_ptr, 0))
    });
    Some((class_id, payload))
}

fn register_boxed_primitive_payload(obj: *mut crate::object::ObjectHeader, payload: f64) {
    if obj.is_null() {
        return;
    }
    BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        m.borrow_mut().insert(obj as usize, payload.to_bits());
    });
}

pub fn scan_boxed_primitive_payload_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut moved = Vec::new();
    BOXED_PRIMITIVE_PAYLOADS.with(|m| {
        let mut m = m.borrow_mut();
        for (&owner, payload_bits) in m.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            visitor.visit_nanbox_u64_slot(payload_bits);
        }
        for (old_owner, new_owner) in moved.drain(..) {
            if let Some(payload) = m.remove(&old_owner) {
                m.insert(new_owner, payload);
            }
        }
    });
}

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
    unsafe { boxed_primitive_payload_for_object(ptr) }
}

#[no_mangle]
pub extern "C" fn js_boxed_number_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_NUMBER, 0);
    // `new Number()` (no args) is spec'd to box +0, not NaN. js_number_coerce
    // would map undefined to NaN, so detect the missing-arg sentinel first.
    let payload = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        0.0
    } else {
        js_number_coerce(value)
    };
    register_boxed_primitive_payload(obj, payload);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_string_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_STRING, 0);
    // `new String()` (no args) is spec'd to box "", not "undefined".
    let ptr = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        crate::string::js_string_from_bytes(std::ptr::null(), 0)
    } else {
        js_string_coerce(value)
    };
    let boxed = f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits());
    register_boxed_primitive_payload(obj, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_boolean_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_BOOLEAN, 0);
    let boxed =
        f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value) != 0).bits());
    register_boxed_primitive_payload(obj, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}
