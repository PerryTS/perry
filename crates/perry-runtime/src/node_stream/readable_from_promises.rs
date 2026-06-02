use super::*;

pub(super) fn hidden_readable_from_promise_pending_key() -> *mut crate::string::StringHeader {
    hidden_key(b"__perryReadableFromPromisePending")
}

pub(super) fn attach_readable_from_promise_chunk(stream: f64, chunk: f64) -> bool {
    if crate::promise::js_value_is_promise(chunk) == 0 {
        return false;
    }
    if has_truthy_hidden(stream, hidden_readable_from_promise_pending_key()) {
        return true;
    }
    let promise = crate::value::js_nanbox_get_pointer(chunk) as *mut crate::promise::Promise;
    match crate::promise::js_promise_state(promise) {
        1 => {
            settle_readable_from_promise_fulfilled(
                stream,
                chunk,
                crate::promise::js_promise_result(promise),
            );
            return true;
        }
        2 => {
            settle_readable_from_promise_rejected(
                stream,
                chunk,
                crate::promise::js_promise_result(promise),
            );
            return true;
        }
        _ => {}
    }

    let fulfill = js_closure_alloc(ns_readable_from_promise_fulfilled as *const u8, 2);
    let reject = js_closure_alloc(ns_readable_from_promise_rejected as *const u8, 2);
    js_closure_set_capture_ptr(fulfill, 0, stream.to_bits() as i64);
    js_closure_set_capture_ptr(fulfill, 1, chunk.to_bits() as i64);
    js_closure_set_capture_ptr(reject, 0, stream.to_bits() as i64);
    js_closure_set_capture_ptr(reject, 1, chunk.to_bits() as i64);
    set_hidden_value(
        stream,
        hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_TRUE),
    );
    crate::promise::js_promise_attach_handlers(promise, fulfill, reject);
    true
}

pub(super) fn consume_readable_buffered_front(stream: f64, chunk: f64) {
    let Some(chunks) = readable_hidden_chunks(stream) else {
        return;
    };
    if !is_array_like_value(chunks) {
        clear_readable_buffer(stream);
        return;
    }
    let arr = raw_ptr_from_value(chunks) as *mut crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if len == 0 {
        clear_readable_buffer(stream);
        return;
    }
    let _ = crate::array::js_array_shift_f64(arr);
    if len == 1 {
        clear_readable_buffer(stream);
        return;
    }
    let consumed = if readable_object_mode(stream) {
        1.0
    } else {
        chunk_byte_len(chunk) as f64
    };
    let remaining =
        (get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0) - consumed).max(0.0);
    set_hidden_value(stream, hidden_buffered_key(), remaining);
    set_hidden_value(stream, hidden_key(b"readableLength"), remaining);
}

fn settle_readable_from_promise_fulfilled(stream: f64, chunk: f64, value: f64) {
    set_hidden_value(
        stream,
        hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    if stream_destroyed(stream) {
        return;
    }
    consume_readable_buffered_front(stream, chunk);
    mark_disturbed(stream);
    if readable_is_flowing(stream) {
        emit_readable_data_unchecked(stream, value);
        schedule_readable_from_drain(stream);
    } else {
        buffer_pending_readable_chunk(stream, value);
    }
}

fn settle_readable_from_promise_rejected(stream: f64, chunk: f64, reason: f64) {
    set_hidden_value(
        stream,
        hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    if !stream_destroyed(stream) {
        consume_readable_buffered_front(stream, chunk);
        let _ = emit_stream_event(stream, string_value(b"error"), &[reason]);
        destroy_stream(stream, f64::from_bits(TAG_UNDEFINED));
    }
}

pub(super) extern "C" fn ns_readable_from_promise_fulfilled(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let chunk = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    settle_readable_from_promise_fulfilled(stream, chunk, value);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn ns_readable_from_promise_rejected(
    closure: *const ClosureHeader,
    reason: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let chunk = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    settle_readable_from_promise_rejected(stream, chunk, reason);
    f64::from_bits(TAG_UNDEFINED)
}
