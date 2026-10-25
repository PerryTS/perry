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

    // #11212: an empty `read()` and a producer-side `push()` do not count as
    // a read; a `read()` that returns data does.
    let _ = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    let _ = js_node_stream_method_push(handle, string_value(b"chunk"));
    assert_eq!(
        js_node_stream_method_readable_did_read(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(js_node_stream_is_disturbed(stream).to_bits(), TAG_FALSE);
    let _ = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        js_node_stream_method_readable_did_read(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(js_node_stream_is_disturbed(stream).to_bits(), TAG_TRUE);
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
    // `readableEnded` is "'end' was emitted"; pushing EOF only ends the buffer.
    assert_eq!(
        js_node_stream_method_readable_ended(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableEnded")).to_bits(),
        TAG_FALSE
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
    let callback = box_pointer(js_closure_alloc(
        crate::fn_info!(super::tests::noop_listener, 0; with_declared(0)),
        0,
    ) as *const u8);

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
    let read = |name: &'static [u8]| js_object_get_field_by_name_f64(view_obj, hidden_key(name));
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
    mark_disturbed(stream);
    assert_eq!(read(b"dataEmitted").to_bits(), TAG_TRUE);

    let json_value = unsafe { crate::json::js_json_stringify(view, 0) };
    let json = crate::fs::validate::read_js_string_pub(f64::from_bits(
        JSValue::string_ptr(json_value).bits(),
    ));
    assert_eq!(
        json,
        r#"{"highWaterMark":65536,"buffer":[{"type":"Buffer","data":[97,98,99]}],"bufferIndex":0,"length":3,"pipes":[],"awaitDrainWriters":null}"#
    );
}

#[test]
fn writable_state_view_reads_live_stream_state() {
    let stream = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let view = js_object_get_field_by_name_f64(obj, hidden_key(b"_writableState"));
    let view_obj = raw_ptr_from_value(view) as *const ObjectHeader;
    assert!(!view_obj.is_null());
    let read = |name: &'static [u8]| js_object_get_field_by_name_f64(view_obj, hidden_key(name));
    assert_eq!(read(b"highWaterMark"), 65536.0);
    assert_eq!(read(b"ended").to_bits(), TAG_FALSE);
    mark_writable_ended(stream);
    assert_eq!(read(b"ended").to_bits(), TAG_TRUE);
    assert!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"_readableState")).to_bits()
            == TAG_UNDEFINED
    );
}

#[test]
fn object_mode_reads_the_record() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    js_object_set_field_by_name(
        opts.get_raw_mut_ptr(),
        hidden_key(b"objectMode"),
        f64::from_bits(TAG_TRUE),
    );
    let stream = scope.root_nanbox_f64(js_node_stream_duplex_new(
        opts.with_mut_ptr(|p: *mut u8| box_pointer(p)),
    ));
    // These inherited getters are read-only. Define an own shadow instead
    // of assigning through them, while the private record remains true.
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let value_key = hidden_key(b"value");
    descriptor.with_mut_ptr(|ptr| {
        js_object_set_field_by_name(ptr, value_key, f64::from_bits(TAG_FALSE));
    });
    for key in [
        b"readableObjectMode".as_slice(),
        b"writableObjectMode".as_slice(),
    ] {
        crate::object::js_object_define_property(
            stream.get_nanbox_f64(),
            f64::from_bits(JSValue::string_ptr(hidden_key(key)).bits()),
            descriptor.with_mut_ptr(|p: *mut u8| box_pointer(p)),
        );
        assert_eq!(
            js_object_get_field_by_name_f64(
                raw_ptr_from_value(stream.get_nanbox_f64()) as *const ObjectHeader,
                hidden_key(key),
            )
            .to_bits(),
            TAG_FALSE,
            "the fixture must shadow the public getter"
        );
    }
    let handle = raw_ptr_from_value(stream.get_nanbox_f64()) as i64;
    assert_eq!(
        js_node_stream_method_readable_object_mode(handle).to_bits(),
        TAG_TRUE,
        "readable mode must come from record"
    );
    assert_eq!(
        js_node_stream_method_writable_object_mode(handle).to_bits(),
        TAG_TRUE,
        "writable mode must come from record"
    );
}
#[test]
fn by_name_mode_sabotage_reddens_record_witness() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "node_stream::state_tests::object_mode_reads_the_record",
            "--nocapture",
        ])
        .env("PERRY_TEST_STREAM_MODE_BY_NAME", "1")
        .output()
        .unwrap();
    assert!(!out.status.success(), "by-name mode sabotage must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("readable mode must come from record"));
}

extern "C" fn counted_object_mode(c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    let calls = js_closure_get_capture_f64(c, 0);
    js_closure_set_capture_f64(c as *mut ClosureHeader, 0, calls + 1.0);
    f64::from_bits(TAG_TRUE)
}
#[test]
fn default_high_water_mark_uses_the_resolved_mode() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let opts = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let getter =
        scope.root_raw_mut_ptr(js_closure_alloc(crate::fn_info!(counted_object_mode, 0), 1));
    js_closure_set_capture_f64(getter.get_raw_mut_ptr(), 0, 0.0);
    let descriptor = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let get_key = hidden_key(b"get");
    js_object_set_field_by_name(
        descriptor.get_raw_mut_ptr(),
        get_key,
        getter.with_mut_ptr(|p: *mut u8| box_pointer(p)),
    );
    crate::object::js_object_define_property(
        opts.with_mut_ptr(|p: *mut u8| box_pointer(p)),
        f64::from_bits(JSValue::string_ptr(hidden_key(b"objectMode")).bits()),
        descriptor.with_mut_ptr(|p: *mut u8| box_pointer(p)),
    );
    let stream = scope.root_nanbox_f64(js_node_stream_readable_new(
        opts.with_mut_ptr(|p: *mut u8| box_pointer(p)),
    ));
    assert_eq!(
        getter.with_const_ptr(|p| js_closure_get_capture_f64(p, 0)),
        1.0,
        "mode option must be read once"
    );
    assert_eq!(
        read_state_data(stream.get_nanbox_f64(), StateData::ReadableHwm),
        Some(16.0)
    );
}
#[test]
fn mode_reread_sabotage_reddens_resolved_mode_witness() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "node_stream::state_tests::default_high_water_mark_uses_the_resolved_mode",
            "--nocapture",
        ])
        .env("PERRY_TEST_STREAM_MODE_REREAD", "1")
        .output()
        .unwrap();
    assert!(!out.status.success(), "mode reread sabotage must fail");
    assert!(String::from_utf8_lossy(&out.stderr).contains("mode option must be read once"));
}

#[test]
fn birth_state_reads_follow_value_descriptor_and_replacement_changes() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED)));
    let key = crate::runtime_state_key!(b"_readableState");
    let state = scope.root_nanbox_f64(key.read_value(stream.get_nanbox_f64()));
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), StateData::ReadableBuffered),
        Some(0.0)
    );
    crate::runtime_state_key!(b"length").write_value(state.get_nanbox_f64(), 17.0);
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), StateData::ReadableBuffered),
        Some(17.0)
    );
    crate::runtime_state_key!(b"extra").write_value(state.get_nanbox_f64(), 1.0);
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), StateData::ReadableBuffered),
        Some(17.0)
    );
    let replacement = scope.root_raw_mut_ptr(crate::object::js_object_alloc_plain(1));
    let replacement = scope.root_nanbox_f64(box_pointer(
        replacement.get_raw_mut_ptr::<ObjectHeader>().cast(),
    ));
    crate::runtime_state_key!(b"length").write_value(replacement.get_nanbox_f64(), 23.0);
    key.write_value(stream.get_nanbox_f64(), replacement.get_nanbox_f64());
    assert_eq!(
        get_hidden_value(stream.get_nanbox_f64(), StateData::ReadableBuffered),
        Some(23.0)
    );
}

#[test]
fn capture_default_memo_follows_native_setter_and_symbol_value_changes() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(super::constructors::event_emitter_constructor_value(
        "EventEmitter",
    ));
    let addr = || crate::value::js_nanbox_get_pointer(ctor.get_nanbox_f64()) as usize;
    let accessor = crate::object::get_accessor_descriptor(addr(), "captureRejections").unwrap();
    let getter = scope.root_nanbox_f64(f64::from_bits(accessor.get));
    let key = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(
        crate::value::js_nanbox_get_pointer(getter.get_nanbox_f64()) as *const ClosureHeader,
        1,
    ));
    let proto = scope.root_nanbox_f64(super::constructors::event_emitter_prototype_value(
        "EventEmitter",
    ));
    for enabled in [true, false, true, false] {
        crate::closure::closure_set_dynamic_prop(
            addr(),
            "captureRejections",
            f64::from_bits(if enabled { TAG_TRUE } else { TAG_FALSE }),
        );
        assert_eq!(super::event_emitter::emitter_capture_default(), enabled);
        assert_eq!(super::event_emitter::emitter_capture_default(), enabled);
    }
    for enabled in [true, false] {
        unsafe {
            crate::object::js_object_set_property_key(
                proto.get_nanbox_f64(),
                key.get_nanbox_f64(),
                f64::from_bits(if enabled { TAG_TRUE } else { TAG_FALSE }),
            );
        }
        assert_eq!(super::event_emitter::emitter_capture_default(), enabled);
    }
    let attrs = crate::object::get_property_attrs(addr(), "captureRejections").unwrap();
    assert_eq!(attrs.bits & 4, 0);
}
