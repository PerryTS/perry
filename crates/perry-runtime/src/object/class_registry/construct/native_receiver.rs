use super::*;

pub(super) unsafe fn finish_native_object(
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
                proto.get_nanbox_u64(),
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
                proto.get_nanbox_u64(),
            );
        }
    }
}

/// `Reflect.construct(F, args, newTarget)` for a WHATWG Fetch base `F` whose
/// constructor returned a registry handle (`result`), when
/// `newTarget.prototype` is not `F.prototype`. A bare handle's [[Prototype]]
/// is always its interface's, so the instance is instead an ordinary object
/// born from `newTarget` that owns `proto` and keeps the handle in the private
/// element a `class X extends F` instance uses, which also roots it. Reads,
/// methods and brand checks reach the native state through that element.
/// `None` leaves `result` as the instance.
pub(super) unsafe fn fetch_backed_object(
    scope: &crate::gc::RuntimeHandleScope,
    result: f64,
    nt: &crate::gc::RuntimeHandle<'_>,
    proto: Option<crate::gc::RuntimeHandle<'_>>,
) -> Option<f64> {
    let proto = proto?;
    let value = JSValue::from_bits(result.to_bits());
    if !value.is_handle() {
        return None;
    }
    let probe = crate::object::fetch_handle_kind_probe()?;
    let name = match probe(value.as_handle()) {
        1 => "Response",
        2 => "Request",
        3 => "Headers",
        4 => "Blob",
        5 => "File",
        _ => return None,
    };
    let handle = scope.root_nanbox_f64(result);
    if crate::object::builtin_prototype_value(name).to_bits() == proto.get_nanbox_u64() {
        return None;
    }
    let receiver = scope.root_nanbox_f64(ordinary_object_from_new_target(
        nt.get_nanbox_f64(),
        Some(proto.get_nanbox_u64()),
    ));
    crate::object::global_this::attach_fetch_handle_to_this(
        receiver.get_nanbox_f64(),
        handle.get_nanbox_f64(),
    );
    Some(receiver.get_nanbox_f64())
}
