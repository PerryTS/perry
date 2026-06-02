//! `util.inherits(constructor, superConstructor)`.

use crate::closure::ClosureHeader;
use crate::object::{ObjectHeader, PropertyAttrs};
use crate::value::JSValue;

const TAG_UNDEFINED_F64: f64 = f64::from_bits(crate::value::TAG_UNDEFINED);

fn named_key(name: &[u8]) -> *const crate::string::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let received = crate::fs::validate::describe_received(value);
    let message =
        format!("The \"{name}\" argument must be of type {expected}. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_super_prototype(value: f64) -> ! {
    let received = crate::fs::validate::describe_received(value);
    let message =
        format!("The \"superCtor.prototype\" property must be of type object. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn is_nullish(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    jv.is_null() || jv.is_undefined()
}

fn closure_ptr(value: f64) -> usize {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return 0;
    }
    let ptr = jv.as_pointer::<ClosureHeader>() as usize;
    if ptr >= 0x1000 && crate::closure::is_closure_ptr(ptr) {
        ptr
    } else {
        0
    }
}

fn object_ptr(value: f64) -> *mut ObjectHeader {
    let jv = JSValue::from_bits(value.to_bits());
    let ptr = if jv.is_pointer() {
        jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        let bits = value.to_bits();
        if bits != 0 && bits <= crate::value::POINTER_MASK && bits > 0x10000 {
            bits as *mut ObjectHeader
        } else {
            std::ptr::null_mut()
        }
    };
    if ptr.is_null() || crate::closure::is_closure_ptr(ptr as usize) {
        return std::ptr::null_mut();
    }
    if crate::object::is_valid_obj_ptr(ptr as *const u8) {
        ptr
    } else {
        std::ptr::null_mut()
    }
}

fn get_property(value: f64, name: &[u8]) -> f64 {
    unsafe { crate::value::js_get_property(value, name.as_ptr() as i64, name.len() as i64) }
}

fn ensure_function_prototype(value: f64) -> f64 {
    let current = get_property(value, b"prototype");
    if current.to_bits() != crate::value::TAG_UNDEFINED {
        return current;
    }
    if closure_ptr(value) == 0 {
        return current;
    }
    let class_id = crate::object::synthetic_class_id_for_function(value);
    if class_id == 0 {
        return current;
    }
    let proto = crate::object::ensure_function_prototype_object(value, class_id);
    if proto.is_null() {
        current
    } else {
        crate::value::js_nanbox_pointer(proto as i64)
    }
}

fn set_super_property(ctor: f64, super_ctor: f64) {
    let key = named_key(b"super_");
    let attrs = PropertyAttrs::new(true, false, true);
    let ctor_closure = closure_ptr(ctor);
    if ctor_closure != 0 {
        crate::closure::closure_set_dynamic_prop(ctor_closure, "super_", super_ctor);
        crate::object::set_property_attrs(ctor_closure, "super_".to_string(), attrs);
        return;
    }

    let obj = object_ptr(ctor);
    if obj.is_null() {
        crate::object::throw_object_type_error(b"Object.defineProperty called on non-object");
    }
    crate::object::js_object_set_field_by_name(obj, key, super_ctor);
    crate::object::set_property_attrs(obj as usize, "super_".to_string(), attrs);
}

fn register_function_parent(ctor: f64, super_ctor: f64) {
    if closure_ptr(ctor) == 0 || closure_ptr(super_ctor) == 0 {
        return;
    }
    let ctor_class = crate::object::synthetic_class_id_for_function(ctor);
    let super_class = crate::object::synthetic_class_id_for_function(super_ctor);
    if ctor_class != 0 && super_class != 0 && ctor_class != super_class {
        crate::object::register_class(ctor_class, super_class);
    }
}

/// `util.inherits(constructor, superConstructor)` -> undefined.
#[no_mangle]
pub extern "C" fn js_util_inherits(ctor: f64, super_ctor: f64) -> f64 {
    if is_nullish(ctor) {
        invalid_arg_type("ctor", "function", ctor);
    }
    if is_nullish(super_ctor) {
        invalid_arg_type("superCtor", "function", super_ctor);
    }

    let super_proto = ensure_function_prototype(super_ctor);
    if JSValue::from_bits(super_proto.to_bits()).is_undefined() {
        invalid_super_prototype(super_proto);
    }

    let ctor_proto = ensure_function_prototype(ctor);
    set_super_property(ctor, super_ctor);
    crate::object::js_object_set_prototype_of(ctor_proto, super_proto);
    register_function_parent(ctor, super_ctor);

    TAG_UNDEFINED_F64
}
