use super::*;

const READABLE_ITERATOR_SHAPE_ID: u32 = 0x7FFF_FF60;
const READABLE_ITERATOR_STREAM_KEY: &[u8] = b"__perryReadableIteratorStream";
const READABLE_ITERATOR_INDEX_KEY: &[u8] = b"__perryReadableIteratorIndex";
const READABLE_ITERATOR_DONE_KEY: &[u8] = b"__perryReadableIteratorDone";
const READABLE_ITERATOR_DESTROY_ON_RETURN_KEY: &[u8] = b"__perryReadableIteratorDestroyOnReturn";
const READABLE_ITERATOR_STREAM_INDEX_KEY: &[u8] = b"__perryReadableIteratorStreamIndex";

fn iterator_result(value: f64, done: bool) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, hidden_key(b"value"), value);
    js_object_set_field_by_name(
        obj,
        hidden_key(b"done"),
        f64::from_bits(if done { TAG_TRUE } else { TAG_FALSE }),
    );
    box_pointer(obj as *const u8)
}

fn readable_iterator_done() -> f64 {
    resolved_promise(iterator_result(f64::from_bits(TAG_UNDEFINED), true))
}

extern "C" fn ns_readable_iterator_chunk_fulfilled(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let outer = js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    if !outer.is_null() {
        crate::promise::js_promise_resolve(outer, iterator_result(value, false));
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn ns_readable_iterator_chunk_rejected(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    let outer = js_closure_get_capture_ptr(closure, 0) as *mut crate::promise::Promise;
    if !outer.is_null() {
        crate::promise::js_promise_reject(outer, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

fn readable_iterator_chunk_result(value: f64) -> f64 {
    if crate::promise::js_value_is_promise(value) == 0 {
        return resolved_promise(iterator_result(value, false));
    }

    let inner = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    let outer = crate::promise::js_promise_new();
    let fulfill = js_closure_alloc(ns_readable_iterator_chunk_fulfilled as *const u8, 1);
    let reject = js_closure_alloc(ns_readable_iterator_chunk_rejected as *const u8, 1);
    js_closure_set_capture_ptr(fulfill, 0, outer as i64);
    js_closure_set_capture_ptr(reject, 0, outer as i64);
    crate::promise::js_promise_attach_handlers(inner, fulfill, reject);
    box_pointer(outer as *const u8)
}

fn destroy_on_return_from_options(opts: f64) -> bool {
    !matches!(
        get_hidden_value(opts, hidden_key(b"destroyOnReturn")),
        Some(value) if value.to_bits() == TAG_FALSE
    )
}

fn iterator_destroys_on_return(iterator: f64) -> bool {
    get_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DESTROY_ON_RETURN_KEY),
    )
    .is_none_or(|value| crate::value::js_is_truthy(value) != 0)
}

fn iterator_has_yielded(iterator: f64) -> bool {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .is_some_and(|index| index > 0.0)
}

fn iterator_local_index(iterator: f64) -> u32 {
    get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .unwrap_or(0.0)
        .max(0.0) as u32
}

fn stream_consume_index(stream: f64) -> u32 {
    get_hidden_value(stream, hidden_key(READABLE_ITERATOR_STREAM_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .unwrap_or(0.0)
        .max(0.0) as u32
}

fn set_stream_consume_index(stream: f64, index: u32) {
    set_hidden_value(
        stream,
        hidden_key(READABLE_ITERATOR_STREAM_INDEX_KEY),
        index as f64,
    );
}

fn settle_iterator_return_value(value: f64) {
    if crate::promise::js_value_is_promise(value) == 0 {
        return;
    }
    let promise = crate::value::js_nanbox_get_pointer(value) as *mut crate::promise::Promise;
    if promise.is_null() {
        return;
    }
    for _ in 0..10_000 {
        if unsafe { (*promise).state } != crate::promise::PromiseState::Pending {
            return;
        }
        if crate::promise::js_promise_run_microtasks() == 0 {
            return;
        }
    }
}

fn call_source_iterator_return(stream: f64) {
    let Some(source_iterator) = get_hidden_value(stream, hidden_key(READABLE_SOURCE_ITERATOR_KEY))
    else {
        return;
    };
    let returned = unsafe {
        crate::object::js_native_call_method(
            source_iterator,
            b"return".as_ptr() as *const i8,
            6,
            std::ptr::null(),
            0,
        )
    };
    settle_iterator_return_value(returned);
}

extern "C" fn ns_readable_iterator_next(closure: *const ClosureHeader) -> f64 {
    let iterator = this_value(closure);
    if get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_DONE_KEY))
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0)
    {
        return readable_iterator_done();
    }
    let Some(stream) = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY)) else {
        return readable_iterator_done();
    };
    prepare_readable_for_iteration(stream);

    if !readable_object_mode(stream) {
        let value = read_stream_available_default(stream);
        if value.to_bits() != TAG_NULL {
            set_hidden_value(
                iterator,
                hidden_key(READABLE_ITERATOR_INDEX_KEY),
                (iterator_local_index(iterator) + 1) as f64,
            );
            return resolved_promise(iterator_result(value, false));
        }
    }

    let arr = readable_chunks_array(stream);
    let index = stream_consume_index(stream);
    if !arr.is_null() && index < crate::array::js_array_length(arr) {
        let value = crate::array::js_array_get_f64(arr, index);
        set_stream_consume_index(stream, index + 1);
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_INDEX_KEY),
            (iterator_local_index(iterator) + 1) as f64,
        );
        mark_disturbed(stream);
        return readable_iterator_chunk_result(value);
    }
    if let Some(err) = readable_hidden_error(stream) {
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_DONE_KEY),
            f64::from_bits(TAG_TRUE),
        );
        return rejected_promise(err);
    }
    if stream_destroyed(stream) {
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_DONE_KEY),
            f64::from_bits(TAG_TRUE),
        );
        return readable_iterator_done();
    }
    if arr.is_null() || index >= crate::array::js_array_length(arr) {
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_DONE_KEY),
            f64::from_bits(TAG_TRUE),
        );
        mark_stream_ended(stream);
        destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
        return readable_iterator_done();
    }
    readable_iterator_done()
}

extern "C" fn ns_readable_iterator_return(closure: *const ClosureHeader) -> f64 {
    let iterator = this_value(closure);
    let already_done = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_DONE_KEY))
        .is_some_and(|v| crate::value::js_is_truthy(v) != 0);
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DONE_KEY),
        f64::from_bits(TAG_TRUE),
    );
    if !already_done && iterator_has_yielded(iterator) && iterator_destroys_on_return(iterator) {
        if let Some(stream) = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY)) {
            call_source_iterator_return(stream);
            destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
        }
    }
    readable_iterator_done()
}

extern "C" fn ns_readable_iterator_self(closure: *const ClosureHeader) -> f64 {
    this_value(closure)
}

pub(super) extern "C" fn ns_async_iterator(closure: *const ClosureHeader) -> f64 {
    build_readable_async_iterator(this_value(closure), true)
}

pub(super) extern "C" fn ns_iterator1(closure: *const ClosureHeader, opts: f64) -> f64 {
    build_readable_async_iterator(this_value(closure), destroy_on_return_from_options(opts))
}

fn install_async_iterator_symbol(target: f64, func: extern "C" fn(*const ClosureHeader) -> f64) {
    let async_iterator = crate::symbol::well_known_symbol("asyncIterator");
    if async_iterator.is_null() {
        return;
    }
    let closure = js_closure_alloc(func as *const u8, 1);
    js_closure_set_capture_ptr(closure, 0, target.to_bits() as i64);
    let closure_value = box_pointer(closure as *const u8);
    let symbol_value = box_pointer(async_iterator as *const u8);
    unsafe {
        crate::symbol::js_object_set_symbol_property(target, symbol_value, closure_value);
    }
}

fn build_readable_async_iterator(stream: f64, destroy_on_return: bool) -> f64 {
    let methods = [
        ("next", cast0(ns_readable_iterator_next)),
        ("return", cast0(ns_readable_iterator_return)),
    ];
    let obj = build_object(&methods, READABLE_ITERATOR_SHAPE_ID + methods.len() as u32);
    let iterator = box_pointer(obj as *const u8);
    set_hidden_value(iterator, hidden_key(READABLE_ITERATOR_STREAM_KEY), stream);
    set_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY), 0.0);
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DONE_KEY),
        f64::from_bits(TAG_FALSE),
    );
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_DESTROY_ON_RETURN_KEY),
        f64::from_bits(if destroy_on_return {
            TAG_TRUE
        } else {
            TAG_FALSE
        }),
    );
    install_async_iterator_symbol(iterator, ns_readable_iterator_self);
    iterator
}

pub(super) fn install_readable_async_iterator_symbol(stream: f64) {
    install_async_iterator_symbol(stream, ns_async_iterator);
}

pub(super) fn register_arities() {
    crate::closure::js_register_closure_arity(ns_async_iterator as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_iterator1 as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iterator_next as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_return as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_self as *const u8, 0);
    crate::closure::js_register_closure_arity(ns_readable_iterator_chunk_fulfilled as *const u8, 1);
    crate::closure::js_register_closure_arity(ns_readable_iterator_chunk_rejected as *const u8, 1);
}
