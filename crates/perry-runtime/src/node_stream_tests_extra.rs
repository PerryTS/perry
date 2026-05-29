//! Second half of unit tests for [`super`] (`node_stream.rs`), split out from
//! `node_stream_tests.rs` to keep each test file under the 2000-line gate.
//!
//! Shared fixtures (thread-local capture state, extern "C" listener closures,
//! string/buffer helpers) live in `super::tests` and are re-imported here.

use super::tests::{
    capture_close_listener, capture_data_listener, capture_drain_listener,
    capture_end_callback_state, capture_end_listener, capture_expected_arg_listener,
    capture_finish_listener, capture_pause_listener, capture_readable_listener,
    capture_resume_listener, noop_listener, string_value, write_capture, write_capture_pending,
    PENDING_WRITE_CALLBACK, READABLE_DATA_CAPTURED, READABLE_END_COUNT, READABLE_READ_CAPTURED,
    READABLE_THIS_MATCHES, STREAM_EVENT_ARG_MATCHES, STREAM_EVENT_ORDER, WRITABLE_CLOSE_COUNT,
    WRITABLE_DRAIN_COUNT, WRITABLE_END_CALLBACK_SNAPSHOT, WRITABLE_FINISH_COUNT, WRITE_CAPTURED,
};
use super::*;

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
fn pipe_and_unpipe_emit_destination_events_with_source() {
    STREAM_EVENT_ARG_MATCHES.with(|matches| matches.borrow_mut().clear());

    let src = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let dest = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let dest_handle = raw_ptr_from_value(dest) as i64;
    let dest_obj = raw_ptr_from_value(dest) as *const ObjectHeader;
    let pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(src) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );
    let unpipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(src) as *const ObjectHeader,
        hidden_key(b"unpipe"),
    );

    let pipe_listener = js_closure_alloc(capture_expected_arg_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_expected_arg_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(pipe_listener, 0, src);
    let listener = box_pointer(pipe_listener as *const u8);
    let _ = js_node_stream_method_on(dest_handle, string_value("pipe"), listener);
    let _ = js_node_stream_method_on(dest_handle, string_value("unpipe"), listener);

    let ret = unsafe { crate::closure::js_native_call_value(pipe, [dest].as_ptr(), 1) };
    assert_eq!(ret.to_bits(), dest.to_bits());
    assert_eq!(
        js_object_get_field_by_name_f64(dest_obj, readable_flowing_key()).to_bits(),
        TAG_NULL
    );
    assert_eq!(
        js_object_get_field_by_name_f64(
            raw_ptr_from_value(src) as *const ObjectHeader,
            readable_flowing_key()
        )
        .to_bits(),
        TAG_TRUE
    );

    let ret = unsafe { crate::closure::js_native_call_value(unpipe, [dest].as_ptr(), 1) };
    assert_eq!(ret.to_bits(), src.to_bits());
    STREAM_EVENT_ARG_MATCHES.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true, true]);
    });
}

#[test]
fn readable_from_pipe_ends_destination() {
    READABLE_END_COUNT.with(|count| *count.borrow_mut() = 0);
    READABLE_THIS_MATCHES.with(|matches| matches.borrow_mut().clear());

    let mut arr = crate::array::js_array_alloc(2);
    arr = crate::array::js_array_push_f64(arr, string_value("a"));
    arr = crate::array::js_array_push_f64(arr, string_value("b"));
    let src = js_node_stream_readable_from(box_pointer(arr as *const u8));
    let dest = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let dest_handle = raw_ptr_from_value(dest) as i64;
    let pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(src) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );

    let data = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let end_closure = js_closure_alloc(capture_end_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_end_listener as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(end_closure, 0, dest);
    let end = box_pointer(end_closure as *const u8);
    let _ = js_node_stream_method_on(dest_handle, string_value("data"), data);
    let _ = js_node_stream_method_on(dest_handle, string_value("end"), end);

    let ret = unsafe { crate::closure::js_native_call_value(pipe, [dest].as_ptr(), 1) };
    assert_eq!(ret.to_bits(), dest.to_bits());
    let _ = crate::promise::js_promise_run_microtasks();

    READABLE_END_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    READABLE_THIS_MATCHES.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true]);
    });
}

#[test]
fn readable_from_pipe_chain_ends_tail_destination() {
    READABLE_END_COUNT.with(|count| *count.borrow_mut() = 0);
    READABLE_THIS_MATCHES.with(|matches| matches.borrow_mut().clear());

    let mut arr = crate::array::js_array_alloc(1);
    arr = crate::array::js_array_push_f64(arr, string_value("x"));
    let src = js_node_stream_readable_from(box_pointer(arr as *const u8));
    let middle = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let sink = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let sink_handle = raw_ptr_from_value(sink) as i64;
    let src_pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(src) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );
    let middle_pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(middle) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );

    let sink_data = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let sink_end_closure = js_closure_alloc(capture_end_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_end_listener as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(sink_end_closure, 0, sink);
    let sink_end = box_pointer(sink_end_closure as *const u8);
    let _ = js_node_stream_method_on(sink_handle, string_value("data"), sink_data);
    let _ = js_node_stream_method_on(sink_handle, string_value("end"), sink_end);

    assert_eq!(
        unsafe { crate::closure::js_native_call_value(src_pipe, [middle].as_ptr(), 1) }.to_bits(),
        middle.to_bits()
    );
    assert_eq!(
        unsafe { crate::closure::js_native_call_value(middle_pipe, [sink].as_ptr(), 1) }.to_bits(),
        sink.to_bits()
    );
    assert_eq!(
        js_object_get_field_by_name_f64(
            raw_ptr_from_value(middle) as *const ObjectHeader,
            readable_flowing_key()
        )
        .to_bits(),
        TAG_TRUE
    );
    let _ = crate::promise::js_promise_run_microtasks();

    READABLE_END_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    READABLE_THIS_MATCHES.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true]);
    });
}

#[test]
fn pause_resume_track_readable_flowing_and_events() {
    STREAM_EVENT_ORDER.with(|events| events.borrow_mut().clear());

    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let pause = js_object_get_field_by_name_f64(obj, hidden_key(b"pause"));
    let resume = js_object_get_field_by_name_f64(obj, hidden_key(b"resume"));
    let is_paused = js_object_get_field_by_name_f64(obj, hidden_key(b"isPaused"));

    assert_eq!(
        js_object_get_field_by_name_f64(obj, readable_flowing_key()).to_bits(),
        TAG_NULL
    );
    assert_eq!(
        unsafe { crate::closure::js_native_call_value(is_paused, std::ptr::null(), 0) }.to_bits(),
        TAG_FALSE
    );

    let data = box_pointer(js_closure_alloc(noop_listener as *const u8, 0) as *const u8);
    let pause_listener =
        box_pointer(js_closure_alloc(capture_pause_listener as *const u8, 0) as *const u8);
    let resume_listener =
        box_pointer(js_closure_alloc(capture_resume_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("pause"), pause_listener);
    let _ = js_node_stream_method_on(handle, string_value("resume"), resume_listener);
    let _ = js_node_stream_method_on(handle, string_value("data"), data);

    assert_eq!(
        js_object_get_field_by_name_f64(obj, readable_flowing_key()).to_bits(),
        TAG_TRUE
    );
    let ret = unsafe { crate::closure::js_native_call_value(pause, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), stream.to_bits());
    assert_eq!(
        js_object_get_field_by_name_f64(obj, readable_flowing_key()).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        unsafe { crate::closure::js_native_call_value(is_paused, std::ptr::null(), 0) }.to_bits(),
        TAG_TRUE
    );
    STREAM_EVENT_ORDER.with(|events| assert_eq!(events.borrow().as_slice(), b"P"));

    let ret = unsafe { crate::closure::js_native_call_value(resume, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), stream.to_bits());
    assert_eq!(
        js_object_get_field_by_name_f64(obj, readable_flowing_key()).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        unsafe { crate::closure::js_native_call_value(is_paused, std::ptr::null(), 0) }.to_bits(),
        TAG_FALSE
    );
    STREAM_EVENT_ORDER.with(|events| assert_eq!(events.borrow().as_slice(), b"P"));

    let _ = crate::promise::js_promise_run_microtasks();
    STREAM_EVENT_ORDER.with(|events| assert_eq!(events.borrow().as_slice(), b"PR"));
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
    assert_eq!(js_node_stream_method_readable_length(handle), 1.0);
    assert_eq!(
        js_object_get_field_by_name_f64(
            raw_ptr_from_value(stream) as *const ObjectHeader,
            hidden_key(b"readableLength"),
        ),
        1.0
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
fn readable_read_default_size_drains_buffer_as_buffer_then_null() {
    READABLE_READ_CAPTURED.with(|captured| captured.borrow_mut().clear());

    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let readable_closure = js_closure_alloc(capture_readable_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_readable_listener as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(readable_closure, 0, stream);

    let _ = js_node_stream_method_push(handle, string_value("abc"));
    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));
    let _ = js_node_stream_method_on(
        handle,
        string_value("readable"),
        box_pointer(readable_closure as *const u8),
    );

    let _ = crate::promise::js_promise_run_microtasks();
    READABLE_READ_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[Some(b"abc".to_vec()), None]);
    });
}

#[test]
fn readable_unshift_prepends_chunk_and_returns_hwm_signal() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;

    let _ = js_node_stream_method_push(handle, string_value("world"));
    assert_eq!(
        js_node_stream_method_unshift(handle, string_value("hello ")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(js_node_stream_method_readable_length(handle), 11.0);

    let joined = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(stream_test_buffer_bytes(joined), b"hello world");
    assert_eq!(js_node_stream_method_readable_length(handle), 0.0);

    let opts = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(opts, hidden_key(b"highWaterMark"), 2.0);
    let low_hwm = js_node_stream_readable_new(box_pointer(opts as *const u8));
    let low_handle = raw_ptr_from_value(low_hwm) as i64;
    assert_eq!(
        js_node_stream_method_unshift(low_handle, string_value("abc")).to_bits(),
        TAG_FALSE
    );
}

fn stream_test_buffer_bytes(value: f64) -> Vec<u8> {
    let len = crate::buffer::js_native_buffer_byte_len(value);
    let data = crate::buffer::js_native_buffer_data_ptr(value);
    if data.is_null() || len == 0 {
        return Vec::new();
    }
    unsafe { std::slice::from_raw_parts(data, len).to_vec() }
}

#[test]
fn readable_read_with_size_splits_buffer_and_keeps_remainder() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;

    let _ = js_node_stream_method_push(handle, string_value("abcdef"));
    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));
    assert_eq!(js_node_stream_method_readable_length(handle), 6.0);

    let first = js_node_stream_method_read(handle, 3.0);
    assert_eq!(stream_test_buffer_bytes(first), b"abc");
    assert_eq!(js_node_stream_method_readable_length(handle), 3.0);

    let second = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(stream_test_buffer_bytes(second), b"def");
    assert_eq!(js_node_stream_method_readable_length(handle), 0.0);
    assert_eq!(
        js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED)).to_bits(),
        TAG_NULL
    );

    let ended = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let ended_handle = raw_ptr_from_value(ended) as i64;
    let _ = js_node_stream_method_push(ended_handle, string_value("ab"));
    let _ = js_node_stream_method_push(ended_handle, f64::from_bits(TAG_NULL));
    let larger_than_remaining = js_node_stream_method_read(ended_handle, 100.0);
    assert_eq!(stream_test_buffer_bytes(larger_than_remaining), b"ab");
    assert_eq!(js_node_stream_method_readable_length(ended_handle), 0.0);
}

#[test]
fn destroyed_readable_drops_late_push_data() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());

    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, stream);
    let data_listener = box_pointer(data_closure as *const u8);

    let _ = js_node_stream_method_on(handle, string_value("data"), data_listener);
    let _ = js_node_stream_method_destroy(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        js_node_stream_method_push(handle, string_value("late")).to_bits(),
        TAG_FALSE
    );

    READABLE_DATA_CAPTURED.with(|captured| assert!(captured.borrow().is_empty()));
}

fn duplex_allow_half_open_defaults_true_and_honors_false_option() {
    let stream = js_node_stream_duplex_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"allowHalfOpen")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_allow_half_open(handle).to_bits(),
        TAG_TRUE
    );

    let opts = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"allowHalfOpen"),
        f64::from_bits(TAG_FALSE),
    );
    let stream = js_node_stream_duplex_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"allowHalfOpen")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_allow_half_open(handle).to_bits(),
        TAG_FALSE
    );
}

#[test]
fn readable_encoding_tracks_constructor_and_set_encoding() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;

    assert_eq!(
        js_node_stream_method_readable_encoding(handle).to_bits(),
        TAG_NULL
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableEncoding")).to_bits(),
        TAG_NULL
    );

    assert_eq!(
        js_node_stream_method_set_encoding(handle, string_value("utf8")).to_bits(),
        stream.to_bits()
    );
    assert!(string_value_eq(
        js_node_stream_method_readable_encoding(handle),
        b"utf8"
    ));
    assert!(string_value_eq(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableEncoding")),
        b"utf8"
    ));

    let opts = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(opts, hidden_key(b"encoding"), string_value("hex"));
    let from_opts = js_node_stream_readable_new(box_pointer(opts as *const u8));
    assert!(string_value_eq(
        js_node_stream_method_readable_encoding(raw_ptr_from_value(from_opts) as i64),
        b"hex"
    ));
}

#[test]
fn stream_object_mode_fields_reflect_defaults_and_options() {
    let readable = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let readable_obj = raw_ptr_from_value(readable) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(readable_obj, hidden_key(b"readableObjectMode")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_readable_object_mode(raw_ptr_from_value(readable) as i64).to_bits(),
        TAG_FALSE
    );

    let writable = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let writable_obj = raw_ptr_from_value(writable) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(writable_obj, hidden_key(b"writableObjectMode")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_writable_object_mode(raw_ptr_from_value(writable) as i64).to_bits(),
        TAG_FALSE
    );

    let opts = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(opts, hidden_key(b"objectMode"), f64::from_bits(TAG_TRUE));
    let object_readable = js_node_stream_readable_new(box_pointer(opts as *const u8));
    let object_readable_obj = raw_ptr_from_value(object_readable) as *const ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name_f64(object_readable_obj, hidden_key(b"readableObjectMode"))
            .to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_readable_object_mode(raw_ptr_from_value(object_readable) as i64)
            .to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_readable_hwm(raw_ptr_from_value(object_readable) as i64),
        16.0
    );
}

#[test]
fn readable_from_uses_node_object_mode_and_high_water_mark_defaults() {
    let mut default_chunks = crate::array::js_array_alloc(1);
    default_chunks = crate::array::js_array_push_f64(default_chunks, string_value("a"));
    let default_readable = js_node_stream_readable_from(box_pointer(default_chunks as *const u8));
    let default_handle = raw_ptr_from_value(default_readable) as i64;
    assert_eq!(
        js_node_stream_method_readable_object_mode(default_handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(js_node_stream_method_readable_hwm(default_handle), 1.0);

    let mut byte_chunks = crate::array::js_array_alloc(1);
    byte_chunks = crate::array::js_array_push_f64(byte_chunks, string_value("b"));
    let opts = crate::object::js_object_alloc(0, 2);
    js_object_set_field_by_name(opts, hidden_key(b"objectMode"), f64::from_bits(TAG_FALSE));
    js_object_set_field_by_name(opts, hidden_key(b"highWaterMark"), 1.0);
    let byte_readable = js_node_stream_readable_from_options(
        box_pointer(byte_chunks as *const u8),
        box_pointer(opts as *const u8),
    );
    let byte_handle = raw_ptr_from_value(byte_readable) as i64;
    assert_eq!(
        js_node_stream_method_readable_object_mode(byte_handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(js_node_stream_method_readable_hwm(byte_handle), 1.0);
}

#[test]
fn writable_corked_counter_tracks_cork_balance() {
    let stream = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let cork = js_object_get_field_by_name_f64(obj, hidden_key(b"cork"));
    let uncork = js_object_get_field_by_name_f64(obj, hidden_key(b"uncork"));

    assert_eq!(js_node_stream_method_writable_corked(handle), 0.0);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"writableCorked")),
        0.0
    );

    assert_eq!(
        unsafe { crate::closure::js_native_call_value(cork, std::ptr::null(), 0) }.to_bits(),
        TAG_UNDEFINED
    );
    assert_eq!(js_node_stream_method_writable_corked(handle), 1.0);

    let _ = unsafe { crate::closure::js_native_call_value(cork, std::ptr::null(), 0) };
    assert_eq!(js_node_stream_method_writable_corked(handle), 2.0);

    let _ = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    assert_eq!(js_node_stream_method_writable_corked(handle), 1.0);

    let _ = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    let _ = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    assert_eq!(js_node_stream_method_writable_corked(handle), 0.0);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"writableCorked")),
        0.0
    );

    assert_eq!(js_node_stream_method_cork(handle).to_bits(), TAG_UNDEFINED);
    assert_eq!(js_node_stream_method_writable_corked(handle), 1.0);
    assert_eq!(
        js_node_stream_method_uncork(handle).to_bits(),
        TAG_UNDEFINED
    );
    assert_eq!(js_node_stream_method_writable_corked(handle), 0.0);
}

#[test]
fn writable_backpressure_tracks_length_need_drain_and_drain_event() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITABLE_DRAIN_COUNT.with(|count| *count.borrow_mut() = 0);
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = None);

    let opts = crate::object::js_object_alloc(0, 2);
    let closure = js_closure_alloc(write_capture_pending as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_pending as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );
    js_object_set_field_by_name(opts, hidden_key(b"highWaterMark"), 1.0);

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let drain = box_pointer(js_closure_alloc(capture_drain_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("drain"), drain);

    assert_eq!(
        js_node_stream_method_write(handle, string_value("xx"), undefined, undefined).to_bits(),
        TAG_FALSE
    );
    assert_eq!(js_node_stream_method_writable_length(handle), 2.0);
    assert_eq!(
        js_node_stream_method_writable_need_drain(handle).to_bits(),
        TAG_TRUE
    );

    let cb = PENDING_WRITE_CALLBACK.with(|pending| pending.borrow_mut().take().unwrap());
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }

    assert_eq!(js_node_stream_method_writable_length(handle), 0.0);
    assert_eq!(
        js_node_stream_method_writable_need_drain(handle).to_bits(),
        TAG_FALSE
    );
    WRITABLE_DRAIN_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn writable_write_returns_false_before_sync_callback_clears_length() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 2);
    let closure = js_closure_alloc(write_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );
    js_object_set_field_by_name(opts, hidden_key(b"highWaterMark"), 1.0);

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);

    assert_eq!(
        js_node_stream_method_write(handle, string_value("xx"), undefined, undefined).to_bits(),
        TAG_FALSE
    );
    assert_eq!(js_node_stream_method_writable_length(handle), 0.0);
    assert_eq!(
        js_node_stream_method_writable_need_drain(handle).to_bits(),
        TAG_FALSE
    );
    WRITE_CAPTURED.with(|captured| assert_eq!(*captured.borrow(), vec![b"xx".to_vec()]));
}

#[test]
fn writable_end_waits_for_pending_write_before_finish() {
    WRITABLE_FINISH_COUNT.with(|count| *count.borrow_mut() = 0);
    WRITABLE_CLOSE_COUNT.with(|count| *count.borrow_mut() = 0);
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = None);

    let opts = crate::object::js_object_alloc(0, 1);
    let closure = js_closure_alloc(write_capture_pending as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_pending as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let finish =
        box_pointer(js_closure_alloc(capture_finish_listener as *const u8, 0) as *const u8);
    let close = box_pointer(js_closure_alloc(capture_close_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("finish"), finish);
    let _ = js_node_stream_method_on(handle, string_value("close"), close);

    let _ = js_node_stream_method_write(handle, string_value("pending"), undefined, undefined);
    let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));
    let _ = crate::promise::js_promise_run_microtasks();
    WRITABLE_FINISH_COUNT.with(|count| assert_eq!(*count.borrow(), 0));
    WRITABLE_CLOSE_COUNT.with(|count| assert_eq!(*count.borrow(), 0));

    let cb = PENDING_WRITE_CALLBACK.with(|pending| pending.borrow_mut().take().unwrap());
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    let _ = crate::promise::js_promise_run_microtasks();
    WRITABLE_FINISH_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    WRITABLE_CLOSE_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn writable_end_callback_runs_before_finish_and_close_events() {
    WRITABLE_FINISH_COUNT.with(|count| *count.borrow_mut() = 0);
    WRITABLE_CLOSE_COUNT.with(|count| *count.borrow_mut() = 0);
    WRITABLE_END_CALLBACK_SNAPSHOT.with(|snapshot| *snapshot.borrow_mut() = None);
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = None);

    let opts = crate::object::js_object_alloc(0, 1);
    let closure = js_closure_alloc(write_capture_pending as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_pending as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let finish =
        box_pointer(js_closure_alloc(capture_finish_listener as *const u8, 0) as *const u8);
    let close = box_pointer(js_closure_alloc(capture_close_listener as *const u8, 0) as *const u8);
    let end_cb = js_closure_alloc(capture_end_callback_state as *const u8, 1);
    js_closure_set_capture_f64(end_cb, 0, stream);
    let end_cb_value = box_pointer(end_cb as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("finish"), finish);
    let _ = js_node_stream_method_on(handle, string_value("close"), close);

    let _ = js_node_stream_method_write(handle, string_value("pending"), undefined, undefined);
    let _ = js_node_stream_method_end3(handle, undefined, undefined, end_cb_value);
    let _ = crate::promise::js_promise_run_microtasks();
    WRITABLE_END_CALLBACK_SNAPSHOT.with(|snapshot| assert!(snapshot.borrow().is_none()));

    let cb = PENDING_WRITE_CALLBACK.with(|pending| pending.borrow_mut().take().unwrap());
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    let _ = crate::promise::js_promise_run_microtasks();

    WRITABLE_END_CALLBACK_SNAPSHOT.with(|snapshot| {
        assert_eq!(*snapshot.borrow(), Some((0, 0, true, false)));
    });
    WRITABLE_FINISH_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    WRITABLE_CLOSE_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn writable_lifecycle_flags_reflect_end_and_finish() {
    WRITABLE_FINISH_COUNT.with(|count| *count.borrow_mut() = 0);
    WRITABLE_CLOSE_COUNT.with(|count| *count.borrow_mut() = 0);

    let stream = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;

    assert_eq!(js_node_stream_method_writable(handle).to_bits(), TAG_TRUE);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"writable")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"closed")).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_writable_ended(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_node_stream_method_writable_finished(handle).to_bits(),
        TAG_FALSE
    );

    let finish =
        box_pointer(js_closure_alloc(capture_finish_listener as *const u8, 0) as *const u8);
    let close = box_pointer(js_closure_alloc(capture_close_listener as *const u8, 0) as *const u8);
    let _ = js_node_stream_method_on(handle, string_value("finish"), finish);
    let _ = js_node_stream_method_on(handle, string_value("close"), close);

    let _ = js_node_stream_method_end(handle, string_value("done"));
    assert_eq!(js_node_stream_method_writable(handle).to_bits(), TAG_FALSE);
    assert_eq!(
        js_node_stream_method_writable_ended(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_node_stream_method_writable_finished(handle).to_bits(),
        TAG_FALSE
    );

    let _ = crate::promise::js_promise_run_microtasks();
    assert_eq!(
        js_node_stream_method_writable_finished(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"writableFinished")).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"closed")).to_bits(),
        TAG_TRUE
    );
    WRITABLE_FINISH_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    WRITABLE_CLOSE_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
}

#[test]
fn stream_destroy_with_error_marks_errored_state() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let destroy = js_object_get_field_by_name_f64(
        raw_ptr_from_value(stream) as *const ObjectHeader,
        hidden_key(b"destroy"),
    );
    let err = string_value("boom");

    assert_eq!(
        js_node_stream_method_errored(raw_ptr_from_value(stream) as i64).to_bits(),
        TAG_NULL
    );
    let ret = unsafe { crate::closure::js_native_call_value(destroy, &err, 1) };

    assert_eq!(ret.to_bits(), stream.to_bits());
    assert_eq!(js_node_stream_is_errored(stream).to_bits(), TAG_FALSE);
    let _ = crate::promise::js_promise_run_microtasks();
    assert_eq!(js_node_stream_is_errored(stream).to_bits(), TAG_TRUE);
    assert_eq!(
        js_node_stream_method_errored(raw_ptr_from_value(stream) as i64).to_bits(),
        err.to_bits()
    );
}

#[test]
fn readable_pause_native_dispatch_returns_stream() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;

    assert_eq!(
        js_node_stream_method_pause(handle).to_bits(),
        stream.to_bits()
    );
}

#[test]
fn readable_exposes_async_dispose_symbol_method() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let async_dispose = crate::symbol::well_known_symbol("asyncDispose");
    let method = unsafe {
        crate::symbol::js_object_get_symbol_property(
            stream,
            box_pointer(async_dispose as *const u8),
        )
    };

    assert!(is_callable_value(method));
    let result = unsafe { crate::closure::js_native_call_value(method, std::ptr::null(), 0) };
    assert_ne!(result.to_bits(), TAG_UNDEFINED);
    assert_eq!(js_node_stream_method_destroyed(handle).to_bits(), TAG_TRUE);
}

#[test]
fn readable_aborted_reflects_destroy_before_end() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let err = string_value("abort");

    assert_eq!(
        js_node_stream_method_readable_aborted(handle).to_bits(),
        TAG_FALSE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableAborted")).to_bits(),
        TAG_FALSE
    );

    let _ = js_node_stream_method_destroy(handle, err);
    assert_eq!(
        js_node_stream_method_readable_aborted(handle).to_bits(),
        TAG_TRUE
    );
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"readableAborted")).to_bits(),
        TAG_TRUE
    );
    let _ = crate::promise::js_promise_run_microtasks();
    assert_eq!(
        js_node_stream_method_readable_aborted(handle).to_bits(),
        TAG_TRUE
    );

    let ended = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let ended_handle = raw_ptr_from_value(ended) as i64;
    let _ = js_node_stream_method_push(ended_handle, f64::from_bits(TAG_NULL));
    let _ = js_node_stream_method_destroy(ended_handle, err);
    assert_eq!(
        js_node_stream_method_readable_aborted(ended_handle).to_bits(),
        TAG_FALSE
    );
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
            crate::closure::lookup_closure_arity(ns_write3 as *const u8),
            Some(3)
        );
    })
    .join()
    .expect("stream arity registration thread should not panic");
}
