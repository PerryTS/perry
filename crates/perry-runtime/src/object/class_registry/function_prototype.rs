use super::construct::{is_arrow_function_value, is_callable_function_value};
use super::*;

/// Does this function have an own `.prototype` slot? This intentionally does
/// not materialize the prototype, so `hasOwnProperty` cannot freeze its attrs.
pub(crate) fn function_would_have_own_prototype(func_value: f64) -> bool {
    if !is_callable_function_value(func_value)
        || is_arrow_function_value(func_value)
        || is_plain_async_function_value(func_value)
        || super::super::native_module::builtin_closure_is_non_constructable_value(func_value)
    {
        return false;
    }
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
        if crate::closure::closure_is_bound_method(ptr)
            && !super::super::native_module::bound_native_callable_is_constructor_value(func_value)
        {
            return false;
        }
    }
    synthetic_class_id_for_function(func_value) != 0
}

pub(crate) fn ordinary_function_prototype_value_for_read(func_value: f64) -> Option<f64> {
    if !is_callable_function_value(func_value)
        || is_arrow_function_value(func_value)
        || is_plain_async_function_value(func_value)
    {
        return None;
    }
    // Bound native class exports are constructors; ordinary bound methods are
    // not. A stable synthetic class gives constructor exports a prototype.
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if jv.is_pointer() {
        let cptr = jv.as_pointer::<crate::closure::ClosureHeader>();
        if !cptr.is_null()
            && crate::value::addr_class::is_plausible_heap_addr(cptr as usize)
            && crate::closure::closure_is_bound_method(cptr)
        {
            if super::super::native_module::builtin_closure_is_non_constructable_value(func_value) {
                return None;
            }
            let is_native_class_export =
                super::super::native_module::bound_native_callable_is_constructor_value(func_value);
            if !is_native_class_export {
                return None;
            }
        }
    }
    if super::super::native_module::builtin_closure_is_non_constructable_value(func_value) {
        return None;
    }
    let cid = synthetic_class_id_for_function(func_value);
    if cid == 0 {
        return None;
    }
    let proto = ensure_function_prototype_object(func_value, cid);
    (!proto.is_null()).then(|| crate::value::js_nanbox_pointer(proto as i64))
}

fn is_plain_async_function_value(func_value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
    if ptr.is_null() || !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return false;
    }
    let fp = crate::closure::get_valid_func_ptr(ptr);
    !fp.is_null()
        && crate::closure::is_registered_async_function(fp)
        && !crate::closure::is_registered_generator_function(fp)
}

#[no_mangle]
pub extern "C" fn js_function_prototype_value_for_read(func_value: f64) -> f64 {
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let jv = crate::value::JSValue::from_bits(func_value.to_bits());
    if !jv.is_pointer() {
        return undef;
    }
    let ptr = jv.as_pointer() as *const crate::closure::ClosureHeader;
    if ptr.is_null() || !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return undef;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return undef;
        }
    }
    let closure_addr = ptr as usize;
    if crate::closure::closure_is_key_deleted(closure_addr, "prototype") {
        return undef;
    }
    let dynamic = crate::closure::closure_get_dynamic_prop(closure_addr, "prototype");
    if dynamic.to_bits() != crate::value::TAG_UNDEFINED {
        return dynamic;
    }
    if let Some(proto) = generator_function_prototype_of(closure_addr) {
        return proto;
    }
    ordinary_function_prototype_value_for_read(func_value).unwrap_or(undef)
}
