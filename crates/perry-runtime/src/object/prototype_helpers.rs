use super::*;

pub(crate) fn constructor_dynamic_prototype(obj: *const ObjectHeader) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let key =
        crate::string::js_string_from_bytes(b"constructor".as_ptr(), b"constructor".len() as u32);
    let constructor = js_object_get_field_by_name_f64(obj, key);
    let bits = constructor.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        return None;
    }
    let raw_addr = (bits & crate::value::POINTER_MASK) as usize;
    if raw_addr < (crate::gc::GC_HEADER_SIZE as usize) + 0x1000 {
        return None;
    }
    let gc = unsafe { gc_header_for(raw_addr as *const ObjectHeader) };
    if unsafe { (*gc).obj_type } != crate::gc::GC_TYPE_CLOSURE {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(raw_addr, "prototype");
    let proto_jsv = crate::value::JSValue::from_bits(proto.to_bits());
    if proto_jsv.is_undefined() {
        None
    } else {
        Some(proto)
    }
}

pub(crate) fn error_kind_prototype_value(kind: u32) -> Option<f64> {
    let name = match kind {
        crate::error::ERROR_KIND_TYPE_ERROR => "TypeError",
        crate::error::ERROR_KIND_RANGE_ERROR => "RangeError",
        crate::error::ERROR_KIND_REFERENCE_ERROR => "ReferenceError",
        crate::error::ERROR_KIND_SYNTAX_ERROR => "SyntaxError",
        crate::error::ERROR_KIND_EVAL_ERROR => "EvalError",
        crate::error::ERROR_KIND_URI_ERROR => "URIError",
        crate::error::ERROR_KIND_AGGREGATE_ERROR => "AggregateError",
        _ => "Error",
    };
    let ctor = js_get_global_this_builtin_value(name.as_ptr(), name.len());
    let ctor_jsv = crate::value::JSValue::from_bits(ctor.to_bits());
    if !ctor_jsv.is_pointer() {
        return None;
    }
    let ctor_ptr = ctor_jsv.as_pointer::<crate::closure::ClosureHeader>() as usize;
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_jsv = crate::value::JSValue::from_bits(proto.to_bits());
    proto_jsv.is_pointer().then_some(proto)
}
