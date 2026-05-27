//! Unit tests for [`super`] (`node_stream.rs`). Split out of node_stream.rs
//! to keep that file under the 2000-line gate (#1746/#1537 batch).

use super::*;
use std::cell::RefCell;

thread_local! {
    static WRITE_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static READABLE_DATA_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static READABLE_THIS_MATCHES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static READABLE_END_COUNT: RefCell<usize> = const { RefCell::new(0) };
}

fn string_value(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    box_string(ptr)
}

fn buffer_value(bytes: &[u8]) -> f64 {
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            crate::buffer::buffer_data_mut(buf),
            bytes.len(),
        );
    }
    box_pointer(buf as *const u8)
}

extern "C" fn write_capture(_closure: *const ClosureHeader, chunk: f64, _enc: f64, cb: f64) -> f64 {
    let readable = js_node_stream_readable_from(chunk);
    let bytes = js_node_stream_collect_bytes(readable);
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().push(bytes));
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn noop_listener(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn capture_data_listener(closure: *const ClosureHeader, chunk: f64) -> f64 {
    let expected = crate::closure::js_closure_get_capture_f64(closure, 0);
    let actual = crate::object::js_implicit_this_get();
    READABLE_THIS_MATCHES.with(|matches| {
        matches
            .borrow_mut()
            .push(actual.to_bits() == expected.to_bits())
    });
    let readable = js_node_stream_readable_from(chunk);
    READABLE_DATA_CAPTURED.with(|captured| {
        captured
            .borrow_mut()
            .push(js_node_stream_collect_bytes(readable))
    });
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn capture_end_listener(closure: *const ClosureHeader) -> f64 {
    let expected = crate::closure::js_closure_get_capture_f64(closure, 0);
    let actual = crate::object::js_implicit_this_get();
    READABLE_THIS_MATCHES.with(|matches| {
        matches
            .borrow_mut()
            .push(actual.to_bits() == expected.to_bits())
    });
    READABLE_END_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn read_records_this(closure: *const ClosureHeader) -> f64 {
    let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
    set_hidden_value(stream, hidden_error_key(), string_value("from-read"));
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn readable_from_retains_string_chunks_for_consumers() {
    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, string_value("he"));
    arr = crate::array::js_array_push_f64(arr, string_value("llo"));

    let readable = js_node_stream_readable_from(box_pointer(arr as *const u8));

    assert_eq!(js_node_stream_collect_bytes(readable), b"hello");
}

#[test]
fn readable_from_retains_buffer_chunks_for_consumers() {
    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, buffer_value(b"ab"));
    arr = crate::array::js_array_push_f64(arr, buffer_value(b"cd"));

    let readable = js_node_stream_readable_from(box_pointer(arr as *const u8));

    assert_eq!(js_node_stream_collect_bytes(readable), b"abcd");
}

#[test]
fn writable_options_write_callback_is_invoked_by_stub_write() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    let opts = crate::object::js_object_alloc(0, 1);
    let closure = js_closure_alloc(write_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );

    let writable = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let write = js_object_get_field_by_name_f64(
        raw_ptr_from_value(writable) as *const ObjectHeader,
        hidden_key(b"write"),
    );
    let args = [string_value("chunk"), f64::from_bits(TAG_UNDEFINED)];
    unsafe {
        let _ = crate::closure::js_native_call_value(write, args.as_ptr(), args.len());
    }

    WRITE_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"chunk".to_vec()]);
    });
}

#[test]
fn readable_options_read_callback_this_is_rebound_to_stream() {
    let opts = crate::object::js_object_alloc(0, 1);
    let closure = js_closure_alloc(
        read_records_this as *const u8,
        crate::closure::CAPTURES_THIS_FLAG | 1,
    );
    crate::closure::js_register_closure_arity(read_records_this as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(closure, 0, box_pointer(opts as *const u8));
    js_object_set_field_by_name(
        opts,
        hidden_key(b"read"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );

    let readable = js_node_stream_readable_new(box_pointer(opts as *const u8));

    let err = js_node_stream_hidden_error_after_read(readable).expect("stream error");
    assert!(string_value_eq(err, b"from-read"));
    assert!(readable_hidden_error(box_pointer(opts as *const u8)).is_none());
}

#[test]
fn stream_methods_use_implicit_this_without_closure_capture() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let prev_this = crate::object::js_implicit_this_set(stream);
    let _ = ns_end3(
        std::ptr::null(),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
    crate::object::js_implicit_this_set(prev_this);

    assert!(js_node_stream_is_stub_ended_after_read(stream));
}

#[test]
fn stream_method_closure_capture_wins_over_stale_implicit_this() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let other = box_pointer(crate::object::js_object_alloc(0, 0) as *const u8);
    let end = js_object_get_field_by_name_f64(
        raw_ptr_from_value(stream) as *const ObjectHeader,
        hidden_key(b"end"),
    );

    let prev_this = crate::object::js_implicit_this_set(other);
    unsafe {
        let _ = crate::closure::js_native_call_value(end, std::ptr::null(), 0);
    }
    crate::object::js_implicit_this_set(prev_this);

    assert!(js_node_stream_is_stub_ended_after_read(stream));
    assert!(!stream_hidden_ended(other));
}

#[test]
fn stream_methods_dispatch_through_dynamic_method_call() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    unsafe {
        let _ = crate::object::js_native_call_method(
            stream,
            b"end".as_ptr() as *const i8,
            3,
            std::ptr::null(),
            0,
        );
    }

    assert!(js_node_stream_is_stub_ended_after_read(stream));
}

#[test]
fn stream_max_listeners_default_and_override_round_trip() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let get_max = js_object_get_field_by_name_f64(obj, hidden_key(b"getMaxListeners"));
    let set_max = js_object_get_field_by_name_f64(obj, hidden_key(b"setMaxListeners"));

    let initial = unsafe { crate::closure::js_native_call_value(get_max, std::ptr::null(), 0) };
    assert_eq!(initial, 10.0);

    let returned = unsafe { crate::closure::js_native_call_value(set_max, [25.0].as_ptr(), 1) };
    assert_eq!(returned.to_bits(), stream.to_bits());

    let updated = unsafe { crate::closure::js_native_call_value(get_max, std::ptr::null(), 0) };
    assert_eq!(updated, 25.0);

    let other = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let other_get = js_object_get_field_by_name_f64(
        raw_ptr_from_value(other) as *const ObjectHeader,
        hidden_key(b"getMaxListeners"),
    );
    let other_initial =
        unsafe { crate::closure::js_native_call_value(other_get, std::ptr::null(), 0) };
    assert_eq!(other_initial, 10.0);

    let native_handle = raw_ptr_from_value(other) as i64;
    assert_eq!(js_node_stream_method_get_max_listeners(native_handle), 10.0);
    assert_eq!(
        js_node_stream_method_set_max_listeners(native_handle, 42.0).to_bits(),
        other.to_bits()
    );
    assert_eq!(js_node_stream_method_get_max_listeners(native_handle), 42.0);
}

#[test]
fn stream_event_names_raw_listeners_and_prepend_chainability() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let raw_listeners = js_object_get_field_by_name_f64(obj, hidden_key(b"rawListeners"));
    let event_names = js_object_get_field_by_name_f64(obj, hidden_key(b"eventNames"));
    let prepend = js_object_get_field_by_name_f64(obj, hidden_key(b"prependListener"));

    let raw = unsafe {
        crate::closure::js_native_call_value(raw_listeners, [string_value("never")].as_ptr(), 1)
    };
    assert_eq!(
        crate::array::js_array_length(raw_ptr_from_value(raw) as *const crate::array::ArrayHeader),
        0
    );

    let names = unsafe { crate::closure::js_native_call_value(event_names, std::ptr::null(), 0) };
    assert_eq!(
        crate::array::js_array_length(raw_ptr_from_value(names) as *const crate::array::ArrayHeader),
        0
    );

    let cb = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let returned = unsafe {
        crate::closure::js_native_call_value(prepend, [string_value("data"), cb].as_ptr(), 2)
    };
    assert_eq!(returned.to_bits(), stream.to_bits());

    let raw = unsafe {
        crate::closure::js_native_call_value(raw_listeners, [string_value("data")].as_ptr(), 1)
    };
    let raw_arr = raw_ptr_from_value(raw) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(raw_arr), 1);
    assert_eq!(
        crate::array::js_array_get_f64(raw_arr, 0).to_bits(),
        cb.to_bits()
    );

    let names = unsafe { crate::closure::js_native_call_value(event_names, std::ptr::null(), 0) };
    let names_arr = raw_ptr_from_value(names) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(names_arr), 1);
    assert!(string_value_eq(
        crate::array::js_array_get_f64(names_arr, 0),
        b"data"
    ));

    let native = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(native) as i64;
    let native_raw = js_node_stream_method_raw_listeners(handle, string_value("never"))
        as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(native_raw), 0);
    let native_names =
        js_node_stream_method_event_names(handle) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(native_names), 0);
    assert_eq!(
        js_node_stream_method_prepend_listener(handle, string_value("data"), cb).to_bits(),
        native.to_bits()
    );
    assert_eq!(
        js_node_stream_method_listener_count(handle, string_value("data")),
        1.0
    );
}

#[test]
fn stream_listener_count_and_listeners_reflect_data_end_storage() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let on = js_object_get_field_by_name_f64(obj, hidden_key(b"on"));
    let listener_count = js_object_get_field_by_name_f64(obj, hidden_key(b"listenerCount"));
    let listeners = js_object_get_field_by_name_f64(obj, hidden_key(b"listeners"));
    let add_listener = js_object_get_field_by_name_f64(obj, hidden_key(b"addListener"));
    assert_eq!(on.to_bits(), add_listener.to_bits());

    let cb1 = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let cb2 = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let cb3 = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);

    unsafe {
        let _ = crate::closure::js_native_call_value(on, [string_value("data"), cb1].as_ptr(), 2);
        let _ = crate::closure::js_native_call_value(on, [string_value("data"), cb2].as_ptr(), 2);
        let _ = crate::closure::js_native_call_value(on, [string_value("end"), cb3].as_ptr(), 2);
    }

    let count_for = |event: &str| unsafe {
        crate::closure::js_native_call_value(listener_count, [string_value(event)].as_ptr(), 1)
    };
    assert_eq!(count_for("data"), 2.0);
    assert_eq!(count_for("end"), 1.0);
    assert_eq!(count_for("error"), 0.0);

    let data_listeners = unsafe {
        crate::closure::js_native_call_value(listeners, [string_value("data")].as_ptr(), 1)
    };
    let data_arr = raw_ptr_from_value(data_listeners) as *mut crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(data_arr), 2);
    assert_eq!(
        crate::array::js_array_get_f64(data_arr, 0).to_bits(),
        cb1.to_bits()
    );
    assert_eq!(
        crate::array::js_array_get_f64(data_arr, 1).to_bits(),
        cb2.to_bits()
    );

    crate::array::js_array_set_length(data_arr, 0.0);
    assert_eq!(count_for("data"), 2.0);

    let missing_listeners = unsafe {
        crate::closure::js_native_call_value(listeners, [string_value("error")].as_ptr(), 1)
    };
    let missing_arr = raw_ptr_from_value(missing_listeners) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(missing_arr), 0);

    let native_stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let native_handle = raw_ptr_from_value(native_stream) as i64;
    let cb4 = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let cb5 = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(native_handle, string_value("data"), cb4);
    let _ = js_node_stream_method_on(native_handle, string_value("data"), cb5);
    assert_eq!(
        js_node_stream_method_listener_count(native_handle, string_value("data")),
        2.0
    );
    let native_arr = js_node_stream_method_listeners(native_handle, string_value("data"))
        as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(native_arr), 2);
}

#[test]
fn readable_push_emits_data_with_stream_this_and_deferred_end() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());
    READABLE_THIS_MATCHES.with(|matches| matches.borrow_mut().clear());
    READABLE_END_COUNT.with(|count| *count.borrow_mut() = 0);

    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;

    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, stream);
    let data_listener = box_pointer(data_closure as *const u8);

    let end_closure = js_closure_alloc(capture_end_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_end_listener as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(end_closure, 0, stream);
    let end_listener = box_pointer(end_closure as *const u8);

    let _ = js_node_stream_method_on(handle, string_value("data"), data_listener);
    assert_eq!(
        js_node_stream_method_push(handle, string_value("x")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_push(handle, f64::from_bits(TAG_NULL)).to_bits(),
        TAG_FALSE
    );
    let _ = js_node_stream_method_on(handle, string_value("end"), end_listener);

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"x".to_vec()]);
    });
    READABLE_END_COUNT.with(|count| assert_eq!(*count.borrow(), 0));

    let _ = crate::promise::js_promise_run_microtasks();
    READABLE_END_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    READABLE_THIS_MATCHES.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true, true]);
    });

    let _ = js_node_stream_method_push(handle, string_value("late"));
    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"x".to_vec()]);
    });
}

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
fn stream_destroy_with_error_marks_errored_state() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let destroy = js_object_get_field_by_name_f64(
        raw_ptr_from_value(stream) as *const ObjectHeader,
        hidden_key(b"destroy"),
    );
    let err = string_value("boom");

    let ret = unsafe { crate::closure::js_native_call_value(destroy, &err, 1) };

    assert_eq!(ret.to_bits(), stream.to_bits());
    assert_eq!(js_node_stream_is_errored(stream).to_bits(), TAG_FALSE);
    let _ = crate::promise::js_promise_run_microtasks();
    assert_eq!(js_node_stream_is_errored(stream).to_bits(), TAG_TRUE);
}

#[test]
fn stream_native_receiver_methods_update_hidden_state() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let err = string_value("boom");
    let cb = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("error"), cb);

    assert_eq!(
        js_node_stream_method_emit(handle, string_value("error"), err).to_bits(),
        TAG_TRUE
    );
    assert!(js_node_stream_hidden_error_after_read(stream).is_some());

    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));
    assert!(js_node_stream_is_stub_ended_after_read(stream));

    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let _ = js_node_stream_method_destroy(handle, err);
    assert!(readable_hidden_error(stream).is_none());
    let _ = crate::promise::js_promise_run_microtasks();
    assert!(js_node_stream_hidden_error_after_read(stream).is_some());
}

#[test]
fn stream_stub_arities_are_registered_per_thread() {
    let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        crate::closure::lookup_closure_arity(ns_end3 as *const u8),
        Some(3)
    );
    assert_eq!(
        crate::closure::lookup_closure_arity(ns_destroy1 as *const u8),
        Some(1)
    );
    assert_eq!(
        crate::closure::lookup_closure_arity(ns_destroy_error_microtask as *const u8),
        Some(0)
    );

    std::thread::spawn(|| {
        let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_end3 as *const u8),
            Some(3)
        );
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_destroy1 as *const u8),
            Some(1)
        );
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_destroy_error_microtask as *const u8),
            Some(0)
        );
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_write2 as *const u8),
            Some(2)
        );
    })
    .join()
    .expect("stream arity registration thread should not panic");
}
