//! Stream-specific native-module constructor metadata.

use super::*;

thread_local! {
    static STREAM_EVENT_EMITTER_PROTOTYPES: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn scan_stream_event_emitter_prototype_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        for bits in protos.iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

pub(crate) fn attach_stream_legacy_prototype(constructor_value: f64) {
    let proto = js_object_alloc_with_shape(
        0x7FFF_FF33,
        1,
        b"constructor\0".as_ptr(),
        b"constructor\0".len() as u32,
    );
    js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        if !protos.contains(&proto_value.to_bits()) {
            protos.push(proto_value.to_bits());
        }
    });
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        proto_value,
    );
}

pub(crate) fn attach_stream_constructor_prototype(constructor_value: f64, name: &str) {
    let shape_id = match name {
        "Readable" => 0x7FFF_FF34,
        "Writable" => 0x7FFF_FF35,
        "Duplex" => 0x7FFF_FF36,
        "Transform" => 0x7FFF_FF37,
        "PassThrough" => 0x7FFF_FF38,
        _ => return,
    };
    let proto = js_object_alloc_with_shape(
        shape_id,
        1,
        b"constructor\0".as_ptr(),
        b"constructor\0".len() as u32,
    );
    js_object_set_field(proto, 0, JSValue::from_bits(constructor_value.to_bits()));
    let proto_value = crate::value::js_nanbox_pointer(proto as i64);
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| {
        let mut protos = protos.borrow_mut();
        if !protos.contains(&proto_value.to_bits()) {
            protos.push(proto_value.to_bits());
        }
    });
    crate::closure::closure_set_dynamic_prop(
        (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize,
        "prototype",
        proto_value,
    );
}

pub(crate) fn is_stream_event_emitter_prototype_value(value: f64) -> bool {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if !jsval.is_pointer() {
        return false;
    }
    STREAM_EVENT_EMITTER_PROTOTYPES.with(|protos| protos.borrow().contains(&bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_stream_prototype_is_event_emitter_instanceof_candidate() {
        let stream_ctor = bound_native_callable_export_value("stream", "Stream");
        let stream_ptr = (stream_ctor.to_bits() & crate::value::POINTER_MASK) as usize;
        let stream_proto = crate::closure::closure_get_dynamic_prop(stream_ptr, "prototype");
        assert!(is_stream_event_emitter_prototype_value(stream_proto));

        let event_emitter = bound_native_callable_export_value("events", "EventEmitter");
        assert_eq!(
            js_instanceof_dynamic(stream_proto, event_emitter).to_bits(),
            crate::value::TAG_TRUE,
        );
        assert_eq!(
            js_instanceof(stream_proto, 0xFFFF0076).to_bits(),
            crate::value::TAG_TRUE,
        );
    }
}
