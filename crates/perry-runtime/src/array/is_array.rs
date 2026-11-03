//! Array.isArray.

/// Check if a value is an array (Array.isArray)
/// Returns a NaN-boxed TAG_TRUE/TAG_FALSE JS boolean per JS semantics.
#[no_mangle]
pub extern "C" fn js_array_is_array(value: f64) -> f64 {
    use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_TYPE_ARRAY};
    use crate::value::JSValue;

    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    let false_val = f64::from_bits(TAG_FALSE);
    let true_val = f64::from_bits(TAG_TRUE);

    // IsArray (ECMA-262 §7.2.2) recurses through a Proxy to its target — a
    // `Proxy([], {})` is an Array, a `Proxy(Proxy([], {}), {})` is too — and
    // throws a TypeError if the Proxy has been revoked. Unwrap before the
    // ordinary heap-pointer inspection below (proxy ids are small encoded
    // values that the pointer checks would otherwise reject as non-arrays).
    let mut value = value;
    while let Some(target) = crate::proxy_hooks::is_array_proxy_step(value) {
        value = target;
    }

    let bits = value.to_bits();
    let jsvalue = JSValue::from_bits(bits);

    // Get the raw pointer, handling both NaN-boxed and raw bitcast pointers
    let raw_ptr: *const u8 = if jsvalue.is_pointer() {
        jsvalue.as_pointer::<u8>()
    } else {
        // Check for raw bitcast pointer (no NaN-box tag, stored as f64 bits)
        let raw = bits;
        let upper = raw >> 48;
        if upper == 0 && (raw & 0x0000_FFFF_FFFF_FFFF) > 0x10000 {
            raw as *const u8
        } else {
            return false_val;
        }
    };

    if raw_ptr.is_null() {
        return false_val;
    }
    if crate::value::addr_class::is_handle_band(raw_ptr as usize) {
        return false_val;
    }

    // Check the GC header's obj_type. Both regular arrays and lazy
    // arrays (Phase 5 JSON.parse result) are arrays from the user's
    // perspective — `Array.isArray(JSON.parse("[...]"))` must return
    // true without forcing the lazy header to materialize.
    unsafe {
        let gc_header = raw_ptr.sub(GC_HEADER_SIZE) as *const GcHeader;
        let obj_type = (*gc_header).obj_type;
        if obj_type == GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            true_val
        } else if crate::object_has_meta_word!(obj_type) {
            // `class X extends Array` — the instance is a plain ObjectHeader, but
            // ECMA-262 IsArray must still report true for an Array subclass.
            if super::subclass::has_array_exotic_brand(
                raw_ptr as *const crate::object::ObjectHeader,
            ) {
                true_val
            } else {
                false_val
            }
        } else {
            false_val
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn array_identity_follows_construction_rather_than_constructor_ancestry() {
        let class_id = 0x0074_2784;
        crate::object::js_register_class_parent(class_id, 0xFFFF0024);
        let object = crate::object::js_object_alloc(class_id, 2);
        let object = crate::value::js_nanbox_pointer(object as i64);
        assert_eq!(
            super::js_array_is_array(object).to_bits(),
            crate::value::TAG_FALSE
        );
        crate::array::subclass_init::js_array_subclass_init(object, 0.0);
        assert_eq!(
            super::js_array_is_array(object).to_bits(),
            crate::value::TAG_TRUE
        );
        crate::object::js_object_set_prototype_of(object, f64::from_bits(crate::value::TAG_NULL));
        assert_eq!(
            super::js_array_is_array(object).to_bits(),
            crate::value::TAG_TRUE
        );
        let sibling = crate::object::js_object_alloc(class_id, 2);
        assert_eq!(
            super::js_array_is_array(crate::value::js_nanbox_pointer(sibling as i64)).to_bits(),
            crate::value::TAG_FALSE
        );
    }
}
