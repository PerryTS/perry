use super::*;
use std::cell::Cell;

thread_local! {
    static READABLES: Cell<usize> = const { Cell::new(0) };
    static ENDS: Cell<usize> = const { Cell::new(0) };
}

extern "C" fn readable(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    READABLES.with(|count| count.set(count.get() + 1));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn end(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    ENDS.with(|count| count.set(count.get() + 1));
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn buffered_eof_notifies_readable_and_emits_end_once_after_drain() {
    let _global = crate::gc::global_side_table_test_lock();
    READABLES.with(|count| count.set(0));
    ENDS.with(|count| count.set(0));
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_node_stream_readable_new(f64::from_bits(TAG_UNDEFINED)));
    for (name, info) in [
        (b"readable".as_slice(), crate::fn_info!(readable, 0)),
        (b"end".as_slice(), crate::fn_info!(end, 0)),
    ] {
        let listener = scope.root_raw_mut_ptr(js_closure_alloc(info, 0));
        js_node_stream_method_on(
            raw_ptr_from_value(stream.get_nanbox_f64()) as i64,
            literal_string_value(name),
            listener.with_mut_ptr(|listener: *mut ClosureHeader| {
                crate::value::js_nanbox_pointer(listener as i64)
            }),
        );
    }
    js_node_stream_method_push(
        raw_ptr_from_value(stream.get_nanbox_f64()) as i64,
        literal_string_value(b"abc"),
    );
    crate::promise::js_promise_run_microtasks();
    READABLES.with(|count| assert_eq!(count.get(), 1));
    js_node_stream_method_push(
        raw_ptr_from_value(stream.get_nanbox_f64()) as i64,
        f64::from_bits(TAG_NULL),
    );
    crate::promise::js_promise_run_microtasks();
    READABLES.with(|count| assert_eq!(count.get(), 2, "EOF wakes a reader with buffered bytes"));
    ENDS.with(|count| assert_eq!(count.get(), 0));
    let bytes =
        js_node_stream_method_read(raw_ptr_from_value(stream.get_nanbox_f64()) as i64, f64::NAN);
    assert_eq!(js_node_stream_collect_bytes(bytes), b"abc");
    js_node_stream_method_read(raw_ptr_from_value(stream.get_nanbox_f64()) as i64, f64::NAN);
    crate::promise::js_promise_run_microtasks();
    ENDS.with(|count| assert_eq!(count.get(), 1));
    js_node_stream_method_push(
        raw_ptr_from_value(stream.get_nanbox_f64()) as i64,
        f64::from_bits(TAG_NULL),
    );
    crate::promise::js_promise_run_microtasks();
    ENDS.with(|count| assert_eq!(count.get(), 1));
}
