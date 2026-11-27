//! The readable side's underlying-source driver: `start()`, the pull microtask and its settlement, and draining the read requests a close or error leaves pending.

use super::{
    build_iter_result, byob, controller_value, error_readable_stream, expando, idalloc,
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_get_capture_ptr,
    js_closure_set_capture_ptr, js_promise_reject, js_promise_resolve, js_promise_then,
    js_readable_stream_controller_close, js_readable_stream_controller_enqueue,
    js_value_is_promise, native, readable_clear_algorithms, rs_rec, tee, tee_source_of,
    transform_clear_algorithms, transform_writable_for_readable, ClosureHeader, JsFunctionInfo,
    Promise, ReadableState, RsRef, TAG_UNDEFINED,
};

pub(super) unsafe fn invoke_start(stream_id: usize) {
    let (cb, controller) = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) if !s.started => {
                s.started = true;
                (s.start_cb, controller_value(stream_id))
            }
            _ => return,
        }
    };
    if cb != 0 {
        js_closure_call1(
            cb as *const ClosureHeader,
            perry_runtime::closure::plain_call_receiver(),
            controller,
        );
    }
}

pub(super) extern "C" fn readable_pull_microtask(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    unsafe {
        let stream_bits = js_closure_get_capture_ptr(closure, 0) as u64;
        let stream_id = f64::from_bits(stream_bits) as usize;
        let cb = perry_runtime::closure::closure_capture_ref(closure, 1);
        let pull_returns_byte_chunk = js_closure_get_capture_ptr(closure, 2) != 0;
        let should_pull = {
            match rs_rec(RsRef(stream_id)).as_deref_mut() {
                Some(s) if s.state == ReadableState::Readable && s.pulling => true,
                Some(s) => {
                    s.pulling = false;
                    false
                }
                None => false,
            }
        };
        if should_pull {
            // A Web Streams `pull()` that throws must ERROR the stream (its
            // pending reads reject with the exception) — it must NOT `longjmp`
            // past this Rust microtask frame. Without the local trap the throw
            // unwinds to the microtask loop, skipping the `pulling = false`
            // reset (and any live frames), leaving the stream wedged and, under
            // concurrency, crashing the process. Catch it here and route to the
            // spec error path so `reader.read()` rejects and the consumer's own
            // handler surfaces the error, exactly as Node does.
            let pull_outcome = perry_runtime::exception::js_call_catching(|| {
                if pull_returns_byte_chunk {
                    pull_deferred_byte_chunk(stream_id, cb);
                    f64::from_bits(TAG_UNDEFINED)
                } else {
                    js_closure_call1(
                        cb as *const ClosureHeader,
                        perry_runtime::closure::plain_call_receiver(),
                        controller_value(stream_id),
                    )
                }
            });
            match pull_outcome {
                Ok(result) => {
                    if js_value_is_promise(result) != 0 {
                        let promise =
                            perry_runtime::value::js_nanbox_get_pointer(result) as *mut Promise;
                        if !promise.is_null() {
                            let fulfilled = readable_pull_settled_closure(
                                perry_runtime::fn_info!(readable_pull_fulfilled, 1; with_declared(1)),
                                stream_id,
                            );
                            let rejected = readable_pull_settled_closure(
                                perry_runtime::fn_info!(readable_pull_rejected, 1; with_declared(1)),
                                stream_id,
                            );
                            let _ = js_promise_then(promise, fulfilled, rejected);
                            return f64::from_bits(TAG_UNDEFINED);
                        }
                    }
                    if let Some(s) = rs_rec(RsRef(stream_id)).as_deref_mut() {
                        s.pulling = false;
                    }
                    if rs_rec(RsRef(stream_id))
                        .as_deref()
                        .is_some_and(|s| s.body_consumer.is_some())
                    {
                        maybe_pull(stream_id);
                    }
                }
                Err(exc) => {
                    if let Some(s) = rs_rec(RsRef(stream_id)).as_deref_mut() {
                        s.pulling = false;
                    }
                    error_readable_stream(stream_id, exc.to_bits());
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn readable_pull_settled_closure(
    info: *const JsFunctionInfo,
    stream_id: usize,
) -> *mut ClosureHeader {
    let closure = js_closure_alloc(info, 1);
    perry_runtime::closure::closure_set_capture_word(closure, 0, stream_id as i64);
    closure
}

pub(super) extern "C" fn readable_pull_fulfilled(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _value: f64,
) -> f64 {
    unsafe {
        let stream_id = js_closure_get_capture_ptr(closure, 0) as usize;
        if let Some(s) = rs_rec(RsRef(stream_id)).as_deref_mut() {
            s.pulling = false;
        }
        maybe_pull(stream_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn readable_pull_rejected(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    reason: f64,
) -> f64 {
    unsafe {
        let stream_id = js_closure_get_capture_ptr(closure, 0) as usize;
        let should_error = {
            match rs_rec(RsRef(stream_id)).as_deref_mut() {
                Some(stream) => {
                    stream.pulling = false;
                    stream.state == ReadableState::Readable
                }
                None => false,
            }
        };
        if should_error {
            error_readable_stream(stream_id, reason.to_bits());
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) unsafe fn maybe_pull(stream_id: usize) {
    // #5989: a read on a tee branch pulls its SOURCE — the branch carries no
    // `pull_cb` of its own; the source's enqueue fans the chunk out to both
    // branches. `force` so a highWaterMark-0 flight producer actually pulls.
    if let Some(source) = tee_source_of(stream_id) {
        // Tick parity: a demand-initiated pull travels TWO microtask cycles
        // (Node's sourceReader.read() resolution + .then(fanout) reaction);
        // buffered chunks + close discovery included. A live producer is
        // driven from the pull job.
        tee::tee_schedule_pull_demand(source);
        return;
    }
    maybe_pull_inner(stream_id, false);
}

/// `maybe_pull` that pulls whenever the source can produce more, ignoring the
/// highWaterMark / read-request gate. Used by the synchronous body drain
/// (`new Response(stream)` / `new Request(url, { body: stream })`): that drain
/// has no parked reader, so for a `highWaterMark: 0` stream the normal
/// `maybe_pull` would refuse to pull and the body would drain EMPTY (CodeRabbit
/// #5776). Forcing the pull when the queue is empty drives such a stream to
/// completion regardless of its strategy.
pub(crate) unsafe fn maybe_pull_force(stream_id: usize) {
    if let Some(source) = tee_source_of(stream_id) {
        tee::tee_schedule_pull_demand(source);
        return;
    }
    maybe_pull_inner(stream_id, true);
}

pub(super) unsafe fn maybe_pull_inner(stream_id: usize, force: bool) {
    if native::pull_native(stream_id) {
        return;
    }
    // ShouldCallPull (#4915): a parked read request always justifies a
    // pull (this is what drives byte streams with highWaterMark 0 — the
    // pull only fires once a `read()` / `read(view)` is waiting);
    // otherwise pull while the queue is under the highWaterMark. `force`
    // (drain path) pulls whenever the queue is empty regardless of strategy.
    let has_byob_pending = byob::has_pending(stream_id);
    let (cb, controller, should_pull, pull_returns_byte_chunk) = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) if s.state == ReadableState::Readable && !s.pulling && s.started => {
                let has_read_request =
                    !s.pending_reads.is_empty() || s.body_consumer.is_some() || has_byob_pending;
                let need = (force && s.chunks.is_empty())
                    || has_read_request
                    || (s.chunks.is_empty() && s.high_water_mark > 0.0)
                    || (!s.chunks.is_empty() && s.queue_total_size < s.high_water_mark);
                if need && s.pull_cb != 0 {
                    s.pulling = true;
                    (s.pull_cb, stream_id as f64, true, s.pull_returns_byte_chunk)
                } else {
                    (0, 0.0, false, false)
                }
            }
            _ => (0, 0.0, false, false),
        }
    };
    if !should_pull {
        return;
    }
    let pull = js_closure_alloc(
        perry_runtime::fn_info!(readable_pull_microtask, 0; with_declared(0)),
        3,
    );
    js_closure_set_capture_ptr(pull, 0, controller.to_bits() as i64);
    perry_runtime::closure::closure_set_capture_ref(pull, 1, cb);
    perry_runtime::closure::closure_set_capture_word(
        pull,
        2,
        if pull_returns_byte_chunk { 1 } else { 0 },
    );
    perry_runtime::builtins::js_queue_microtask(pull as i64);
}

pub(super) unsafe fn pull_deferred_byte_chunk(stream_id: usize, cb: i64) {
    let chunk = js_closure_call0(
        cb as *const ClosureHeader,
        perry_runtime::closure::plain_call_receiver(),
    );
    if chunk.to_bits() == TAG_UNDEFINED {
        let _ = js_readable_stream_controller_close(controller_value(stream_id));
    } else {
        let _ = js_readable_stream_controller_enqueue(controller_value(stream_id), chunk);
    }
}

pub(super) unsafe fn close_pending(stream_id: usize) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    native::advance_body(stream_id);
    let promises: Vec<*mut Promise> = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) => s.pending_reads.drain(..).collect(),
            None => Vec::new(),
        }
    };
    // Keep the entire detached batch live while an earlier result allocates.
    let promises: Vec<_> = promises
        .into_iter()
        .map(|p| scope.root_raw_mut_ptr(p))
        .collect();
    for p in promises {
        let result = build_iter_result(TAG_UNDEFINED, true);
        js_promise_resolve(p.get_raw_mut_ptr(), f64::from_bits(result));
    }
    byob::close_pending_byob(stream_id);
    // ReadableStreamClose with the queue drained (or after the cancel
    // algorithm ran): the controller's ClearAlgorithms step.
    readable_clear_algorithms(stream_id);
    // #5437: stream is done — drop any expando entries so the table doesn't
    // grow one row per stream over the server's lifetime.
    expando::stream_expando_clear(stream_id);
    idalloc::retire_readable_terminal(stream_id);
}

pub(super) unsafe fn error_pending(stream_id: usize, reason_bits: u64) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_u64(reason_bits);
    native::advance_body(stream_id);
    let promises: Vec<*mut Promise> = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) => s.pending_reads.drain(..).collect(),
            None => Vec::new(),
        }
    };
    // Keep the entire detached batch live while an earlier result allocates.
    let promises: Vec<_> = promises
        .into_iter()
        .map(|p| scope.root_raw_mut_ptr(p))
        .collect();
    for p in promises {
        js_promise_reject(p.get_raw_mut_ptr(), f64::from_bits(reason.get_nanbox_u64()));
    }
    byob::error_pending_byob(stream_id, reason.get_nanbox_u64());
    // ReadableStreamDefaultControllerError / ReadableByteStreamControllerError
    // clear the source algorithms; on a TransformStream's readable side this
    // is TransformStreamError, which clears the transformer's too.
    readable_clear_algorithms(stream_id);
    if let Some(writable_id) = transform_writable_for_readable(stream_id) {
        transform_clear_algorithms(writable_id);
    }
    // #5437: stream errored — drop any expando entries (see close_pending).
    expando::stream_expando_clear(stream_id);
    idalloc::retire_readable_terminal(stream_id);
}
