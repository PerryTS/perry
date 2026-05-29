#[inline]
fn is_weak_collection_value(value: f64) -> bool {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<u8>();
    let addr = ptr as usize;
    if addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        let obj = ptr as *const crate::object::ObjectHeader;
        matches!(
            (*obj).class_id,
            crate::weakref::CLASS_ID_WEAKMAP | crate::weakref::CLASS_ID_WEAKSET
        )
    }
}

#[inline]
pub(super) fn is_identity_only_deep_equal_value(value: f64) -> bool {
    crate::promise::js_value_is_promise(value) != 0 || is_weak_collection_value(value)
}
