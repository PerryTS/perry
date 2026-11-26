// ─────────────────────────────────────────────────────────────────────
// TransformStream FFI
// ─────────────────────────────────────────────────────────────────────

use super::*;
use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder,
};
use flate2::Compression;
use std::io::Read;

/// `new TransformStream(transformer, writableStrategy, readableStrategy)`
/// (#4915): both strategy params accept a plain highWaterMark number, a
/// strategy object (`{ highWaterMark, size }`, e.g. a
/// ByteLengthQueuingStrategy), or undefined.
#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_new(
    start_bits: f64,
    transform_bits: f64,
    flush_bits: f64,
    writable_strategy: f64,
    readable_strategy: f64,
) -> f64 {
    let start_cb = closure_from_bits(start_bits.to_bits());
    let transform_cb = closure_from_bits(transform_bits.to_bits());
    let flush_cb = closure_from_bits(flush_bits.to_bits());
    let writable = parse_strategy_value(writable_strategy);
    let readable = if readable_strategy.to_bits() == TAG_UNDEFINED {
        (0.0, 0)
    } else {
        parse_strategy_value(readable_strategy)
    };
    alloc_transform_stream_with_strategies(
        start_cb,
        transform_cb,
        flush_cb,
        None,
        writable,
        readable,
    )
}

pub(super) unsafe fn alloc_transform_stream(
    start_cb: i64,
    transform_cb: i64,
    flush_cb: i64,
    native: Option<NativeTransformKind>,
    hwm: f64,
) -> f64 {
    alloc_transform_stream_with_strategies(
        start_cb,
        transform_cb,
        flush_cb,
        native,
        (hwm, 0),
        (hwm, 0),
    )
}

unsafe fn alloc_transform_stream_with_strategies(
    start_cb: i64,
    transform_cb: i64,
    flush_cb: i64,
    native: Option<NativeTransformKind>,
    (w_hwm, w_size_cb): (f64, i64),
    (r_hwm, r_size_cb): (f64, i64),
) -> f64 {
    ensure_gc_registered();

    // Allocate the readable side empty (controller is its own handle).
    // The transform's id comes first: its readable side is born owned by it
    // (its controller is a TransformStreamDefaultController).
    let id = next_stream_id();
    let readable_id = alloc_readable_owned(0, 0, 0, r_hwm, false, r_size_cb, Some(id));
    {
        if let Some(s) = rs_rec(RsRef(readable_id)).as_deref_mut() {
            s.started = true;
            s.high_water_mark = if r_hwm.is_nan() { 0.0 } else { r_hwm.max(0.0) };
        }
    }

    // Allocate writable side; its write_cb is synthesized via the
    // dispatcher table below to invoke transform(chunk, controller).
    let writable_id = next_stream_id();
    let ready = internal_promise();
    let closed = internal_promise();
    js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    // Sentinel: write_cb=0, close_cb=0 — the dispatcher checks the writable's
    // `owner_transform` first and routes through the user transform_cb /
    // flush_cb instead.
    let mut writable = WritableStreamData::new(
        0,
        0,
        0,
        w_size_cb,
        if w_hwm.is_nan() || w_hwm <= 0.0 {
            1.0
        } else {
            w_hwm
        },
        ready,
        closed,
    );

    writable.owner_transform = Some(id);
    insert_ws(WsRef(writable_id), writable);
    insert_ts(
        TsRef(id),
        TransformStreamData {
            props: Default::default(),
            readable: stream_value(readable_id).to_bits(),
            writable_handle: writable_id,
            transform_cb,
            flush_cb,
            native,
            backpressure: r_hwm <= 0.0,
            expando: Vec::new(),
            controller_expando: Vec::new(),
        },
    );

    // #1644: TransformStream `start(controller)` fires synchronously at
    // construction. The controller is the readable-side handle (same value the
    // transform/flush callbacks receive), so `controller.enqueue(c)` /
    // `controller.terminate()` / `controller.error(e)` act on the readable.
    if start_cb != 0 {
        js_closure_call1(
            start_cb as *const ClosureHeader,
            perry_runtime::closure::plain_call_receiver(),
            controller_value(readable_id),
        );
    }
    stream_value(id)
}

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_new_from_transformer_object(
    transformer: f64,
    writable_strategy: f64,
    readable_strategy: f64,
) -> f64 {
    ensure_gc_registered();
    js_transform_stream_new(
        stream_object_field(transformer, b"start"),
        stream_object_field(transformer, b"transform"),
        stream_object_field(transformer, b"flush"),
        writable_strategy,
        readable_strategy,
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_readable(handle: f64) -> f64 {
    let id = stream_id(handle);
    ts_rec(TsRef(id))
        .as_deref()
        .map(|t| stream_value(t.readable_handle()))
        .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub unsafe extern "C" fn js_transform_stream_writable(handle: f64) -> f64 {
    let id = stream_id(handle);
    ts_rec(TsRef(id))
        .as_deref()
        .map(|t| stream_value(t.writable_handle))
        .unwrap_or(f64::from_bits(TAG_UNDEFINED))
}

pub(super) fn transform_writable_for_readable(readable_id: usize) -> Option<usize> {
    let transform = transform_of_readable(RsRef(readable_id))?;
    with_ts(transform, |t| t.writable_handle)
}

/// Replacement `writer.write` for the writable side of a TransformStream
/// — invokes the user transform with (chunk, transformController) where
/// the transformController is the readable-side stream handle (so
/// `controller.enqueue(...)` reuses the readable controller path).
pub(super) unsafe fn transform_write(writable_id: usize, chunk: f64) -> *mut Promise {
    let promise = js_promise_new();
    {
        match ws_rec(WsRef(writable_id)).as_deref() {
            Some(s) if s.state == WritableState::Writable => {}
            Some(s) if s.state == WritableState::Errored => {
                js_promise_reject(promise, f64::from_bits(s.error_value));
                return promise;
            }
            _ => {
                let err = make_error_with_message("Stream is closed or closing");
                js_promise_reject(promise, f64::from_bits(err));
                return promise;
            }
        }
    }
    let mut handled_native = false;
    let mut native_error = None;
    let (transform_cb, readable_id) = {
        match transform_of_writable(WsRef(writable_id)) {
            Some(t_id) => match ts_rec(t_id).as_deref_mut() {
                Some(t) => {
                    let readable = t.readable_handle();
                    if let Some(native) = t.native.as_mut() {
                        handled_native = true;
                        if let Err(error_bits) = native_transform_write(native, readable, chunk) {
                            native_error = Some(error_bits);
                        }
                    }
                    (t.transform_cb, t.readable_handle())
                }
                None => (0, 0),
            },
            None => (0, 0),
        }
    };
    if let Some(error_bits) = native_error {
        if readable_id != 0 {
            js_readable_stream_controller_error(
                controller_value(readable_id),
                f64::from_bits(error_bits),
            );
        }
        js_promise_reject(promise, f64::from_bits(error_bits));
        return promise;
    }
    if handled_native {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
        return promise;
    }
    // Spec/Node tick parity: the transformer's `transform()` runs as a promise
    // JOB (TransformStreamDefaultSinkWriteAlgorithm chains PerformTransform),
    // never synchronously inside `writer.write()`. Running it inline made the
    // transform side of a piped tee one tick per chunk — faster than Node's
    // cadence (teepipe2.js) — which let a tee sibling's reader outrun the
    // pipeline (Next.js cold-start head reorder). Queue the invocation; the
    // write promise resolves in the job, after the transform enqueues.
    let job = perry_runtime::closure::js_closure_alloc(
        perry_runtime::fn_info!(transform_write_job, 0; with_declared(0)),
        5,
    );
    perry_runtime::closure::closure_set_capture_ref(job, 0, transform_cb);
    super::capture_stream(job, 1, readable_id);
    perry_runtime::closure::js_closure_set_capture_ptr(job, 2, chunk.to_bits() as i64);
    perry_runtime::closure::closure_set_capture_ref(job, 3, promise as i64);
    super::capture_stream(job, 4, writable_id);
    // #6607: a `writer.close()` in the same synchronous run must not close the
    // readable ahead of this queued job — count it so transform_close defers.
    with_ws(WsRef(writable_id), |s| s.transform_pending_writes += 1);
    if transform_initial_backpressure(readable_id) {
        with_rs(RsRef(readable_id), |s| s.defer_job(job));
    } else {
        perry_runtime::builtins::js_queue_microtask(job as i64);
    }
    promise
}

/// First hop: the sink write algorithm job. Node's write path costs two jobs
/// before the transformer runs (sink write algorithm → PerformTransform);
/// teepipe2.js shows the transform output landing one tick later than a
/// single-job deferral produces. Re-queue once, then run the transform.
extern "C" fn transform_write_job(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    {
        let job = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(transform_write_job2, 0; with_declared(0)),
            5,
        );
        for i in 0..5 {
            perry_runtime::closure::js_closure_set_capture_ptr(
                job,
                i,
                perry_runtime::closure::js_closure_get_capture_ptr(closure, i),
            );
        }
        perry_runtime::builtins::js_queue_microtask(job as i64);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_write_job2(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    unsafe {
        let transform_cb = perry_runtime::closure::closure_capture_ref(closure, 0);
        let readable_id = super::captured_stream(closure, 1);
        let chunk_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 2) as u64;
        let promise = perry_runtime::closure::closure_capture_ref(closure, 3) as *mut Promise;
        let writable_id = super::captured_stream(closure, 4);
        let chunk = f64::from_bits(chunk_bits);
        if transform_cb != 0 && readable_id != 0 {
            let ret = js_closure_call2(
                transform_cb as *const ClosureHeader,
                perry_runtime::closure::plain_call_receiver(),
                chunk,
                controller_value(readable_id),
            );
            // Spec PerformTransform: `Promise.resolve(transformer.transform(
            // chunk, controller))` — an async transform's returned promise
            // gates the write's settlement. Next's buffered transform returns
            // a setImmediate-deferred flush promise; ignoring it let Perry's
            // pipe run a macrotask ahead of Node per write.
            if perry_runtime::promise::js_value_is_promise(ret) != 0 {
                let inner = perry_runtime::value::js_nanbox_get_pointer(ret) as *mut Promise;
                if !inner.is_null() {
                    let fc = perry_runtime::closure::js_closure_alloc(
                        perry_runtime::fn_info!(transform_write_settle_fulfilled, 1; with_declared(1)),
                        3,
                    );
                    super::capture_stream(fc, 0, readable_id);
                    perry_runtime::closure::closure_set_capture_ref(fc, 1, promise as i64);
                    super::capture_stream(fc, 2, writable_id);
                    let rc = perry_runtime::closure::js_closure_alloc(
                        perry_runtime::fn_info!(transform_write_settle_rejected, 1; with_declared(1)),
                        3,
                    );
                    super::capture_stream(rc, 0, readable_id);
                    perry_runtime::closure::closure_set_capture_ref(rc, 1, promise as i64);
                    super::capture_stream(rc, 2, writable_id);
                    let _ = perry_runtime::promise::js_promise_then(inner, fc, rc);
                    return f64::from_bits(TAG_UNDEFINED);
                }
            }
        } else if readable_id != 0 {
            // Identity transform — pass-through.
            js_readable_stream_controller_enqueue(controller_value(readable_id), chunk);
        }
        // Spec backpressure (TransformStreamDefaultSinkWriteAlgorithm): the
        // readable side's default highWaterMark is 0, so any chunk sitting
        // undrained in its queue keeps backpressure ON and the write promise
        // PENDING until the consumer catches up (a read pops the queue empty
        // or parks on it). Resolving unconditionally let a `pipeTo` upstream
        // keep pulling at full speed — in a teed pipeline the fast pipe
        // pre-filled the sibling branch's queue and reordered promise chains
        // racing the stream (Next.js cold-start head reorder: node's pipe is
        // stalled here, so the flight branch's read #3 stays pending while
        // the module-require chain finishes).
        settle_transform_write(readable_id, promise);
        transform_write_job_done(writable_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Settle a transform write with spec semantics: reject when the readable
/// side has errored (the TransformStream is errored — writes must not report
/// success), park on backpressure while undrained chunks remain, else resolve.
unsafe fn settle_transform_write(readable_id: usize, promise: *mut Promise) {
    enum Settle {
        Errored(u64),
        Park,
        Resolve,
    }
    let initial_backpressure = transform_initial_backpressure(readable_id);
    let settle = {
        match rs_rec(RsRef(readable_id)).as_deref() {
            Some(s) if s.state == ReadableState::Errored => Settle::Errored(s.error_value()),
            Some(s) if transform_has_backpressure(s, initial_backpressure) => Settle::Park,
            _ => Settle::Resolve,
        }
    };
    match settle {
        Settle::Errored(reason) => js_promise_reject(promise, f64::from_bits(reason)),
        Settle::Park => {
            with_rs(RsRef(readable_id), |s| s.park_write(promise));
        }
        Settle::Resolve => js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED)),
    }
}

/// #6607: a queued transform write job finished delivering its chunk. When the
/// last pending job for the writable drains, run any close request that
/// arrived while the jobs were still queued (see `transform_close`).
unsafe fn transform_write_job_done(writable_id: usize) {
    let drained = with_ws(WsRef(writable_id), |s| match s.transform_pending_writes {
        0 => true,
        n => {
            s.transform_pending_writes = n - 1;
            n == 1
        }
    })
    .unwrap_or(true);
    if !drained {
        return;
    }
    let deferred = with_ws(WsRef(writable_id), |s| s.transform_pending_close.take()).flatten();
    if let Some(promise) = deferred {
        // The stream may have errored (writer.abort(), controller.error(...))
        // while the close waited on the jobs — settle the close request with
        // that error instead of running flush on an errored stream.
        let errored = ws_rec(WsRef(writable_id))
            .as_deref()
            .and_then(|s| (s.state == WritableState::Errored).then_some(s.error_value));
        match errored {
            Some(reason) => js_promise_reject(promise, f64::from_bits(reason)),
            None => perform_transform_close(writable_id, promise),
        }
    }
}

/// The async transformer's returned promise fulfilled — settle the write with
/// the same backpressure gate as the synchronous path.
extern "C" fn transform_write_settle_fulfilled(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _value: f64,
) -> f64 {
    unsafe {
        let readable_id = super::captured_stream(closure, 0);
        let promise = perry_runtime::closure::closure_capture_ref(closure, 1) as *mut Promise;
        let writable_id = super::captured_stream(closure, 2);
        settle_transform_write(readable_id, promise);
        transform_write_job_done(writable_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// The async transformer's returned promise rejected — error the readable side
/// and reject the write (spec: transformResultPromise rejection errors the
/// TransformStream).
extern "C" fn transform_write_settle_rejected(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    reason: f64,
) -> f64 {
    unsafe {
        let readable_id = super::captured_stream(closure, 0);
        let promise = perry_runtime::closure::closure_capture_ref(closure, 1) as *mut Promise;
        let writable_id = super::captured_stream(closure, 2);
        if readable_id != 0 {
            js_readable_stream_controller_error(controller_value(readable_id), reason);
        }
        js_promise_reject(promise, reason);
        transform_write_job_done(writable_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Consumer progress on a transform's readable side: release parked write
/// promises once its queue has drained (spec: the source pull algorithm sets
/// backpressure = false). Called from the reader-read paths in `streams.rs`.
pub(super) unsafe fn transform_release_writes(readable_id: usize) {
    set_transform_initial_backpressure(readable_id, false);
    // Detached arrays below are locals (stack-pinned with their entries).
    let jobs = with_rs(RsRef(readable_id), |s| s.take_deferred_jobs())
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    for k in 0..objects::array_len(jobs) {
        let job = closure_of_value(objects::array_get(jobs, k));
        perry_runtime::builtins::js_queue_microtask(job as i64);
    }
    let (backpressured, errored) = {
        match rs_rec(RsRef(readable_id)).as_deref() {
            Some(s) => (
                transform_has_backpressure(s, false),
                (s.state == ReadableState::Errored).then_some(s.error_value()),
            ),
            None => (false, None),
        }
    };
    if backpressured && errored.is_none() {
        return;
    }
    let parked = with_rs(RsRef(readable_id), |s| s.take_parked_writes())
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    for k in 0..objects::array_len(parked) {
        let p = state::promise_of(objects::array_get(parked, k));
        match errored {
            Some(reason) => js_promise_reject(p, f64::from_bits(reason)),
            None => js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED)),
        }
    }
}

pub(super) unsafe fn transform_abort_pending_writes(writable_id: usize, reason: f64) {
    let readable_id = transform_of_writable(WsRef(writable_id))
        .and_then(|transform| with_ts(transform, |stream| stream.readable_handle()));
    let Some(readable_id) = readable_id else {
        return;
    };
    let jobs = with_rs(RsRef(readable_id), |s| s.take_deferred_jobs())
        .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    for k in 0..objects::array_len(jobs) {
        let job = closure_of_value(objects::array_get(jobs, k));
        let promise = perry_runtime::closure::closure_capture_ref(job, 3) as *mut Promise;
        js_promise_reject(promise, reason);
        transform_write_job_done(writable_id);
    }
}

/// A closure value back as its raw pointer.
fn closure_of_value(value: f64) -> *mut ClosureHeader {
    state::promise_of(value) as *mut ClosureHeader
}

fn transform_has_backpressure(stream: &ReadableStreamData, initial: bool) -> bool {
    initial || (!stream.chunks_empty() && stream.queue_total_size >= stream.high_water_mark)
}

fn transform_initial_backpressure(readable_id: usize) -> bool {
    transform_of_readable(RsRef(readable_id))
        .and_then(|transform| with_ts(transform, |stream| stream.backpressure))
        .unwrap_or(false)
}

fn set_transform_initial_backpressure(readable_id: usize, value: bool) {
    if let Some(transform) = transform_of_readable(RsRef(readable_id)) {
        with_ts(transform, |stream| stream.backpressure = value);
    }
}

#[cfg(test)]
mod backpressure_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TRANSFORM_CALLS: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn capture_transform(
        _closure: *const ClosureHeader,
        _this: perry_runtime::closure::JsThis,
        _chunk: f64,
        _controller: f64,
    ) -> f64 {
        TRANSFORM_CALLS.fetch_add(1, Ordering::Relaxed);
        f64::from_bits(TAG_UNDEFINED)
    }

    #[test]
    fn default_transform_starts_backpressured_until_read_demand() {
        let _serial = crate::streams::tests::serial_guard();
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let transform = unsafe {
            js_transform_stream_new(undefined, undefined, undefined, undefined, undefined)
        };
        let readable = stream_id(unsafe { js_transform_stream_readable(transform) });

        assert!(transform_initial_backpressure(readable));
        unsafe { transform_release_writes(readable) };
        assert!(!transform_initial_backpressure(readable));
    }

    #[test]
    fn initial_backpressure_defers_transform_until_read_demand() {
        let _serial = crate::streams::tests::serial_guard();
        TRANSFORM_CALLS.store(0, Ordering::Relaxed);
        let transform = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(capture_transform, 2; with_declared(2)),
            0,
        );
        let transform_value = f64::from_bits(JSValue::pointer(transform as *const u8).bits());
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let stream = unsafe {
            js_transform_stream_new(undefined, transform_value, undefined, undefined, undefined)
        };
        let readable = stream_id(unsafe { js_transform_stream_readable(stream) });
        let writable = stream_id(unsafe { js_transform_stream_writable(stream) });
        let write = unsafe { transform_write(writable, 1.0) };

        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(TRANSFORM_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(perry_runtime::promise::js_promise_state(write), 0);

        unsafe { transform_release_writes(readable) };
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(TRANSFORM_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(perry_runtime::promise::js_promise_state(write), 1);
    }

    #[test]
    fn abort_rejects_transform_waiting_for_read_demand() {
        let _serial = crate::streams::tests::serial_guard();
        TRANSFORM_CALLS.store(0, Ordering::Relaxed);
        let transform = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(capture_transform, 2; with_declared(2)),
            0,
        );
        let transform_value = f64::from_bits(JSValue::pointer(transform as *const u8).bits());
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let stream = unsafe {
            js_transform_stream_new(undefined, transform_value, undefined, undefined, undefined)
        };
        let writable = stream_id(unsafe { js_transform_stream_writable(stream) });
        let write = unsafe { transform_write(writable, 1.0) };

        unsafe {
            super::writable::js_writable_stream_abort_inner(stream_value(writable), 7.0, true);
        }
        perry_runtime::promise::js_promise_run_microtasks();
        assert_eq!(TRANSFORM_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(perry_runtime::promise::js_promise_state(write), 2);
        assert_eq!(perry_runtime::promise::js_promise_reason(write), 7.0);
    }
}

pub(super) unsafe fn transform_close(writable_id: usize) -> *mut Promise {
    // #6607 (WHATWG TransformStreamDefaultSinkCloseAlgorithm ordering): the
    // sink close runs only after queued writes complete. `transform_write`
    // defers the transformer invocation through a two-hop microtask job, so a
    // same-run `writer.close()` would otherwise close the readable before the
    // jobs deliver their chunks — controller_enqueue would then silently drop
    // them on a Closed readable. Defer the whole close (flush included)
    // behind the pending jobs; `transform_write_job_done` resumes it.
    if let Some(pending) = with_ws(WsRef(writable_id), |s| s.transform_pending_close).flatten() {
        return pending;
    }
    let promise = js_promise_new();
    let jobs_pending = with_ws(WsRef(writable_id), |s| s.transform_pending_writes).unwrap_or(0) > 0;
    if jobs_pending {
        with_ws(WsRef(writable_id), |s| {
            s.transform_pending_close = Some(promise)
        });
        // Close is requested: writes arriving during the wait must reject
        // ("Stream is closed or closing"), same as the plain-writable path.
        if let Some(s) = ws_rec(WsRef(writable_id)).as_deref_mut() {
            if s.state == WritableState::Writable {
                s.state = WritableState::Closing;
            }
        }
        return promise;
    }
    perform_transform_close(writable_id, promise);
    promise
}

unsafe fn perform_transform_close(writable_id: usize, promise: *mut Promise) {
    let mut handled_native = false;
    let mut native_error = None;
    let (flush_cb, readable_id) = {
        match transform_of_writable(WsRef(writable_id)) {
            Some(t_id) => match ts_rec(t_id).as_deref_mut() {
                Some(t) => {
                    let readable = t.readable_handle();
                    if let Some(native) = t.native.as_mut() {
                        handled_native = true;
                        if let Err(error_bits) = native_transform_close(native, readable) {
                            native_error = Some(error_bits);
                        }
                    }
                    (t.flush_cb, t.readable_handle())
                }
                None => (0, 0),
            },
            None => (0, 0),
        }
    };
    // Writable ProcessClose: the close algorithm (TransformStreamDefaultSink-
    // CloseAlgorithm: flush, then TransformStreamDefaultControllerClear-
    // Algorithms) runs, then the writable's ClearAlgorithms.
    let clear_close_algorithms = || {
        transform_clear_algorithms(writable_id);
        writable_clear_algorithms(writable_id);
    };
    if let Some(error_bits) = native_error {
        clear_close_algorithms();
        if readable_id != 0 {
            js_readable_stream_controller_error(
                controller_value(readable_id),
                f64::from_bits(error_bits),
            );
        }
        if let Some(s) = ws_rec(WsRef(writable_id)).as_deref_mut() {
            s.state = WritableState::Errored;
            s.error_value = error_bits;
            let cp = s.closed_promise;
            js_promise_reject(cp, f64::from_bits(error_bits));
        }
        super::idalloc::retire_writable_terminal(writable_id);
        js_promise_reject(promise, f64::from_bits(error_bits));
        return;
    }
    // Invoke the user `flush(controller)`. It may return a promise: e.g.
    // Next.js' `createBufferedTransformStream` buffers chunks in `transform()`
    // and enqueues the coalesced chunk from a `setImmediate`, with `flush()`
    // returning a promise that resolves only once that deferred enqueue runs.
    // Per the WHATWG spec the transformer's flush completion is awaited before
    // the readable side is closed; closing early drops the not-yet-enqueued
    // chunk (the #5989 Next.js `bytes=0` empty-body bug — the buffered HTML
    // never reached `res.write`). So when flush returns a still-pending
    // promise, defer the readable close until it settles.
    let flush_result = if !handled_native && flush_cb != 0 && readable_id != 0 {
        Some(js_closure_call1(
            flush_cb as *const ClosureHeader,
            perry_runtime::closure::plain_call_receiver(),
            controller_value(readable_id),
        ))
    } else {
        None
    };
    clear_close_algorithms();

    if let Some(fv) = flush_result {
        if perry_runtime::promise::js_value_is_promise(fv) != 0 {
            let inner = js_nanbox_get_pointer(fv) as *mut Promise;
            if !inner.is_null() {
                match perry_runtime::promise::js_promise_state(inner) {
                    // Already fulfilled — nothing deferred; close synchronously.
                    1 => {}
                    // Rejected — error the readable and reject the close request.
                    2 => {
                        let reason = perry_runtime::promise::js_promise_reason(inner);
                        error_transform_close(writable_id, readable_id, promise, reason.to_bits());
                        return;
                    }
                    // Pending — chain the close onto flush's settlement so the
                    // deferred enqueue lands on the still-open readable first.
                    _ => {
                        let fulfill = perry_runtime::closure::js_closure_alloc(
                            perry_runtime::fn_info!(transform_flush_fulfilled, 1),
                            3,
                        );
                        let reject = perry_runtime::closure::js_closure_alloc(
                            perry_runtime::fn_info!(transform_flush_rejected, 1),
                            3,
                        );
                        super::capture_stream(fulfill, 0, writable_id);
                        super::capture_stream(fulfill, 1, readable_id);
                        perry_runtime::closure::closure_set_capture_ref(fulfill, 2, promise as i64);
                        super::capture_stream(reject, 0, writable_id);
                        super::capture_stream(reject, 1, readable_id);
                        perry_runtime::closure::closure_set_capture_ref(reject, 2, promise as i64);
                        let _ = perry_runtime::promise::js_promise_then(inner, fulfill, reject);
                        return;
                    }
                }
            }
        }
    }

    finish_transform_close(writable_id, readable_id, promise);
}

/// Close a TransformStream's writable side once `flush()` has settled: close
/// the readable (delivering any chunk flush enqueued, possibly from a deferred
/// `setImmediate`), mark the writable closed, and resolve both its closed
/// promise and the close-request promise the caller (pipeTo/writer.close) is
/// awaiting.
unsafe fn finish_transform_close(
    writable_id: usize,
    readable_id: usize,
    close_promise: *mut Promise,
) {
    if readable_id != 0 {
        js_readable_stream_controller_close(controller_value(readable_id));
    }
    if let Some(s) = ws_rec(WsRef(writable_id)).as_deref_mut() {
        s.state = WritableState::Closed;
        let cp = s.closed_promise;
        js_promise_resolve(cp, f64::from_bits(TAG_UNDEFINED));
    }
    super::idalloc::retire_writable_terminal(writable_id);
    js_promise_resolve(close_promise, f64::from_bits(TAG_UNDEFINED));
}

/// Error a TransformStream's writable side when `flush()` rejected: error the
/// readable, mark the writable errored, and reject both the closed promise and
/// the close-request promise.
unsafe fn error_transform_close(
    writable_id: usize,
    readable_id: usize,
    close_promise: *mut Promise,
    reason_bits: u64,
) {
    if readable_id != 0 {
        js_readable_stream_controller_error(
            controller_value(readable_id),
            f64::from_bits(reason_bits),
        );
    }
    if let Some(s) = ws_rec(WsRef(writable_id)).as_deref_mut() {
        s.state = WritableState::Errored;
        s.error_value = reason_bits;
        let cp = s.closed_promise;
        js_promise_reject(cp, f64::from_bits(reason_bits));
    }
    super::idalloc::retire_writable_terminal(writable_id);
    js_promise_reject(close_promise, f64::from_bits(reason_bits));
}

extern "C" fn transform_flush_fulfilled(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    _value: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let writable_id = super::captured_stream(closure, 0);
        let readable_id = super::captured_stream(closure, 1);
        let close_promise = perry_runtime::closure::closure_capture_ref(closure, 2) as *mut Promise;
        finish_transform_close(writable_id, readable_id, close_promise);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn transform_flush_rejected(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    reason: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let writable_id = super::captured_stream(closure, 0);
        let readable_id = super::captured_stream(closure, 1);
        let close_promise = perry_runtime::closure::closure_capture_ref(closure, 2) as *mut Promise;
        error_transform_close(writable_id, readable_id, close_promise, reason.to_bits());
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) fn split_utf8_prefix(bytes: &[u8]) -> Result<(usize, bool), ()> {
    match std::str::from_utf8(bytes) {
        Ok(_) => Ok((bytes.len(), false)),
        Err(err) => {
            if err.error_len().is_none() {
                Ok((err.valid_up_to(), true))
            } else {
                Err(())
            }
        }
    }
}

unsafe fn enqueue_string(readable_id: usize, text: &str) {
    let ptr = js_string_from_bytes(text.as_ptr(), text.len() as u32);
    js_readable_stream_controller_enqueue(
        controller_value(readable_id),
        f64::from_bits(JSValue::string_ptr(ptr).bits()),
    );
}

unsafe fn native_text_decoder_drain(
    pending: &mut Vec<u8>,
    fatal: bool,
    readable_id: usize,
    flush: bool,
) -> Result<(), u64> {
    if pending.is_empty() {
        return Ok(());
    }
    if fatal {
        let (valid_len, incomplete) = split_utf8_prefix(pending).map_err(|_| {
            make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            )
        })?;
        if valid_len > 0 {
            let text = std::str::from_utf8(&pending[..valid_len]).map_err(|_| {
                make_type_error_with_code(
                    "The encoded data was not valid for encoding utf-8",
                    "ERR_ENCODING_INVALID_ENCODED_DATA",
                )
            })?;
            enqueue_string(readable_id, text);
            pending.drain(..valid_len);
        }
        if flush && !pending.is_empty() {
            return Err(make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            ));
        }
        if !incomplete && !pending.is_empty() {
            return Err(make_type_error_with_code(
                "The encoded data was not valid for encoding utf-8",
                "ERR_ENCODING_INVALID_ENCODED_DATA",
            ));
        }
        return Ok(());
    }

    if flush {
        let text = String::from_utf8_lossy(pending).into_owned();
        pending.clear();
        if !text.is_empty() {
            enqueue_string(readable_id, &text);
        }
        return Ok(());
    }

    let (valid_len, incomplete) = split_utf8_prefix(pending).unwrap_or((pending.len(), false));
    let emit_len = if incomplete { valid_len } else { pending.len() };
    if emit_len > 0 {
        let text = String::from_utf8_lossy(&pending[..emit_len]).into_owned();
        enqueue_string(readable_id, &text);
        pending.drain(..emit_len);
    }
    Ok(())
}

pub(super) fn run_web_compression_codec(
    format: WebCompressionFormat,
    decompress: bool,
    input: &[u8],
) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match (format, decompress) {
        (WebCompressionFormat::Gzip, false) => {
            GzEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Gzip, true) => {
            GzDecoder::new(input).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Deflate, false) => {
            ZlibEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::Deflate, true) => {
            ZlibDecoder::new(input).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::DeflateRaw, false) => {
            DeflateEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        (WebCompressionFormat::DeflateRaw, true) => {
            DeflateDecoder::new(input).read_to_end(&mut out)?;
        }
        #[cfg(feature = "streams-brotli")]
        (WebCompressionFormat::Brotli, false) => {
            let mut reader = brotli::CompressorReader::new(input, 4096, 11, 22);
            reader.read_to_end(&mut out)?;
        }
        #[cfg(feature = "streams-brotli")]
        (WebCompressionFormat::Brotli, true) => {
            let mut reader = brotli::Decompressor::new(input, 4096);
            reader.read_to_end(&mut out)?;
        }
        // Unreachable through JS: `parse_web_compression_format` refuses
        // "brotli" at construction when the codec is not compiled in. Kept so
        // the match stays exhaustive and a future direct caller gets an error,
        // not silently empty output.
        #[cfg(not(feature = "streams-brotli"))]
        (WebCompressionFormat::Brotli, _) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Brotli CompressionStream support is not compiled into this binary",
            ));
        }
    }
    Ok(out)
}

unsafe fn native_transform_write(
    native: &mut NativeTransformKind,
    readable_id: usize,
    chunk: f64,
) -> Result<(), u64> {
    match native {
        NativeTransformKind::TextEncoder => {
            let text = js_string_value_to_string(chunk, true).unwrap_or_default();
            let bytes = alloc_uint8array_from_bytes(text.as_bytes());
            js_readable_stream_controller_enqueue(
                controller_value(readable_id),
                f64::from_bits(bytes),
            );
            Ok(())
        }
        NativeTransformKind::TextDecoder { fatal, pending } => {
            let bytes = read_bytes_from_chunk(chunk.to_bits()).ok_or_else(|| {
                make_type_error_with_code(
                    "The \"chunk\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                    "ERR_INVALID_ARG_TYPE",
                )
            })?;
            pending.extend_from_slice(&bytes);
            native_text_decoder_drain(pending, *fatal, readable_id, false)
        }
        NativeTransformKind::Compression { chunks, .. } => {
            let bytes = read_bytes_from_chunk(chunk.to_bits()).ok_or_else(|| {
                make_type_error_with_code(
                    "The \"chunk\" argument must be an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                    "ERR_INVALID_ARG_TYPE",
                )
            })?;
            chunks.extend_from_slice(&bytes);
            Ok(())
        }
    }
}

unsafe fn native_transform_close(
    native: &mut NativeTransformKind,
    readable_id: usize,
) -> Result<(), u64> {
    match native {
        NativeTransformKind::TextEncoder => Ok(()),
        NativeTransformKind::TextDecoder { fatal, pending } => {
            native_text_decoder_drain(pending, *fatal, readable_id, true)
        }
        NativeTransformKind::Compression {
            format,
            decompress,
            chunks,
        } => match run_web_compression_codec(*format, *decompress, chunks) {
            Ok(out) => {
                let chunk = alloc_uint8array_from_bytes(&out);
                js_readable_stream_controller_enqueue(
                    controller_value(readable_id),
                    f64::from_bits(chunk),
                );
                chunks.clear();
                Ok(())
            }
            Err(err) => Err(make_type_error_with_code(&err.to_string(), "Z_DATA_ERROR")),
        },
    }
}

unsafe fn attach_stream_field(object_value: f64, name: &[u8], value: f64) {
    let ptr = js_nanbox_get_pointer(object_value) as *mut ObjectHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return;
    }
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(ptr, key, value);
}

unsafe fn attach_stream_string_field(object_value: f64, name: &[u8], value: &[u8]) {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    attach_stream_field(
        object_value,
        name,
        f64::from_bits(JSValue::string_ptr(ptr).bits()),
    );
}

unsafe fn attach_stream_bool_field(object_value: f64, name: &[u8], value: bool) {
    attach_stream_field(
        object_value,
        name,
        f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE }),
    );
}

unsafe fn attach_transform_endpoints(object_value: f64, readable_id: usize, writable_id: usize) {
    attach_stream_field(object_value, b"readable", stream_value(readable_id));
    attach_stream_field(object_value, b"writable", stream_value(writable_id));
}

unsafe fn bool_option(options: f64, name: &[u8]) -> bool {
    let jsval = JSValue::from_bits(options.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let value =
        perry_runtime::value::js_get_property(options, name.as_ptr() as i64, name.len() as i64);
    perry_runtime::value::js_is_truthy(value) != 0
}

unsafe fn parse_text_decoder_stream_label(label: f64) {
    let jsval = JSValue::from_bits(label.to_bits());
    if jsval.is_undefined()
        || value_string_equals(label, b"utf-8")
        || value_string_equals(label, b"utf8")
    {
        return;
    }
    let label_text = js_string_value_to_string(label, true).unwrap_or_default();
    let message = format!("The \"{label_text}\" encoding is not supported");
    throw_range_error_with_code(&message, "ERR_ENCODING_NOT_SUPPORTED");
}

unsafe fn parse_web_compression_format(value: f64, constructor_name: &str) -> WebCompressionFormat {
    if value_string_equals(value, b"gzip") {
        return WebCompressionFormat::Gzip;
    }
    if value_string_equals(value, b"deflate") {
        return WebCompressionFormat::Deflate;
    }
    if value_string_equals(value, b"deflate-raw") {
        return WebCompressionFormat::DeflateRaw;
    }
    if value_string_equals(value, b"brotli") {
        #[cfg(feature = "streams-brotli")]
        return WebCompressionFormat::Brotli;
        // Fail LOUDLY at construction rather than on the first write. Should be
        // unreachable: auto-optimize enables `streams-brotli` whenever the
        // program references CompressionStream/DecompressionStream or has
        // deferred dynamic code. If it is ever hit, that detection missed.
        #[cfg(not(feature = "streams-brotli"))]
        throw_type_error_with_code(
            &format!(
                "Failed to construct '{constructor_name}': Brotli support is not compiled into this binary (perry-stdlib feature `streams-brotli`)"
            ),
            "ERR_FEATURE_UNAVAILABLE_ON_PLATFORM",
        );
    }
    let received =
        js_string_value_to_string(value, true).unwrap_or_else(|| "undefined".to_string());
    let message = format!(
        "Failed to construct '{constructor_name}': 1st argument value '{received}' is not a valid enum value of type CompressionFormat."
    );
    throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

unsafe fn build_native_transform_object(object_value: f64, native: NativeTransformKind) -> f64 {
    let handle = alloc_transform_stream(0, 0, 0, Some(native), 1.0);
    let readable_id = stream_id(js_transform_stream_readable(handle));
    let writable_id = stream_id(js_transform_stream_writable(handle));
    attach_transform_endpoints(object_value, readable_id, writable_id);
    object_value
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_text_encoder_stream_new() -> f64 {
    let object = perry_runtime::object::js_text_encoder_stream_new();
    attach_stream_string_field(object, b"encoding", b"utf-8");
    build_native_transform_object(object, NativeTransformKind::TextEncoder)
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_text_decoder_stream_new(label: f64, options: f64) -> f64 {
    parse_text_decoder_stream_label(label);
    let fatal = bool_option(options, b"fatal");
    let ignore_bom = bool_option(options, b"ignoreBOM");
    let object = perry_runtime::object::js_text_decoder_stream_new();
    attach_stream_string_field(object, b"encoding", b"utf-8");
    attach_stream_bool_field(object, b"fatal", fatal);
    attach_stream_bool_field(object, b"ignoreBOM", ignore_bom);
    build_native_transform_object(
        object,
        NativeTransformKind::TextDecoder {
            fatal,
            pending: Vec::new(),
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_compression_stream_new(format: f64) -> f64 {
    let format = parse_web_compression_format(format, "CompressionStream");
    let object = perry_runtime::object::js_compression_stream_new();
    build_native_transform_object(
        object,
        NativeTransformKind::Compression {
            format,
            decompress: false,
            chunks: Vec::new(),
        },
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_stream_web_decompression_stream_new(format: f64) -> f64 {
    let format = parse_web_compression_format(format, "DecompressionStream");
    let object = perry_runtime::object::js_decompression_stream_new();
    build_native_transform_object(
        object,
        NativeTransformKind::Compression {
            format,
            decompress: true,
            chunks: Vec::new(),
        },
    )
}

// BYOB readers and ByteLengthQueuingStrategy accounting are implemented
// (#4915) — the old `js_streams_throw_*_not_implemented` stubs are gone.
