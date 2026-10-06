//! Writable completion and Transform finalization, shared by JS and payload streams.
use super::*;

pub(in crate::node_stream) fn schedule_writable_finish(stream: f64, callback: Option<f64>) {
    if has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
        || has_truthy_hidden(stream, hidden_writable_final_pending_key())
    {
        return;
    }
    if let Some(final_callback) = writable_hidden_final(stream) {
        if !has_truthy_hidden(stream, hidden_writable_final_invoked_key()) {
            set_hidden_value(
                stream,
                hidden_writable_final_invoked_key(),
                f64::from_bits(TAG_TRUE),
            );
            set_hidden_value(
                stream,
                hidden_writable_final_pending_key(),
                f64::from_bits(TAG_TRUE),
            );
            let cb = js_closure_alloc(crate::fn_info!(ns_writable_final_callback_done, 1), 2);
            js_closure_set_capture_f64(cb, 0, stream);
            js_closure_set_capture_f64(
                cb,
                1,
                callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED)),
            );
            let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
            unsafe {
                let _ = crate::closure::native_call_value_this(
                    final_callback,
                    crate::closure::JsThis::from_f64(stream),
                    [cb_value].as_ptr(),
                    1,
                );
            }
            return;
        }
    }
    set_hidden_value(
        stream,
        hidden_finish_scheduled_key(),
        f64::from_bits(TAG_TRUE),
    );
    let closure = js_closure_alloc(crate::fn_info!(ns_writable_finish_microtask, 0), 2);
    js_closure_set_capture_ptr(closure, 0, stream.to_bits() as i64);
    js_closure_set_capture_ptr(
        closure,
        1,
        callback
            .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
            .to_bits() as i64,
    );
    if has_truthy_hidden(stream, hidden_writable_final_invoked_key())
        && crate::node_stream::native_hooks::hooks_of(stream)
            .is_some_and(|h| h.timing == crate::node_stream::native_hooks::StepTiming::DEFERRED)
    {
        // A deferred binding whose _final has completed queues writable
        // finish before Promise continuations from that task. The default
        // Transform final queues readable end first through the normal queue.
        crate::builtins::js_queue_next_tick(closure as i64);
    } else {
        crate::builtins::js_queue_microtask(closure as i64);
    }
}

pub(in crate::node_stream) fn schedule_writable_finish_then_transform_end(
    stream: f64,
    callback: Option<f64>,
) {
    // node's Transform `final` pushes `null` (queueing the readable `end`)
    // before its callback lets `finish` go, so `end` is queued first when no
    // user `_final` stands in between.
    if is_transform_stream(stream)
        && !has_truthy_hidden(stream, hidden_transform_finishing_key())
        && !has_truthy_hidden(stream, hidden_writable_final_pending_key())
        && (writable_hidden_final(stream).is_none()
            || has_truthy_hidden(stream, hidden_writable_final_invoked_key()))
    {
        schedule_readable_end(stream);
    }
    schedule_writable_finish(stream, callback);
    let finish_ready = has_truthy_hidden(stream, hidden_finish_scheduled_key())
        || has_truthy_hidden(stream, hidden_finish_emitted_key());
    let final_pending = has_truthy_hidden(stream, hidden_writable_final_pending_key());
    if is_transform_stream(stream)
        && finish_ready
        && !final_pending
        && !has_truthy_hidden(stream, hidden_transform_finishing_key())
    {
        schedule_readable_end(stream);
    }
}

pub(in crate::node_stream) fn set_pending_writable_finish_callback(
    stream: f64,
    callback: Option<f64>,
) {
    let value = callback.unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED));
    set_hidden_value(stream, hidden_writable_pending_finish_callback_key(), value);
}

fn take_pending_writable_finish_callback(stream: f64) -> Option<f64> {
    let value = get_hidden_value(stream, hidden_writable_pending_finish_callback_key());
    set_hidden_value(
        stream,
        hidden_writable_pending_finish_callback_key(),
        f64::from_bits(TAG_UNDEFINED),
    );
    value.filter(|v| is_callable_value(*v))
}

pub(in crate::node_stream) fn schedule_pending_writable_finish_if_ready(stream: f64) {
    if has_truthy_hidden(stream, hidden_transform_end_pending_key())
        && writable_length(stream) == 0.0
        && !super::write_state::writable_writing(stream)
    {
        // The transform's deferred `end()`: every write completed, so its
        // flush (or a native stream's Final step) runs now.
        set_hidden_value(
            stream,
            hidden_transform_end_pending_key(),
            f64::from_bits(TAG_FALSE),
        );
        let callback = take_pending_writable_finish_callback(stream);
        if !finish_transform_stream(stream, callback) {
            finish_stream(stream, callback);
        }
        return;
    }
    if !stream_hidden_ended(stream)
        || writable_length(stream) > 0.0
        || has_truthy_hidden(stream, hidden_finish_emitted_key())
        || has_truthy_hidden(stream, hidden_finish_scheduled_key())
    {
        return;
    }
    let callback = take_pending_writable_finish_callback(stream);
    schedule_writable_finish_then_transform_end(stream, callback);
}
