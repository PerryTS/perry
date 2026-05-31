//! Unit tests for [`super`] (`node_stream.rs`). Split out of node_stream.rs
//! to keep that file under the 2000-line gate (#1746/#1537 batch).

use super::*;
use std::cell::RefCell;

thread_local! {
    pub(super) static WRITE_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static WRITEV_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static WRITEV_BUFFER_SHAPE: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static WRITE_ENCODINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static WRITE_CHUNK_STRING_FLAGS: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static WRITE_CALLBACK_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static READABLE_DATA_CAPTURED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static READABLE_DATA_TEXT_CAPTURED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static READABLE_DATA_STRING_FLAGS: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    pub(super) static READABLE_READ_CAPTURED: RefCell<Vec<Option<Vec<u8>>>> = const { RefCell::new(Vec::new()) };
    pub(super) static READABLE_THIS_MATCHES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    pub(super) static READABLE_END_COUNT: RefCell<usize> = const { RefCell::new(0) };
    static ERROR_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static STREAM_EVENT_ARG_MATCHES: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    pub(super) static STREAM_EVENT_ORDER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    pub(super) static WRITABLE_FINISH_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static WRITABLE_CLOSE_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static WRITABLE_END_CALLBACK_SNAPSHOT: RefCell<Option<(usize, usize, bool, bool)>> = const { RefCell::new(None) };
    pub(super) static WRITABLE_DRAIN_COUNT: RefCell<usize> = const { RefCell::new(0) };
    pub(super) static PENDING_WRITE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    static TRANSFORM_THIS_HAS_STREAM_STATE: RefCell<Vec<bool>> = const { RefCell::new(Vec::new()) };
    static TRANSFORM_FLUSH_COUNT: RefCell<usize> = const { RefCell::new(0) };
}

pub(super) fn string_value(s: &str) -> f64 {
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

fn string_contents(value: f64) -> String {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(value, &mut scratch) else {
        return format!("0x{:x}", value.to_bits());
    };
    if ptr.is_null() {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    String::from_utf8_lossy(bytes).into_owned()
}

pub(super) extern "C" fn write_capture(
    _closure: *const ClosureHeader,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let readable = js_node_stream_readable_from(chunk);
    let bytes = js_node_stream_collect_bytes(readable);
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().push(bytes));
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn write_capture_pending(
    _closure: *const ClosureHeader,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let readable = js_node_stream_readable_from(chunk);
    let bytes = js_node_stream_collect_bytes(readable);
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().push(bytes));
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = Some(cb));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn write_capture_encoding(
    _closure: *const ClosureHeader,
    chunk: f64,
    enc: f64,
    cb: f64,
) -> f64 {
    let readable = js_node_stream_readable_from(chunk);
    let bytes = js_node_stream_collect_bytes(readable);
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().push(bytes));
    WRITE_ENCODINGS.with(|encodings| encodings.borrow_mut().push(string_contents(enc)));
    WRITE_CHUNK_STRING_FLAGS.with(|flags| {
        flags
            .borrow_mut()
            .push(JSValue::from_bits(chunk.to_bits()).is_any_string())
    });
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn write_callback_error(
    closure: *const ClosureHeader,
    _chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let err = crate::closure::js_closure_get_capture_f64(closure, 0);
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, [err].as_ptr(), 1);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writev_capture(_closure: *const ClosureHeader, chunks: f64, cb: f64) -> f64 {
    let chunks = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(chunks);
    for i in 0..len {
        let record = crate::array::js_array_get_f64(chunks, i);
        let record = raw_ptr_from_value(record) as *const ObjectHeader;
        let chunk = js_object_get_field_by_name_f64(record, hidden_key(b"chunk"));
        let encoding = js_object_get_field_by_name_f64(record, hidden_key(b"encoding"));
        let raw = raw_ptr_from_value(chunk);
        WRITEV_BUFFER_SHAPE.with(|shape| {
            shape.borrow_mut().push(
                crate::buffer::is_registered_buffer(raw) && string_value_eq(encoding, b"buffer"),
            )
        });
        let readable = js_node_stream_readable_from(chunk);
        WRITEV_CAPTURED.with(|captured| {
            captured
                .borrow_mut()
                .push(js_node_stream_collect_bytes(readable))
        });
    }
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, std::ptr::null(), 0);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn capture_write_callback(_closure: *const ClosureHeader) -> f64 {
    WRITE_CALLBACK_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn noop_listener(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_data_listener(closure: *const ClosureHeader, chunk: f64) -> f64 {
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

extern "C" fn capture_data_text_listener(_closure: *const ClosureHeader, chunk: f64) -> f64 {
    READABLE_DATA_STRING_FLAGS.with(|flags| {
        flags
            .borrow_mut()
            .push(JSValue::from_bits(chunk.to_bits()).is_any_string())
    });
    READABLE_DATA_TEXT_CAPTURED.with(|captured| captured.borrow_mut().push(string_contents(chunk)));
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_readable_listener(closure: *const ClosureHeader) -> f64 {
    let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
    let got = js_node_stream_method_read(raw_ptr_from_value(stream) as i64, f64::NAN);
    READABLE_READ_CAPTURED.with(|captured| {
        let value = if JSValue::from_bits(got.to_bits()).is_null() {
            None
        } else {
            Some(js_node_stream_collect_bytes(got))
        };
        captured.borrow_mut().push(value);
    });
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_end_listener(closure: *const ClosureHeader) -> f64 {
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

#[test]
fn readable_set_encoding_emits_buffer_chunks_as_strings() {
    READABLE_DATA_TEXT_CAPTURED.with(|captured| captured.borrow_mut().clear());
    READABLE_DATA_STRING_FLAGS.with(|flags| flags.borrow_mut().clear());

    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    test_install_manual_read(stream);
    let handle = raw_ptr_from_value(stream) as i64;
    js_node_stream_method_set_encoding(handle, string_value("base64"));

    let data_closure = js_closure_alloc(capture_data_text_listener as *const u8, 0);
    crate::closure::js_register_closure_arity(capture_data_text_listener as *const u8, 1);
    let _ = js_node_stream_method_on(
        handle,
        string_value("data"),
        box_pointer(data_closure as *const u8),
    );

    let _ = js_node_stream_method_push(handle, buffer_value(b"hello"));
    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));

    let _ = crate::promise::js_promise_run_microtasks();

    READABLE_DATA_STRING_FLAGS.with(|flags| {
        assert_eq!(flags.borrow().as_slice(), &[true, true]);
    });
    READABLE_DATA_TEXT_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &["aGVs".to_string(), "bG8=".to_string()]
        );
    });
}

#[test]
fn readable_set_encoding_read_returns_decoded_string() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    js_node_stream_method_set_encoding(handle, string_value("hex"));

    let _ = js_node_stream_method_push(handle, buffer_value(&[0xab, 0xcd]));
    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));

    let got = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert!(JSValue::from_bits(got.to_bits()).is_any_string());
    assert_eq!(string_contents(got), "abcd");
}

extern "C" fn capture_error_listener(_closure: *const ClosureHeader, _err: f64) -> f64 {
    ERROR_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_expected_arg_listener(
    closure: *const ClosureHeader,
    arg: f64,
) -> f64 {
    let expected = crate::closure::js_closure_get_capture_f64(closure, 0);
    STREAM_EVENT_ARG_MATCHES.with(|matches| {
        matches
            .borrow_mut()
            .push(arg.to_bits() == expected.to_bits())
    });
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_pause_listener(_closure: *const ClosureHeader) -> f64 {
    STREAM_EVENT_ORDER.with(|events| events.borrow_mut().push(b'P'));
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_resume_listener(_closure: *const ClosureHeader) -> f64 {
    STREAM_EVENT_ORDER.with(|events| events.borrow_mut().push(b'R'));
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_finish_listener(_closure: *const ClosureHeader) -> f64 {
    WRITABLE_FINISH_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_close_listener(_closure: *const ClosureHeader) -> f64 {
    WRITABLE_CLOSE_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_end_callback_state(closure: *const ClosureHeader) -> f64 {
    let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
    let handle = raw_ptr_from_value(stream) as i64;
    let finish_count = WRITABLE_FINISH_COUNT.with(|count| *count.borrow());
    let close_count = WRITABLE_CLOSE_COUNT.with(|count| *count.borrow());
    let finished = js_node_stream_method_writable_finished(handle).to_bits() == TAG_TRUE;
    let closed = js_node_stream_method_closed(handle).to_bits() == TAG_TRUE;
    let snapshot = (finish_count, close_count, finished, closed);
    WRITABLE_END_CALLBACK_SNAPSHOT.with(|value| *value.borrow_mut() = Some(snapshot));
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn capture_drain_listener(_closure: *const ClosureHeader) -> f64 {
    WRITABLE_DRAIN_COUNT.with(|count| *count.borrow_mut() += 1);
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn read_records_this(closure: *const ClosureHeader) -> f64 {
    let stream = crate::closure::js_closure_get_capture_f64(closure, 0);
    set_hidden_value(stream, hidden_error_key(), string_value("from-read"));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_upper_callback(
    _closure: *const ClosureHeader,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| {
        matches.borrow_mut().push(
            get_hidden_value(this, hidden_readable_flag_key()).is_some()
                && get_hidden_value(this, hidden_writable_flag_key()).is_some(),
        )
    });
    let readable = js_node_stream_readable_from(chunk);
    let bytes = js_node_stream_collect_bytes(readable);
    let upper = String::from_utf8(bytes).unwrap().to_uppercase();
    let args = [f64::from_bits(TAG_UNDEFINED), string_value(&upper)];
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_identity_callback(
    _closure: *const ClosureHeader,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let args = [f64::from_bits(TAG_UNDEFINED), chunk];
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_push_pair_callback(
    _closure: *const ClosureHeader,
    _chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    let this = crate::object::js_implicit_this_get();
    let push = js_object_get_field_by_name_f64(
        raw_ptr_from_value(this) as *const ObjectHeader,
        hidden_key(b"push"),
    );
    unsafe {
        let _ = crate::closure::js_native_call_value(push, [string_value("a")].as_ptr(), 1);
        let _ = crate::closure::js_native_call_value(push, [string_value("b")].as_ptr(), 1);
        let _ =
            crate::closure::js_native_call_value(cb, [f64::from_bits(TAG_UNDEFINED)].as_ptr(), 1);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_flush_tail_callback(_closure: *const ClosureHeader, cb: f64) -> f64 {
    let this = crate::object::js_implicit_this_get();
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| {
        matches.borrow_mut().push(
            get_hidden_value(this, hidden_readable_flag_key()).is_some()
                && get_hidden_value(this, hidden_writable_flag_key()).is_some(),
        )
    });
    TRANSFORM_FLUSH_COUNT.with(|count| *count.borrow_mut() += 1);
    let args = [f64::from_bits(TAG_UNDEFINED), string_value("!")];
    unsafe {
        let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
    }
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
fn readable_from_typed_uint8array_retains_numeric_byte_chunks() {
    let mut arr = crate::array::js_array_alloc(3);
    arr = crate::array::js_array_push_f64(arr, 1.0);
    arr = crate::array::js_array_push_f64(arr, 2.0);
    arr = crate::array::js_array_push_f64(arr, 3.0);
    let typed =
        crate::typedarray::js_typed_array_new_from_array(crate::typedarray::KIND_UINT8 as i32, arr);

    let readable = js_node_stream_readable_from(box_pointer(typed as *const u8));
    let chunks = readable_hidden_chunks(readable).expect("readable chunks");
    let mut values = Vec::new();
    push_chunk_values(chunks, &mut values, 0);

    assert_eq!(values, vec![1.0, 2.0, 3.0]);
}

#[test]
fn readable_from_set_retains_values_in_insertion_order() {
    let mut set = crate::set::js_set_alloc(3);
    set = crate::set::js_set_add(set, 10.0);
    set = crate::set::js_set_add(set, 20.0);
    set = crate::set::js_set_add(set, 30.0);

    let readable = js_node_stream_readable_from(box_pointer(set as *const u8));
    let chunks = readable_hidden_chunks(readable).expect("readable chunks");
    let mut values = Vec::new();
    push_chunk_values(chunks, &mut values, 0);

    assert_eq!(values, vec![10.0, 20.0, 30.0]);
}

#[test]
fn readable_from_map_retains_entry_pairs_in_insertion_order() {
    let mut map = crate::map::js_map_alloc(2);
    map = crate::map::js_map_set(map, string_value("a"), 1.0);
    map = crate::map::js_map_set(map, string_value("b"), 2.0);

    let readable = js_node_stream_readable_from(box_pointer(map as *const u8));
    let chunks = readable_hidden_chunks(readable).expect("readable chunks");
    let arr = raw_ptr_from_value(chunks) as *const crate::array::ArrayHeader;

    assert_eq!(crate::array::js_array_length(arr), 2);

    let first = crate::array::js_array_get_f64(arr, 0);
    let first_pair = raw_ptr_from_value(first) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(first_pair), 2);
    let mut first_key = Vec::new();
    append_chunk_bytes(
        crate::array::js_array_get_f64(first_pair, 0),
        &mut first_key,
        0,
    );
    assert_eq!(first_key, b"a");
    assert_eq!(crate::array::js_array_get_f64(first_pair, 1), 1.0);

    let second = crate::array::js_array_get_f64(arr, 1);
    let second_pair = raw_ptr_from_value(second) as *const crate::array::ArrayHeader;
    assert_eq!(crate::array::js_array_length(second_pair), 2);
    let mut second_key = Vec::new();
    append_chunk_bytes(
        crate::array::js_array_get_f64(second_pair, 0),
        &mut second_key,
        0,
    );
    assert_eq!(second_key, b"b");
    assert_eq!(crate::array::js_array_get_f64(second_pair, 1), 2.0);
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
fn transform_option_callback_transforms_written_chunks() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());
    READABLE_THIS_MATCHES.with(|matches| matches.borrow_mut().clear());
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| matches.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 1);
    let transform_cb = js_closure_alloc(transform_upper_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_upper_callback as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );

    let stream = js_node_stream_transform_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, stream);
    let _ = js_node_stream_method_on(
        handle,
        string_value("data"),
        box_pointer(data_closure as *const u8),
    );

    let _ = js_node_stream_method_write(
        handle,
        string_value("hello"),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"HELLO".to_vec()]);
    });
    READABLE_THIS_MATCHES.with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
    TRANSFORM_THIS_HAS_STREAM_STATE
        .with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
}

#[test]
fn transform_pipe_chain_applies_callback_output() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());
    READABLE_THIS_MATCHES.with(|matches| matches.borrow_mut().clear());
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| matches.borrow_mut().clear());

    let mut chunks = crate::array::js_array_alloc(1);
    chunks = crate::array::js_array_push_f64(chunks, string_value("ab"));
    let src = js_node_stream_readable_from(box_pointer(chunks as *const u8));

    let opts = crate::object::js_object_alloc(0, 1);
    let transform_cb = js_closure_alloc(transform_upper_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_upper_callback as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );
    let upper = js_node_stream_transform_new(box_pointer(opts as *const u8));
    let sink = js_node_stream_passthrough_new(f64::from_bits(TAG_UNDEFINED));

    let sink_data = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(sink_data, 0, sink);
    let _ = js_node_stream_method_on(
        raw_ptr_from_value(sink) as i64,
        string_value("data"),
        box_pointer(sink_data as *const u8),
    );

    let src_pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(src) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );
    let upper_pipe = js_object_get_field_by_name_f64(
        raw_ptr_from_value(upper) as *const ObjectHeader,
        hidden_key(b"pipe"),
    );
    let _ = unsafe { crate::closure::js_native_call_value(src_pipe, [upper].as_ptr(), 1) };
    let _ = unsafe { crate::closure::js_native_call_value(upper_pipe, [sink].as_ptr(), 1) };
    let _ = crate::promise::js_promise_run_microtasks();

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"AB".to_vec()]);
    });
    READABLE_THIS_MATCHES.with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
    TRANSFORM_THIS_HAS_STREAM_STATE
        .with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
}

#[test]
fn compose_source_transform_applies_stage_snapshot() {
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| matches.borrow_mut().clear());

    let mut chunks = crate::array::js_array_alloc(1);
    chunks = crate::array::js_array_push_f64(chunks, string_value("ab"));
    let src = js_node_stream_readable_from(box_pointer(chunks as *const u8));

    let opts = crate::object::js_object_alloc(0, 1);
    let transform_cb = js_closure_alloc(transform_upper_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_upper_callback as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );
    let upper = js_node_stream_transform_new(box_pointer(opts as *const u8));

    let mut args = crate::array::js_array_alloc(2);
    args = crate::array::js_array_push_f64(args, src);
    args = crate::array::js_array_push_f64(args, upper);
    let composed = js_node_stream_compose_args(args);

    assert_eq!(js_node_stream_collect_bytes(composed), b"AB".to_vec());
    TRANSFORM_THIS_HAS_STREAM_STATE
        .with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
}

#[test]
fn compose_single_transform_returns_stage() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| matches.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 1);
    let transform_cb = js_closure_alloc(transform_upper_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_upper_callback as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );
    let upper = js_node_stream_transform_new(box_pointer(opts as *const u8));

    let mut args = crate::array::js_array_alloc(1);
    args = crate::array::js_array_push_f64(args, upper);
    let composed = js_node_stream_compose_args(args);
    assert_eq!(composed.to_bits(), upper.to_bits());

    let handle = raw_ptr_from_value(composed) as i64;
    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, composed);
    let _ = js_node_stream_method_on(
        handle,
        string_value("data"),
        box_pointer(data_closure as *const u8),
    );

    let _ = js_node_stream_method_write(
        handle,
        string_value("ab"),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
    let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"AB".to_vec()]);
    });
    TRANSFORM_THIS_HAS_STREAM_STATE
        .with(|matches| assert_eq!(matches.borrow().as_slice(), &[true]));
}

#[test]
fn transform_flush_callback_pushes_tail_before_finish() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| matches.borrow_mut().clear());
    TRANSFORM_FLUSH_COUNT.with(|count| *count.borrow_mut() = 0);

    let opts = crate::object::js_object_alloc(0, 2);
    let transform_cb = js_closure_alloc(transform_identity_callback as *const u8, 0);
    let flush_cb = js_closure_alloc(transform_flush_tail_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_identity_callback as *const u8, 3);
    crate::closure::js_register_closure_arity(transform_flush_tail_callback as *const u8, 1);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"flush"),
        box_pointer(flush_cb as *const u8),
    );

    let stream = js_node_stream_transform_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, stream);
    let _ = js_node_stream_method_on(
        handle,
        string_value("data"),
        box_pointer(data_closure as *const u8),
    );

    let _ = js_node_stream_method_write(
        handle,
        string_value("a"),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );
    let _ = js_node_stream_method_end(handle, f64::from_bits(TAG_UNDEFINED));

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"!".to_vec()]
        );
    });
    TRANSFORM_FLUSH_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    TRANSFORM_THIS_HAS_STREAM_STATE.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true]);
    });
}

#[test]
fn transform_callback_can_push_multiple_outputs_per_input() {
    READABLE_DATA_CAPTURED.with(|captured| captured.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 1);
    let transform_cb = js_closure_alloc(transform_push_pair_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(transform_push_pair_callback as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"transform"),
        box_pointer(transform_cb as *const u8),
    );

    let stream = js_node_stream_transform_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let data_closure = js_closure_alloc(capture_data_listener as *const u8, 1);
    crate::closure::js_register_closure_arity(capture_data_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(data_closure, 0, stream);
    let _ = js_node_stream_method_on(
        handle,
        string_value("data"),
        box_pointer(data_closure as *const u8),
    );

    let _ = js_node_stream_method_write(
        handle,
        string_value("x"),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );

    READABLE_DATA_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"b".to_vec()]
        );
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
fn stream_json_stringify_uses_node_state_shape() {
    let readable = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let mut readable_json = String::new();
    unsafe {
        assert!(try_stringify_node_stream_json(
            raw_ptr_from_value(readable) as *const u8,
            &mut readable_json,
        ));
    }
    assert_eq!(
        readable_json,
        r#"{"_events":{},"_readableState":{"highWaterMark":65536,"buffer":[],"bufferIndex":0,"length":0,"pipes":[],"awaitDrainWriters":null}}"#
    );

    let writable = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let mut writable_json = String::new();
    unsafe {
        assert!(try_stringify_node_stream_json(
            raw_ptr_from_value(writable) as *const u8,
            &mut writable_json,
        ));
    }
    assert_eq!(
        writable_json,
        r#"{"_events":{},"_writableState":{"highWaterMark":65536,"length":0,"corked":0,"writelen":0,"bufferedIndex":0,"pendingcb":0}}"#
    );
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
fn callable_stream_constructor_autoinstantiates_passthrough() {
    let ctor = crate::object::bound_native_callable_export_value("stream", "PassThrough");
    let stream = unsafe { crate::closure::js_native_call_value(ctor, std::ptr::null(), 0) };

    assert!(raw_ptr_from_value(stream) >= 0x10000);
    assert!(get_hidden_value(stream, hidden_readable_flag_key()).is_some());
    assert!(get_hidden_value(stream, hidden_writable_flag_key()).is_some());

    let constructor =
        get_hidden_value(stream, hidden_key(b"constructor")).expect("constructor field");
    let (module, method) = unsafe {
        crate::object::bound_native_callable_module_and_method(constructor)
            .expect("bound stream constructor")
    };
    assert_eq!(module, "stream");
    assert_eq!(method, "PassThrough");
}

#[test]
fn readable_pipe_stub_returns_destination_and_rejects_missing_destination() {
    let dest = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));

    assert_eq!(
        ns_pipe2(std::ptr::null(), dest, f64::from_bits(TAG_UNDEFINED)).to_bits(),
        dest.to_bits()
    );
    assert!(pipe_destination_is_missing(f64::from_bits(TAG_UNDEFINED)));
    assert!(pipe_destination_is_missing(f64::from_bits(TAG_NULL)));
    assert!(!pipe_destination_is_missing(dest));
}

#[test]
fn readable_wrap_method_is_present_and_chainable() {
    let stream = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let wrap = js_object_get_field_by_name_f64(obj, hidden_key(b"wrap"));
    assert_ne!(wrap.to_bits(), TAG_UNDEFINED);

    let wrapped = js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED));
    let args = [wrapped];
    let result = unsafe { crate::closure::js_native_call_value(wrap, args.as_ptr(), args.len()) };
    assert_eq!(result.to_bits(), stream.to_bits());
}

#[test]
fn writable_cork_and_uncork_update_counter_and_return_undefined() {
    let stream = js_node_stream_writable_new(f64::from_bits(TAG_UNDEFINED));
    let handle = raw_ptr_from_value(stream) as i64;
    let obj = raw_ptr_from_value(stream) as *const ObjectHeader;
    let cork = js_object_get_field_by_name_f64(obj, hidden_key(b"cork"));
    let uncork = js_object_get_field_by_name_f64(obj, hidden_key(b"uncork"));

    assert_eq!(writable_corked_count(stream), 0.0);

    let ret = unsafe { crate::closure::js_native_call_value(cork, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(writable_corked_count(stream), 1.0);
    assert_eq!(
        js_object_get_field_by_name_f64(obj, hidden_key(b"writableCorked")),
        1.0
    );

    let ret = unsafe { crate::closure::js_native_call_value(cork, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(writable_corked_count(stream), 2.0);

    let ret = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(writable_corked_count(stream), 1.0);

    let ret = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(writable_corked_count(stream), 0.0);

    let ret = js_node_stream_method_cork(handle);
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(js_node_stream_method_writable_corked(handle), 1.0);
    let ret = js_node_stream_method_uncork(handle);
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(js_node_stream_method_writable_corked(handle), 0.0);

    let ret = unsafe { crate::closure::js_native_call_value(uncork, std::ptr::null(), 0) };
    assert_eq!(ret.to_bits(), TAG_UNDEFINED);
    assert_eq!(writable_corked_count(stream), 0.0);
}

#[test]
fn writable_cork_buffers_writes_until_uncorked() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    let opts = crate::object::js_object_alloc(0, 1);
    let closure = js_closure_alloc(write_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let _ = js_node_stream_method_cork(handle);
    let _ = js_node_stream_method_cork(handle);
    let _ = js_node_stream_method_write(handle, string_value("a"), undefined, undefined);
    let _ = js_node_stream_method_write(handle, string_value("b"), undefined, undefined);
    WRITE_CAPTURED.with(|captured| assert!(captured.borrow().is_empty()));

    let _ = js_node_stream_method_uncork(handle);
    WRITE_CAPTURED.with(|captured| assert!(captured.borrow().is_empty()));

    let _ = js_node_stream_method_uncork(handle);
    WRITE_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"b".to_vec()]
        );
    });
}

#[test]
fn writable_write_returns_false_at_high_water_mark() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = None);
    let opts = crate::object::js_object_alloc(0, 2);
    let closure = js_closure_alloc(write_capture_pending as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_pending as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(closure as *const u8).bits()),
    );
    js_object_set_field_by_name(opts, hidden_key(b"highWaterMark"), 2.0);

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let first = js_node_stream_method_write(handle, string_value("a"), undefined, undefined);
    let second = js_node_stream_method_write(handle, string_value("b"), undefined, undefined);

    assert_eq!(first.to_bits(), TAG_TRUE);
    assert_eq!(second.to_bits(), TAG_FALSE);
    assert_eq!(writable_length(stream), 2.0);
    WRITE_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"b".to_vec()]
        );
    });
    PENDING_WRITE_CALLBACK.with(|pending| *pending.borrow_mut() = None);
}

#[test]
fn writable_cork_uses_writev_for_multi_chunk_flush() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITEV_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITEV_BUFFER_SHAPE.with(|shape| shape.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 2);
    let write = js_closure_alloc(write_capture as *const u8, 0);
    let writev = js_closure_alloc(writev_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    crate::closure::js_register_closure_arity(writev_capture as *const u8, 2);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"writev"),
        f64::from_bits(JSValue::pointer(writev as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let _ = js_node_stream_method_cork(handle);
    let _ = js_node_stream_method_write(handle, string_value("a"), undefined, undefined);
    let _ = js_node_stream_method_write(handle, string_value("b"), undefined, undefined);
    let _ = js_node_stream_method_uncork(handle);

    WRITE_CAPTURED.with(|captured| assert!(captured.borrow().is_empty()));
    WRITEV_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"b".to_vec()]
        );
    });
    WRITEV_BUFFER_SHAPE.with(|shape| assert_eq!(shape.borrow().as_slice(), &[true, true]));
}

#[test]
fn writable_cork_with_decode_strings_false_preserves_writev_strings() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITEV_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITEV_BUFFER_SHAPE.with(|shape| shape.borrow_mut().clear());

    let opts = crate::object::js_object_alloc(0, 3);
    let write = js_closure_alloc(write_capture as *const u8, 0);
    let writev = js_closure_alloc(writev_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    crate::closure::js_register_closure_arity(writev_capture as *const u8, 2);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"writev"),
        f64::from_bits(JSValue::pointer(writev as *const u8).bits()),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"decodeStrings"),
        f64::from_bits(TAG_FALSE),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let _ = js_node_stream_method_cork(handle);
    let _ = js_node_stream_method_write(handle, string_value("a"), undefined, undefined);
    let _ = js_node_stream_method_write(handle, string_value("b"), undefined, undefined);
    let _ = js_node_stream_method_uncork(handle);

    WRITE_CAPTURED.with(|captured| assert!(captured.borrow().is_empty()));
    WRITEV_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"a".to_vec(), b"b".to_vec()]
        );
    });
    WRITEV_BUFFER_SHAPE.with(|shape| assert_eq!(shape.borrow().as_slice(), &[false, false]));
}

#[test]
fn writable_write_after_end_emits_error_without_calling_write() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    ERROR_COUNT.with(|count| *count.borrow_mut() = 0);

    let opts = crate::object::js_object_alloc(0, 1);
    let write = js_closure_alloc(write_capture as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let error = js_closure_alloc(capture_error_listener as *const u8, 0);
    crate::closure::js_register_closure_arity(capture_error_listener as *const u8, 1);
    let _ = js_node_stream_method_on(
        handle,
        string_value("error"),
        f64::from_bits(JSValue::pointer(error as *const u8).bits()),
    );

    let _ = js_node_stream_method_end(handle, string_value("a"));
    let result = js_node_stream_method_write(
        handle,
        string_value("b"),
        f64::from_bits(TAG_UNDEFINED),
        f64::from_bits(TAG_UNDEFINED),
    );

    assert_eq!(result.to_bits(), TAG_FALSE);
    ERROR_COUNT.with(|count| assert_eq!(*count.borrow(), 1));
    WRITE_CAPTURED.with(|captured| assert_eq!(captured.borrow().as_slice(), &[b"a".to_vec()]));
}

#[test]
fn writable_write_callback_error_emits_error_and_destroys() {
    STREAM_EVENT_ARG_MATCHES.with(|matches| matches.borrow_mut().clear());

    let msg = crate::string::js_string_from_bytes(b"sink-rejection".as_ptr(), 14);
    let err = crate::error::js_error_new_with_message(msg);
    let err_value = crate::value::js_nanbox_pointer(err as i64);

    let opts = crate::object::js_object_alloc(0, 1);
    let write = js_closure_alloc(write_callback_error as *const u8, 1);
    crate::closure::js_register_closure_arity(write_callback_error as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(write, 0, err_value);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    crate::closure::js_register_closure_arity(capture_expected_arg_listener as *const u8, 1);

    let error = js_closure_alloc(capture_expected_arg_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(error, 0, err_value);
    let _ = js_node_stream_method_on(
        handle,
        string_value("error"),
        f64::from_bits(JSValue::pointer(error as *const u8).bits()),
    );

    let write_cb = js_closure_alloc(capture_expected_arg_listener as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(write_cb, 0, err_value);
    let write_cb_value = f64::from_bits(JSValue::pointer(write_cb as *const u8).bits());
    let result = js_node_stream_method_write3(
        handle,
        string_value("x"),
        write_cb_value,
        f64::from_bits(TAG_UNDEFINED),
    );
    assert_eq!(result.to_bits(), TAG_TRUE);

    let _ = crate::promise::js_promise_run_microtasks();

    STREAM_EVENT_ARG_MATCHES.with(|matches| {
        assert_eq!(matches.borrow().as_slice(), &[true, true]);
    });
    assert_eq!(js_node_stream_method_destroyed(handle).to_bits(), TAG_TRUE);
    assert_eq!(
        js_node_stream_method_errored(handle).to_bits(),
        err_value.to_bits()
    );
}

#[test]
fn writable_write_decodes_string_chunks_and_runs_callback() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITE_ENCODINGS.with(|encodings| encodings.borrow_mut().clear());
    WRITE_CHUNK_STRING_FLAGS.with(|flags| flags.borrow_mut().clear());
    WRITE_CALLBACK_COUNT.with(|count| *count.borrow_mut() = 0);

    let opts = crate::object::js_object_alloc(0, 1);
    let write = js_closure_alloc(write_capture_encoding as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_encoding as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let cb = js_closure_alloc(capture_write_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(capture_write_callback as *const u8, 0);
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let result =
        js_node_stream_method_write3(handle, string_value("6869"), string_value("hex"), cb_value);
    assert_eq!(result.to_bits(), TAG_TRUE);

    let result = js_node_stream_method_write3(handle, string_value("ok"), cb_value, undefined);
    assert_eq!(result.to_bits(), TAG_TRUE);

    WRITE_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"hi".to_vec(), b"ok".to_vec()]
        );
    });
    WRITE_ENCODINGS.with(|encodings| {
        assert_eq!(
            encodings.borrow().as_slice(),
            &["buffer".to_string(), "buffer".to_string()]
        );
    });
    WRITE_CHUNK_STRING_FLAGS.with(|flags| {
        assert_eq!(flags.borrow().as_slice(), &[false, false]);
    });
    WRITE_CALLBACK_COUNT.with(|count| assert_eq!(*count.borrow(), 2));
}

#[test]
fn writable_decode_strings_false_preserves_string_chunks() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITE_ENCODINGS.with(|encodings| encodings.borrow_mut().clear());
    WRITE_CHUNK_STRING_FLAGS.with(|flags| flags.borrow_mut().clear());
    WRITE_CALLBACK_COUNT.with(|count| *count.borrow_mut() = 0);

    let opts = crate::object::js_object_alloc(0, 2);
    let write = js_closure_alloc(write_capture_encoding as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_encoding as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );
    js_object_set_field_by_name(
        opts,
        hidden_key(b"decodeStrings"),
        f64::from_bits(TAG_FALSE),
    );

    let stream = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;
    let cb = js_closure_alloc(capture_write_callback as *const u8, 0);
    crate::closure::js_register_closure_arity(capture_write_callback as *const u8, 0);
    let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
    let undefined = f64::from_bits(TAG_UNDEFINED);

    let result =
        js_node_stream_method_write3(handle, string_value("6869"), string_value("hex"), cb_value);
    assert_eq!(result.to_bits(), TAG_TRUE);

    let result = js_node_stream_method_write3(handle, string_value("plain"), cb_value, undefined);
    assert_eq!(result.to_bits(), TAG_TRUE);

    WRITE_CAPTURED.with(|captured| {
        assert_eq!(
            captured.borrow().as_slice(),
            &[b"6869".to_vec(), b"plain".to_vec()]
        );
    });
    WRITE_ENCODINGS.with(|encodings| {
        assert_eq!(
            encodings.borrow().as_slice(),
            &["hex".to_string(), "utf8".to_string()]
        );
    });
    WRITE_CHUNK_STRING_FLAGS.with(|flags| {
        assert_eq!(flags.borrow().as_slice(), &[true, true]);
    });
    WRITE_CALLBACK_COUNT.with(|count| assert_eq!(*count.borrow(), 2));
}

#[test]
fn writable_buffer_write_passes_buffer_encoding() {
    WRITE_CAPTURED.with(|captured| captured.borrow_mut().clear());
    WRITE_ENCODINGS.with(|encodings| encodings.borrow_mut().clear());
    let opts = crate::object::js_object_alloc(0, 1);
    let write = js_closure_alloc(write_capture_encoding as *const u8, 0);
    crate::closure::js_register_closure_arity(write_capture_encoding as *const u8, 3);
    js_object_set_field_by_name(
        opts,
        hidden_key(b"write"),
        f64::from_bits(JSValue::pointer(write as *const u8).bits()),
    );

    let writable = js_node_stream_writable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(writable) as i64;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    let _ = js_node_stream_method_write(handle, buffer_value(b"hi"), undefined, undefined);

    WRITE_CAPTURED.with(|captured| {
        assert_eq!(captured.borrow().as_slice(), &[b"hi".to_vec()]);
    });
    WRITE_ENCODINGS.with(|encodings| {
        assert_eq!(encodings.borrow().as_slice(), &["buffer".to_string()]);
    });
}

#[test]
fn readable_object_mode_read_returns_one_object_per_call() {
    let opts = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(opts, hidden_key(b"objectMode"), f64::from_bits(TAG_TRUE));
    let stream = js_node_stream_readable_new(box_pointer(opts as *const u8));
    let handle = raw_ptr_from_value(stream) as i64;

    let first_obj = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(first_obj, hidden_key(b"a"), 1.0);
    let first_value = box_pointer(first_obj as *const u8);
    let second_obj = crate::object::js_object_alloc(0, 1);
    js_object_set_field_by_name(second_obj, hidden_key(b"b"), 2.0);
    let second_value = box_pointer(second_obj as *const u8);

    let _ = js_node_stream_method_push(handle, first_value);
    let _ = js_node_stream_method_push(handle, second_value);
    let _ = js_node_stream_method_push(handle, f64::from_bits(TAG_NULL));
    assert_eq!(js_node_stream_method_readable_length(handle), 2.0);

    let first_read = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(first_read.to_bits(), first_value.to_bits());
    assert_eq!(js_node_stream_method_readable_length(handle), 1.0);
    let second_read = js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED));
    assert_eq!(second_read.to_bits(), second_value.to_bits());
    assert_eq!(js_node_stream_method_readable_length(handle), 0.0);
    assert_eq!(
        js_node_stream_method_read(handle, f64::from_bits(TAG_UNDEFINED)).to_bits(),
        TAG_NULL
    );
}
