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
    let closure = (constructor_value.to_bits() & crate::value::POINTER_MASK) as usize;
    for name in [
        "_isArrayBufferView",
        "_isUint8Array",
        "_uint8ArrayToBuffer",
        "isDestroyed",
    ] {
        crate::closure::closure_set_dynamic_prop(
            closure,
            name,
            bound_native_callable_export_value("stream", name),
        );
    }
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

pub(crate) unsafe fn dispatch_stream_native_module_method(
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
    };
    let pack_args = || -> *mut crate::array::ArrayHeader {
        let mut arr = crate::array::js_array_alloc(args_len as u32);
        for i in 0..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };

    Some(match method_name {
        "compose" => crate::node_stream::js_node_stream_compose_args(pack_args()),
        "duplexPair" => crate::node_stream::js_node_stream_duplex_pair(arg(0)),
        "pipeline" => crate::node_stream::js_node_stream_pipeline(pack_args()),
        "finished" => crate::node_stream::js_node_stream_finished(pack_args()),
        "isDisturbed" => crate::node_stream::js_node_stream_is_disturbed(arg(0)),
        "isErrored" => crate::node_stream::js_node_stream_is_errored(arg(0)),
        "isReadable" => crate::node_stream::js_node_stream_is_readable(arg(0)),
        "isWritable" => crate::node_stream::js_node_stream_is_writable(arg(0)),
        "getDefaultHighWaterMark" => crate::node_stream::js_node_stream_get_default_hwm(arg(0)),
        "setDefaultHighWaterMark" => {
            crate::node_stream::js_node_stream_set_default_hwm(arg(0), arg(1))
        }
        "addAbortSignal" => crate::node_stream::js_node_stream_add_abort_signal(arg(0), arg(1)),
        "_isArrayBufferView" => crate::node_stream::js_node_stream_is_array_buffer_view(arg(0)),
        "_isUint8Array" => crate::node_stream::js_node_stream_is_uint8_array(arg(0)),
        "_uint8ArrayToBuffer" => crate::node_stream::js_node_stream_uint8_array_to_buffer(arg(0)),
        "isDestroyed" => crate::node_stream::js_node_stream_is_destroyed(arg(0)),
        "Readable" => crate::node_stream::js_node_stream_readable_new(arg(0)),
        "Writable" => crate::node_stream::js_node_stream_writable_new(arg(0)),
        "Duplex" => crate::node_stream::js_node_stream_duplex_new(arg(0)),
        "Transform" => crate::node_stream::js_node_stream_transform_new(arg(0)),
        "PassThrough" => crate::node_stream::js_node_stream_passthrough_new(arg(0)),
        _ => return None,
    })
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
