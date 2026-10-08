//! Drive a live Readable.from iterator one result at a time.
//! A pending next() must return to the agent loop: synchronously collecting
//! the iterator blocks future worker messages and bypasses stream credit.
use super::*;

pub(super) fn pull(stream: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let Some(source) = get_hidden_value(
        stream.get_nanbox_f64(),
        READABLE_SOURCE_ITERATOR_KEY,
    ) else {
        return false;
    };
    let source = scope.root_nanbox_f64(source);
    if stream_hidden_ended(stream.get_nanbox_f64()) || stream_destroyed(stream.get_nanbox_f64()) {
        return false;
    }
    // A paused pipe still fills its readable buffer up to its HWM. This
    // one-result lookahead discovers EOF before the destination's last write
    // completes, so end() suppresses that write's otherwise redundant drain.
    let buffered = get_hidden_value(stream.get_nanbox_f64(), hidden_buffered_key()).unwrap_or(0.0);
    let hwm = get_hidden_value(stream.get_nanbox_f64(), hidden_hwm_key()).unwrap_or(1.0);
    if !readable_is_flowing(stream.get_nanbox_f64()) && buffered >= hwm {
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
    if done && !readable_chunks_nonempty(stream.get_nanbox_f64()) {
        schedule_readable_end(stream.get_nanbox_f64());
    } else if readable_is_flowing(stream.get_nanbox_f64()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    thread_local! {
        static NEXTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    extern "C" fn next(_: *const ClosureHeader, _: crate::closure::JsThis) -> f64 {
        let count = NEXTS.with(|n| {
            let count = n.get();
            n.set(count + 1);
            count
        });
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(buffer_value_from_bytes(&[count as u8; 1024]));
        let result =
            scope.root_nanbox_f64(box_pointer(crate::object::js_object_alloc(0, 2).cast()));
        set_visible_own_value(
            result.get_nanbox_f64(),
            hidden_key(b"done"),
            f64::from_bits(if count == 4 { TAG_TRUE } else { TAG_FALSE }),
        );
        set_visible_own_value(
            result.get_nanbox_f64(),
            hidden_key(b"value"),
            value.get_nanbox_f64(),
        );
        result.get_nanbox_f64()
    }
    #[test]
    fn synchronous_iterator_is_lazy_and_stops_at_readable_credit() {
        NEXTS.with(|n| n.set(0));
        let scope = crate::gc::RuntimeHandleScope::new();
        let source =
            scope.root_nanbox_f64(box_pointer(crate::object::js_object_alloc(0, 1).cast()));
        let next = js_closure_alloc(crate::fn_info!(next, 0), 0);
        set_visible_own_value(
            source.get_nanbox_f64(),
            hidden_key(b"next"),
            box_pointer(next.cast()),
        );
        let options =
            scope.root_nanbox_f64(box_pointer(crate::object::js_object_alloc(0, 2).cast()));
        set_visible_own_value(
            options.get_nanbox_f64(),
            hidden_key(b"objectMode"),
            f64::from_bits(TAG_FALSE),
        );
        set_visible_own_value(
            options.get_nanbox_f64(),
            hidden_key(b"highWaterMark"),
            1024.0,
        );
        let stream = scope.root_nanbox_f64(constructors::js_node_stream_readable_from_options(
            source.get_nanbox_f64(),
            options.get_nanbox_f64(),
        ));
        assert_eq!(
            NEXTS.with(std::cell::Cell::get),
            0,
            "construction must not exhaust the source"
        );
        assert!(pull(stream.get_nanbox_f64()));
        crate::promise::js_promise_run_microtasks();
        for _ in 0..3 {
            assert!(pull(stream.get_nanbox_f64()));
        }
        crate::promise::js_promise_run_microtasks();
        assert_eq!(
            NEXTS.with(std::cell::Cell::get),
            1,
            "a paused source stops at its HWM"
        );
        assert_eq!(
            get_hidden_value(stream.get_nanbox_f64(), hidden_buffered_key()),
            Some(1024.0)
        );
        crate::gc::js_gc_collect();
        let chunk = scope.root_nanbox_f64(super::super::js_node_stream_method_read(
            raw_ptr_from_value(stream.get_nanbox_f64()) as i64,
            f64::from_bits(TAG_UNDEFINED),
        ));
        crate::buffer::bytes::no_gc(|no_gc| {
            assert_eq!(
                crate::buffer::bytes::bytes(chunk.get_nanbox_f64(), no_gc).unwrap(),
                &[0; 1024]
            )
        });
        assert!(pull(stream.get_nanbox_f64()));
        crate::promise::js_promise_run_microtasks();
        assert_eq!(NEXTS.with(std::cell::Cell::get), 2);
        destroy_stream(stream.get_nanbox_f64(), f64::from_bits(TAG_UNDEFINED));
        crate::promise::js_promise_run_microtasks();
    }
    #[test]
    fn eager_iterator_sabotage_turns_credit_witness_red() {
        let witness = "node_stream::readable_from_iterator::tests::synchronous_iterator_is_lazy_and_stops_at_readable_credit";
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", witness, "--nocapture", "--test-threads=1"])
            .env("PERRY_TEST_STREAM_SABOTAGE", "eager_iterator_from")
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&result.stdout).contains("running 1 test"));
        assert!(
            !result.status.success(),
            "eager iterator materialization must turn the credit witness red"
        );
    }
}
