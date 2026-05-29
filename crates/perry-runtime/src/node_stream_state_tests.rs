//! State/introspection tests for [`super`] (`node_stream.rs`). Kept separate
//! from the helper-heavy stream tests so each file stays under the CI size gate.

use super::*;

#[test]
fn fresh_streams_expose_destroyed_false() {
    let streams = [
        js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED)),
        js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED)),
        js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED)),
        js_node_stream_transform_new(f64::from_bits(TAG_UNDEFINED)),
    ];

    for stream in streams {
        let destroyed = js_object_get_field_by_name_f64(
            raw_ptr_from_value(stream) as *const ObjectHeader,
            hidden_key(b"destroyed"),
        );
        assert_eq!(destroyed.to_bits(), TAG_FALSE);
        assert_eq!(
            js_node_stream_method_destroyed(raw_ptr_from_value(stream) as i64).to_bits(),
            TAG_FALSE
        );
    }
}

#[test]
fn readable_lifecycle_flags_reflect_ended_state() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    test_install_manual_read(stream);
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;

    assert_eq!(js_node_stream_method_readable(handle).to_bits(), TAG_TRUE);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readable")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_readable_ended(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableEnded")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_readable_did_read(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableDidRead")).to_bits(),
        TAG_FALSE
    );

    let _ = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        js_node_stream_method_readable_did_read(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableDidRead")).to_bits(),
        TAG_TRUE
    );

    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));
    assert_eq!(js_node_stream_method_readable(handle).to_bits(), TAG_FALSE);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readable")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_readable_ended(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableEnded")).to_bits(),
        TAG_TRUE
    );
}

#[test]
fn stream_object_mode_flags_default_false_and_follow_options() {
    let default_transform = js_node_stream_transform_new(f64::from_bits(TAG_UNDEFINED));
    let default_obj = raw_ptr_from_value(default_transform) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(default_obj, hidden_key(b"readableObjectMode")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(default_obj, hidden_key(b"writableObjectMode")).to_bits(),
        TAG_FALSE
    );

    let opts = crate::object::js_object_alloc(0, 2);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"readableObjectMode"),
        f64::from_bits(TAG_TRUE),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"writableObjectMode"),
        f64::from_bits(TAG_FALSE),
    );
    let duplex = js_node_stream_duplex_new(box_pointer(opts as *const u8));
    let duplex_obj = raw_ptr_from_value(duplex) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(duplex_obj, hidden_key(b"readableObjectMode")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(duplex_obj, hidden_key(b"writableObjectMode")).to_bits(),
        TAG_FALSE
    );
}

#[test]
fn stream_dynamic_instanceof_follows_node_stream_inheritance() {
    let readable = crate::object::bound_native_callable_export_value("stream", "Readable");
    let writable = crate::object::bound_native_callable_export_value("stream", "Writable");
    let duplex = crate::object::bound_native_callable_export_value("stream", "Duplex");
    let transform_ctor = crate::object::bound_native_callable_export_value("stream", "Transform");
    let passthrough_ctor =
        crate::object::bound_native_callable_export_value("stream", "PassThrough");
    let stream_ctor = crate::object::bound_native_callable_export_value("stream", "Stream");
    let event_emitter_ctor =
        crate::object::bound_native_callable_export_value("events", "EventEmitter");

    let transform = js_node_stream_transform_new(f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, transform_ctor).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof(transform, 0xFFFF0074).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, duplex).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, readable).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, writable).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, stream_ctor).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, event_emitter_ctor).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof(transform, 0xFFFF0076).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(transform, passthrough_ctor).to_bits(),
        TAG_FALSE
    );

    let passthrough = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        crate::object::js_instanceof_dynamic(passthrough, passthrough_ctor).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        crate::object::js_instanceof_dynamic(passthrough, transform_ctor).to_bits(),
        TAG_TRUE
    );
}
