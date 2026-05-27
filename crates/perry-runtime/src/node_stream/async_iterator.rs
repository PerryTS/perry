use super::*;

const READABLE_ITERATOR_SHAPE_ID: u32 = 0x7FFF_FF60;
const READABLE_ITERATOR_STREAM_KEY: &[u8] = b"__perryReadableIteratorStream";
const READABLE_ITERATOR_INDEX_KEY: &[u8] = b"__perryReadableIteratorIndex";
const READABLE_ITERATOR_DONE_KEY: &[u8] = b"__perryReadableIteratorDone";

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
    if let Some(err) = readable_hidden_error(stream) {
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_DONE_KEY),
            f64::from_bits(TAG_TRUE),
        );
        return rejected_promise(err);
    }
    let arr = readable_chunks_array(stream);
    let index = get_hidden_value(iterator, hidden_key(READABLE_ITERATOR_INDEX_KEY))
        .and_then(jsvalue_as_f64)
        .unwrap_or(0.0)
        .max(0.0) as u32;
    if arr.is_null() || index >= crate::array::js_array_length(arr) {
        set_hidden_value(
            iterator,
            hidden_key(READABLE_ITERATOR_DONE_KEY),
            f64::from_bits(TAG_TRUE),
        );
        set_hidden_value(stream, hidden_ended_key(), f64::from_bits(TAG_TRUE));
        return readable_iterator_done();
    }
    let value = crate::array::js_array_get_f64(arr, index);
    set_hidden_value(
        iterator,
        hidden_key(READABLE_ITERATOR_INDEX_KEY),
        (index + 1) as f64,
    );
    mark_disturbed(stream);
    resolved_promise(iterator_result(value, false))
}

extern "C" fn ns_readable_iterator_return(closure: *const ClosureHeader) -> f64 {
    set_hidden_value(
        this_value(closure),
        hidden_key(READABLE_ITERATOR_DONE_KEY),
        f64::from_bits(TAG_TRUE),
    );
    readable_iterator_done()
}

extern "C" fn ns_readable_iterator_self(closure: *const ClosureHeader) -> f64 {
    this_value(closure)
}

pub(super) extern "C" fn ns_async_iterator(closure: *const ClosureHeader) -> f64 {
    build_readable_async_iterator(this_value(closure))
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

fn build_readable_async_iterator(stream: f64) -> f64 {
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
    install_async_iterator_symbol(iterator, ns_readable_iterator_self);
    iterator
}

pub(super) fn install_readable_async_iterator_symbol(stream: f64) {
    install_async_iterator_symbol(stream, ns_async_iterator);
}
