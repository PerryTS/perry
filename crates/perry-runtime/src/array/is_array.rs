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
    while let Some(target) = crate::proxy::is_array_proxy_step(value) {
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
        } else if obj_type == crate::gc::GC_TYPE_OBJECT {
            // `class X extends Array` — the instance is a plain ObjectHeader, but
            // ECMA-262 IsArray must still report true for an Array subclass.
            let class_id = crate::object::js_object_get_class_id(
                raw_ptr as *const crate::object::ObjectHeader,
            );
            if super::subclass::is_array_subclass_class_id(class_id) {
                true_val
            } else {
                false_val
            }
        } else {
            false_val
        }
    }
}
