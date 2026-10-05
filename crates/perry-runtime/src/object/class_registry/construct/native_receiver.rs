use super::*;

pub(super) unsafe fn finish_regexp(
    scope: &crate::gc::RuntimeHandleScope,
    result: f64,
    nt: &crate::gc::RuntimeHandle<'_>,
    proto: Option<crate::gc::RuntimeHandle<'_>>,
) -> f64 {
    let object =
        scope.root_raw_mut_ptr(crate::value::js_nanbox_get_pointer(result) as *mut ObjectHeader);
    object.with_mut_ptr::<ObjectHeader, _>(|object| {
        (*object).class_id = new_target_class_id(nt.get_nanbox_f64()).unwrap_or(0);
        crate::object::shapes::restamp_object_proto_id(object);
        crate::object::field_get_set::stamp_private_evaluation_brand(object, nt.get_nanbox_f64());
    });
    if let Some(proto) = proto {
        object.with_mut_ptr::<ObjectHeader, _>(|object| {
            crate::object::prototype_chain::object_set_static_prototype(
                object as usize,
                proto.get_heap_word_u64(),
            );
        });
    }
    return object.with_const_ptr::<ObjectHeader, _>(|object| {
        crate::value::js_nanbox_pointer(object as i64)
    });
}

pub(super) unsafe fn apply_prototype(result: f64, proto: Option<crate::gc::RuntimeHandle<'_>>) {
    if let Some(proto) = proto {
        let bits = result.to_bits();
        let addr = if (bits >> 48) == 0x7FFD {
            (bits & crate::value::POINTER_MASK) as usize
        } else if (bits >> 48) == 0
            && crate::buffer::buffer_family_type_owned(bits as usize).is_some()
        {
            // ArrayBuffer and SharedArrayBuffer are represented by a
            // raw BufferHeader pointer rather than a NaN-boxed object.
            bits as usize
        } else {
            0
        };
        if addr != 0 {
            crate::object::prototype_chain::object_set_static_prototype(
                addr,
                proto.get_heap_word_u64(),
            );
        }
    }
}
