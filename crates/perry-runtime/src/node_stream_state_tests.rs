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
fn passthrough_subclass_uses_override_or_identity_transform() {
    crate::closure::js_register_closure_arity(super::tests::noop_listener as *const u8, 0);
    let callback =
        box_pointer(js_closure_alloc(super::tests::noop_listener as *const u8, 0) as *const u8);

    let overridden_obj = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(overridden_obj, hidden_key(b"_transform"), callback);
    let overridden = js_node_stream_passthrough_subclass_init(
        box_pointer(overridden_obj as *const u8),
        f64::from_bits(TAG_UNDEFINED),
    );
    assert_eq!(
        transform_hidden_callback(overridden).map(f64::to_bits),
        Some(callback.to_bits())
    );
    assert!(!has_truthy_hidden(
        overridden,
        hidden_transform_passthrough_key()
    ));

    let default_obj = crate::object::js_object_alloc(0, 0);
    let default = js_node_stream_passthrough_subclass_init(
        box_pointer(default_obj as *const u8),
        f64::from_bits(TAG_UNDEFINED),
    );
    assert!(transform_hidden_callback(default).is_none());
    assert!(has_truthy_hidden(
        default,
        hidden_transform_passthrough_key()
    ));
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

/// #11197: `_readableState` / `_writableState` are live views over the
/// stream's own hidden state, not snapshots.
#[test]
fn readable_state_view_reads_live_stream_state() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    test_install_manual_read(stream);
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let view = js_object_get_field_by_name_f64(obj, hidden_key(b"_readableState"));
    let view_obj = raw_ptr_from_value(view) as *const ObjectHeader;
    assert!(!view_obj.is_null());
    // Same view on every read.
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"_readableState")).to_bits(),
        view.to_bits()
    );
    let read = |name: &[u8]| js_object_get_field_by_name_f64(view_obj, hidden_key(name));
    assert_eq!(read(b"highWaterMark"), 65536.0);
    assert_eq!(read(b"length"), 0.0);
    assert_eq!(read(b"ended").to_bits(), TAG_FALSE);
    assert_eq!(read(b"flowing").to_bits(), TAG_NULL);

    push_chunk(stream, string_value(b"abc"));
    assert_eq!(read(b"length"), 3.0);
    push_chunk(stream, f64::from_bits(TAG_NULL));
    assert_eq!(read(b"ended").to_bits(), TAG_TRUE);
    assert_eq!(read(b"endEmitted").to_bits(), TAG_FALSE);

    // `dataEmitted` follows the stream's own flag, which emitting `'data'`
    // sets (the JS-level setter path is covered by
    // test-files/test_gap_stream_readable_state.ts).
    assert_eq!(read(b"dataEmitted").to_bits(), TAG_FALSE);
    note_data_emitted(stream);
    assert_eq!(read(b"dataEmitted").to_bits(), TAG_TRUE);

    let mut json = String::new();
    unsafe {
        assert!(try_stringify_node_stream_json(
            view_obj as *const u8,
            &mut json
        ));
    }
    assert_eq!(
        json,
        r#"{"highWaterMark":65536,"buffer":[],"bufferIndex":0,"length":3,"pipes":[],"awaitDrainWriters":null}"#
    );
}

#[test]
fn writable_state_view_reads_live_stream_state() {
    let stream = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let view = js_object_get_field_by_name_f64(obj, hidden_key(b"_writableState"));
    let view_obj = raw_ptr_from_value(view) as *const ObjectHeader;
    assert!(!view_obj.is_null());
    let read = |name: &[u8]| js_object_get_field_by_name_f64(view_obj, hidden_key(name));
    assert_eq!(read(b"highWaterMark"), 65536.0);
    assert_eq!(read(b"ended").to_bits(), TAG_FALSE);
    mark_writable_ended(stream);
    assert_eq!(read(b"ended").to_bits(), TAG_TRUE);
    assert!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"_readableState")).to_bits()
            == TAG_UNDEFINED
    );
}
