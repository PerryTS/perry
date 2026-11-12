use super::*;

pub(crate) fn constructor_dynamic_prototype(obj: *const ObjectHeader) -> Option<f64> {
    if obj.is_null() {
        return None;
    }
    let key = crate::string::canonical_key(b"constructor");
    let constructor = js_object_get_field_by_name_f64(obj, key);
    let bits = constructor.to_bits();
    let top16 = bits >> 48;
    if top16 != 0x7FFD {
        return None;
    }
    let raw_addr = (bits & crate::value::POINTER_MASK) as usize;
    if raw_addr < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc = unsafe { gc_header_for(raw_addr as *const ObjectHeader) };
    if unsafe { (*gc).obj_type } != crate::gc::GC_TYPE_CLOSURE {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(raw_addr, "prototype");
    let proto_jsv = crate::value::JSValue::from_bits(proto.to_bits());
    if proto_jsv.is_undefined() {
        return None;
    }
    if proto_jsv.is_pointer() {
        let proto_addr = proto_jsv.as_pointer::<u8>() as usize;
        // Prototype objects commonly have `constructor` pointing back to the
        // constructor whose `.prototype` is this same object. That is not their
        // [[Prototype]]; let the caller fall back to Object.prototype/null.
        if proto_addr == obj as usize {
            return None;
        }
    }
    Some(proto)
}

pub(crate) fn error_kind_prototype_value(kind: u32) -> Option<f64> {
    use crate::object::BuiltinConstructorIdentity as B;
    // The realm's intrinsic `%<Kind>.prototype%`, never the writable
    // `globalThis.<Kind>` binding.
    let identity = match kind {
        crate::error::ERROR_KIND_TYPE_ERROR => B::TypeError,
        crate::error::ERROR_KIND_RANGE_ERROR => B::RangeError,
        crate::error::ERROR_KIND_REFERENCE_ERROR => B::ReferenceError,
        crate::error::ERROR_KIND_SYNTAX_ERROR => B::SyntaxError,
        crate::error::ERROR_KIND_EVAL_ERROR => B::EvalError,
        crate::error::ERROR_KIND_URI_ERROR => B::URIError,
        crate::error::ERROR_KIND_AGGREGATE_ERROR => B::AggregateError,
        _ => B::Error,
    };
    let addr = crate::realm_value_read! { table = GLOBAL_THIS_BUILTIN_CONSTRUCTORS; crate::array::intrinsic_prototype_addr(identity) };
    (addr != 0).then(|| crate::value::js_nanbox_pointer(addr as i64))
}
