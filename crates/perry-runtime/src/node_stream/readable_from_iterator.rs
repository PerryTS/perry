//! Drive a live Readable.from iterator one result at a time.
//! A pending next() must return to the agent loop: synchronously collecting
//! the iterator blocks future worker messages and bypasses stream credit.
use super::*;

pub(super) fn pull(stream: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let Some(source) = get_hidden_value(
        stream.get_nanbox_f64(),
        hidden_key(READABLE_SOURCE_ITERATOR_KEY),
    ) else {
        return false;
    };
    let source = scope.root_nanbox_f64(source);
    if stream_hidden_ended(stream.get_nanbox_f64()) || stream_destroyed(stream.get_nanbox_f64()) {
        return false;
    }
    if !readable_is_flowing(stream.get_nanbox_f64()) {
        return true;
    }
    if has_truthy_hidden(
        stream.get_nanbox_f64(),
        readable_from_promises::hidden_readable_from_promise_pending_key(),
    ) {
        return true;
    }
    mark_readable_live_push(stream.get_nanbox_f64());
    set_hidden_value(
        stream.get_nanbox_f64(),
        readable_from_promises::hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_TRUE),
    );
    let next = match catch_pipeline_throw(|| unsafe {
        crate::object::js_native_call_method(
            source.get_nanbox_f64(),
            b"next".as_ptr().cast(),
            4,
            std::ptr::null(),
            0,
        )
    }) {
        Ok(next) => scope.root_nanbox_f64(crate::promise::adapt_foreign_promise_value(next)),
        Err(reason) => {
            let reason = scope.root_nanbox_f64(reason);
            destroy_stream(stream.get_nanbox_f64(), reason.get_nanbox_f64());
            return true;
        }
    };
    let promise = if crate::promise::js_value_is_promise(next.get_nanbox_f64()) != 0 {
        crate::value::js_nanbox_get_pointer(next.get_nanbox_f64()) as *mut crate::promise::Promise
    } else {
        crate::promise::js_promise_resolved(next.get_nanbox_f64())
    };
    let promise = scope.root_raw_mut_ptr(promise);
    let fulfill = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(next_fulfilled, 1; with_declared(1)),
        1,
    ));
    let reject = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(next_rejected, 1; with_declared(1)),
        1,
    ));
    js_closure_set_capture_f64(fulfill.get_raw_mut_ptr(), 0, stream.get_nanbox_f64());
    js_closure_set_capture_f64(reject.get_raw_mut_ptr(), 0, stream.get_nanbox_f64());
    crate::promise::js_promise_attach_handlers(
        promise.get_raw_mut_ptr(),
        fulfill.get_raw_mut_ptr(),
        reject.get_raw_mut_ptr(),
    );
    true
}

extern "C" fn next_fulfilled(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    result: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let result = scope.root_nanbox_f64(result);
    set_hidden_value(
        stream.get_nanbox_f64(),
        readable_from_promises::hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    if stream_destroyed(stream.get_nanbox_f64()) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let done_key = scope.root_string_ptr(hidden_key(b"done"));
    let value_key = scope.root_string_ptr(hidden_key(b"value"));
    let step = object_ptr_from_value(result.get_nanbox_f64()).map(|obj| {
        let done = crate::object::js_object_get_field_by_name_f64(
            obj as *const crate::object::ObjectHeader,
            done_key.get_raw_const_ptr(),
        );
        let done = crate::value::js_is_truthy(done) != 0;
        let obj = object_ptr_from_value(result.get_nanbox_f64()).unwrap();
        let value = crate::object::js_object_get_field_by_name_f64(
            obj as *const crate::object::ObjectHeader,
            value_key.get_raw_const_ptr(),
        );
        (done, value)
    });
    let Some((done, value)) = step else {
        let message = literal_string_value(b"Iterator result is not an object");
        let error = crate::error::js_error_new_kind_from_value(
            crate::error::ERROR_KIND_TYPE_ERROR,
            message,
        );
        destroy_stream(stream.get_nanbox_f64(), box_pointer(error.cast()));
        return f64::from_bits(TAG_UNDEFINED);
    };
    if done {
        mark_stream_ended(stream.get_nanbox_f64());
    } else {
        // Publish the chunk in the existing queue before scheduling delivery.
        // If a pipe paused during next(), resume drains this same queue.
        let value = scope.root_nanbox_f64(value);
        let chunks = scope.root_nanbox_f64(ensure_hidden_array(
            stream.get_nanbox_f64(),
            hidden_chunks_key(),
        ));
        let chunks = crate::array::js_array_push_f64(
            raw_ptr_from_value(chunks.get_nanbox_f64()) as *mut crate::array::ArrayHeader,
            value.get_nanbox_f64(),
        );
        let chunks = scope.root_raw_mut_ptr(chunks);
        set_hidden_value(
            stream.get_nanbox_f64(),
            hidden_chunks_key(),
            box_pointer(chunks.get_raw_const_ptr()),
        );
        initialize_readable_from_buffered_length(
            stream.get_nanbox_f64(),
            box_pointer(chunks.get_raw_const_ptr()),
        );
    }
    if readable_is_flowing(stream.get_nanbox_f64()) {
        schedule_readable_from_drain(stream.get_nanbox_f64());
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn next_rejected(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let reason = scope.root_nanbox_f64(reason);
    set_hidden_value(
        stream.get_nanbox_f64(),
        readable_from_promises::hidden_readable_from_promise_pending_key(),
        f64::from_bits(TAG_FALSE),
    );
    if !stream_destroyed(stream.get_nanbox_f64()) {
        destroy_stream(stream.get_nanbox_f64(), reason.get_nanbox_f64());
    }
    f64::from_bits(TAG_UNDEFINED)
}
