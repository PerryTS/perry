//! Unit tests for [`super`] (`node_stream.rs`). Split out of node_stream.rs
//! to keep that file under the 2000-line gate (#1746/#1537 batch).

use super::*;
use std::cell::RefCell;

thread_local! {
    static WRITE_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
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
    let _ = ns_end1(std::ptr::null(), f64::from_bits(TAG_UNDEFINED));
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
fn stream_native_receiver_methods_update_hidden_state() {
    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let err = string_value("boom");

    assert_eq!(
        js_node_stream_method_emit(handle, string_value("error"), err).to_bits(),
        TAG_TRUE
    );
    assert!(js_node_stream_hidden_error_after_read(stream).is_some());

    let stream = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));
    assert!(js_node_stream_is_stub_ended_after_read(stream));
}

#[test]
fn stream_stub_arities_are_registered_per_thread() {
    let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
    assert_eq!(
        crate::closure::lookup_closure_arity(ns_end1 as *const u8),
        Some(1)
    );

    std::thread::spawn(|| {
        let _ = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_end1 as *const u8),
            Some(1)
        );
        assert_eq!(
            crate::closure::lookup_closure_arity(ns_write2 as *const u8),
            Some(2)
        );
    })
    .join()
    .expect("stream arity registration thread should not panic");
}
