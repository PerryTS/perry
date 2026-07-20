// ─────────────────────────────────────────────────────────────────────
// WritableStream FFI
// ─────────────────────────────────────────────────────────────────────

use super::*;

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new(
    start_bits: f64,
    write_bits: f64,
    close_bits: f64,
    abort_bits: f64,
    hwm: f64,
) -> f64 {
    js_writable_stream_new_with_sink_type(
        start_bits,
        write_bits,
        close_bits,
        abort_bits,
        hwm,
        f64::from_bits(TAG_UNDEFINED),
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new_with_sink_type(
    start_bits: f64,
    write_bits: f64,
    close_bits: f64,
    abort_bits: f64,
    hwm: f64,
    sink_type: f64,
) -> f64 {
    if sink_type.to_bits() != TAG_UNDEFINED {
        throw_range_error_with_code(
            "The argument 'type' is invalid. Received a non-undefined value",
            "ERR_INVALID_ARG_VALUE",
        );
    }

    ensure_gc_registered();
    // `hwm` may be a plain number or a whole strategy object (#4915).
    let (hwm, size_cb) = parse_strategy_value(hwm);
    let id = alloc_writable_with_strategy(
        closure_from_bits(write_bits.to_bits()),
        closure_from_bits(close_bits.to_bits()),
        closure_from_bits(abort_bits.to_bits()),
        hwm,
        size_cb,
    );
    // #1545: WritableStream `start(controller)` fires synchronously at
    // construction (before any write), matching the WHATWG order
    // start → write → close. The controller arg is the stream handle.
    let start_cb = closure_from_bits(start_bits.to_bits());
    if start_cb != 0 {
        js_closure_call1(start_cb as *const ClosureHeader, id as f64);
    }
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_new_from_sink_object(sink: f64, hwm: f64) -> f64 {
    ensure_gc_registered();
    let sink_type = stream_object_field(sink, b"type");
    if sink_type.to_bits() != TAG_UNDEFINED {
        throw_range_error_with_code(
            "The argument 'type' is invalid. Received a non-undefined value",
            "ERR_INVALID_ARG_VALUE",
        );
    }

    let (hwm, size_cb) = parse_strategy_value(hwm);
    let id = alloc_writable_with_strategy(
        stream_object_closure(sink, b"write"),
        stream_object_closure(sink, b"close"),
        stream_object_closure(sink, b"abort"),
        hwm,
        size_cb,
    );
    let start_cb = stream_object_closure(sink, b"start");
    if start_cb != 0 {
        js_closure_call1(start_cb as *const ClosureHeader, id as f64);
    }
    id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_throw_invalid_sink() -> f64 {
    throw_invalid_arg_type("The \"sink\" argument must be of type object")
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_get_writer(stream_handle: f64) -> f64 {
    ensure_gc_registered();
    let id = stream_handle as usize;
    let mut g = WRITABLE_STREAMS.lock().unwrap();
    let s = match g.get_mut(&id) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    if s.writer_handle.is_some() {
        drop(g);
        throw_type_error("WritableStream is locked");
    }
    let writer_id = next_stream_id();
    s.writer_handle = Some(writer_id);
    let closed_p = s.closed_promise;
    let ready_p = s.ready_promise;
    drop(g);
    WRITERS.lock().unwrap().insert(
        writer_id,
        WriterData {
            stream_handle: id,
            locked: true,
            closed_promise: closed_p,
            ready_promise: ready_p,
        },
    );
    writer_id as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_locked(stream_handle: f64) -> f64 {
    let id = stream_handle as usize;
    let g = WRITABLE_STREAMS.lock().unwrap();
    let locked = g
        .get(&id)
        .map(|s| s.writer_handle.is_some())
        .unwrap_or(false);
    f64::from_bits(if locked { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_close(stream_handle: f64) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let start_close = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => match s.state {
                WritableState::Writable => {
                    s.state = WritableState::Closing;
                    s.close_request_promise = promise;
                    !s.in_flight && s.write_queue.is_empty()
                }
                WritableState::Closing => {
                    if !s.close_request_promise.is_null() {
                        return s.close_request_promise;
                    }
                    s.close_request_promise = promise;
                    !s.in_flight && s.write_queue.is_empty()
                }
                WritableState::Closed => {
                    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
                    return promise;
                }
                WritableState::Errored => {
                    js_promise_reject(promise, f64::from_bits(s.error_value));
                    return promise;
                }
            },
            None => {
                js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
                return promise;
            }
        }
    };
    if start_close {
        finish_writable_close(id);
    }
    promise
}

#[no_mangle]
pub unsafe extern "C" fn js_writable_stream_abort(stream_handle: f64, reason: f64) -> *mut Promise {
    js_writable_stream_abort_inner(stream_handle, reason, false)
}

pub(super) unsafe fn js_writable_stream_abort_inner(
    stream_handle: f64,
    reason: f64,
    allow_locked: bool,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_handle as usize;
    let reason_bits = reason.to_bits();
    let mut locked_reject = false;
    let (cb, closed_p, close_request) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&id) {
            Some(s) => {
                if !allow_locked && s.writer_handle.is_some() {
                    locked_reject = true;
                    (0, std::ptr::null_mut(), std::ptr::null_mut())
                } else {
                    s.state = WritableState::Errored;
                    s.error_value = reason_bits;
                    let close_request = s.close_request_promise;
                    s.close_request_promise = std::ptr::null_mut();
                    s.close_started = false;
                    (s.abort_cb, s.closed_promise, close_request)
                }
            }
            None => (0, std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if locked_reject {
        reject_type_error(promise, "Invalid state: WritableStream is locked");
        return promise;
    }
    if cb != 0 {
        js_closure_call1(cb as *const ClosureHeader, reason);
    }
    if !closed_p.is_null() {
        js_promise_reject(closed_p, reason);
    }
    if !close_request.is_null() {
        js_promise_reject(close_request, reason);
    }
    // #5437: writable stream aborted (terminal Errored) — drop expandos.
    super::expando::stream_expando_clear(id);
    super::idalloc::retire_writable_terminal(id);
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    promise
}

// ─────────────────────────────────────────────────────────────────────
// WritableStreamDefaultWriter FFI
// ─────────────────────────────────────────────────────────────────────

fn writable_desired_size(s: &WritableStreamData) -> f64 {
    let queued: f64 = s.write_queue.iter().map(|(_, _, size)| size).sum();
    s.high_water_mark - s.in_flight_size - queued
}

fn sync_writer_ready_promise(stream_id: usize, writer_id: usize, ready: *mut Promise) {
    if let Some(w) = WRITERS.lock().unwrap().get_mut(&writer_id) {
        if w.stream_handle == stream_id {
            w.ready_promise = ready;
        }
    }
}

pub(super) unsafe fn install_writable_backpressure_ready(stream_id: usize, writer_id: usize) {
    let ready = internal_promise();
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.ready_promise = ready;
    }
    sync_writer_ready_promise(stream_id, writer_id, ready);
}

fn writable_capture_usize(closure: *const ClosureHeader, idx: u32) -> usize {
    let bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as u64;
    f64::from_bits(bits) as usize
}

fn writable_capture_promise(closure: *const ClosureHeader, idx: u32) -> *mut Promise {
    perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as *mut Promise
}

extern "C" fn writable_write_start_microtask(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let writer_id = writable_capture_usize(closure, 1);
        let cb = perry_runtime::closure::js_closure_get_capture_ptr(closure, 2);
        let chunk_bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, 3) as u64;
        let write_promise = writable_capture_promise(closure, 4);
        run_writable_write(
            stream_id,
            writer_id,
            cb,
            f64::from_bits(chunk_bits),
            write_promise,
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_write_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let writer_id = writable_capture_usize(closure, 1);
        let write_promise = writable_capture_promise(closure, 2);
        finish_writable_write_success(stream_id, writer_id, write_promise);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_write_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let write_promise = writable_capture_promise(closure, 2);
        finish_writable_write_error(stream_id, write_promise, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn attach_writable_write_handlers(
    stream_id: usize,
    writer_id: usize,
    write_promise: *mut Promise,
    sink_promise: *mut Promise,
) {
    let fulfilled_fn = writable_write_fulfilled as *const u8;
    let rejected_fn = writable_write_rejected as *const u8;
    perry_runtime::closure::js_register_closure_arity(fulfilled_fn, 1);
    perry_runtime::closure::js_register_closure_arity(rejected_fn, 1);

    let on_fulfilled = perry_runtime::closure::js_closure_alloc(fulfilled_fn, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        1,
        (writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_fulfilled, 2, write_promise as i64);

    let on_rejected = perry_runtime::closure::js_closure_alloc(rejected_fn, 3);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        1,
        (writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_rejected, 2, write_promise as i64);

    let _ = perry_runtime::promise::js_promise_then(sink_promise, on_fulfilled, on_rejected);
}

unsafe fn try_call_writable_write(cb: i64, chunk: f64) -> Result<f64, u64> {
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let result = js_closure_call1(cb as *const ClosureHeader, chunk);
        perry_runtime::exception::js_try_end();
        Ok(result)
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        Err(err.to_bits())
    }
}

unsafe fn try_call_writable_close(cb: i64) -> Result<f64, u64> {
    let trap_buf = perry_runtime::exception::js_try_push();
    let jumped = perry_runtime::ffi::setjmp::setjmp(trap_buf as *mut c_int);
    if jumped == 0 {
        let result = js_closure_call0(cb as *const ClosureHeader);
        perry_runtime::exception::js_try_end();
        Ok(result)
    } else {
        let err = perry_runtime::exception::js_get_exception();
        perry_runtime::exception::js_clear_exception();
        perry_runtime::exception::js_try_end();
        Err(err.to_bits())
    }
}

extern "C" fn writable_close_fulfilled(closure: *const ClosureHeader, _value: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let close_promise = writable_capture_promise(closure, 1);
        finish_writable_close_success(stream_id, close_promise);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn writable_close_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let stream_id = writable_capture_usize(closure, 0);
        let close_promise = writable_capture_promise(closure, 1);
        finish_writable_close_error(stream_id, close_promise, reason);
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn attach_writable_close_handlers(
    stream_id: usize,
    close_promise: *mut Promise,
    sink_promise: *mut Promise,
) {
    let fulfilled_fn = writable_close_fulfilled as *const u8;
    let rejected_fn = writable_close_rejected as *const u8;
    perry_runtime::closure::js_register_closure_arity(fulfilled_fn, 1);
    perry_runtime::closure::js_register_closure_arity(rejected_fn, 1);

    let on_fulfilled = perry_runtime::closure::js_closure_alloc(fulfilled_fn, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_fulfilled,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_fulfilled, 1, close_promise as i64);

    let on_rejected = perry_runtime::closure::js_closure_alloc(rejected_fn, 2);
    perry_runtime::closure::js_closure_set_capture_ptr(
        on_rejected,
        0,
        (stream_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(on_rejected, 1, close_promise as i64);

    let _ = perry_runtime::promise::js_promise_then(sink_promise, on_fulfilled, on_rejected);
}

unsafe fn finish_writable_close(stream_id: usize) {
    let (cb, close_promise) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) if s.state == WritableState::Closing => {
                if s.in_flight || !s.write_queue.is_empty() || s.close_started {
                    return;
                }
                s.close_started = true;
                (s.close_cb, s.close_request_promise)
            }
            _ => return,
        }
    };

    if cb == 0 {
        finish_writable_close_success(stream_id, close_promise);
        return;
    }
    let result = match try_call_writable_close(cb) {
        Ok(result) => result,
        Err(reason) => {
            finish_writable_close_error(stream_id, close_promise, f64::from_bits(reason));
            return;
        }
    };
    let sink_promise = perry_runtime::promise::js_promise_resolved(result);
    if !sink_promise.is_null() {
        attach_writable_close_handlers(stream_id, close_promise, sink_promise);
        return;
    }
    finish_writable_close_success(stream_id, close_promise);
}

unsafe fn finish_writable_close_success(stream_id: usize, promise: *mut Promise) {
    let (ready, closed) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.state = WritableState::Closed;
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                (s.ready_promise, s.closed_promise)
            }
            None => (std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if !promise.is_null() {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    }
    if !ready.is_null() {
        js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    }
    if !closed.is_null() {
        js_promise_resolve(closed, f64::from_bits(TAG_UNDEFINED));
    }
    // #5437: writable stream reached its terminal Closed state — drop expandos.
    super::expando::stream_expando_clear(stream_id);
    super::idalloc::retire_writable_terminal(stream_id);
}

unsafe fn finish_writable_close_error(stream_id: usize, promise: *mut Promise, reason: f64) {
    let (ready, closed) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.state = WritableState::Errored;
                s.error_value = reason.to_bits();
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                (s.ready_promise, s.closed_promise)
            }
            None => (std::ptr::null_mut(), std::ptr::null_mut()),
        }
    };
    if !promise.is_null() {
        js_promise_reject(promise, reason);
    }
    if !ready.is_null() {
        js_promise_reject(ready, reason);
    }
    if !closed.is_null() {
        js_promise_reject(closed, reason);
    }
    // #5437: writable stream reached its terminal Errored state — drop expandos.
    super::expando::stream_expando_clear(stream_id);
    super::idalloc::retire_writable_terminal(stream_id);
}

pub(super) unsafe fn run_writable_write(
    stream_id: usize,
    writer_id: usize,
    cb: i64,
    chunk: f64,
    promise: *mut Promise,
) {
    if cb == 0 {
        finish_writable_write_success(stream_id, writer_id, promise);
        return;
    }
    let result = match try_call_writable_write(cb, chunk) {
        Ok(result) => result,
        Err(reason) => {
            finish_writable_write_error(stream_id, promise, f64::from_bits(reason));
            return;
        }
    };
    let sink_promise = perry_runtime::promise::js_promise_resolved(result);
    if !sink_promise.is_null() {
        attach_writable_write_handlers(stream_id, writer_id, promise, sink_promise);
        return;
    }
    finish_writable_write_success(stream_id, writer_id, promise);
}

pub(super) unsafe fn writable_stream_write(
    stream_id: usize,
    writer_id: usize,
    chunk: f64,
) -> *mut Promise {
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&stream_id) {
        return transform_write(stream_id, chunk);
    }
    let promise = js_promise_new();
    // The strategy's size(chunk) is user JS — run it before taking the
    // registry lock (it may re-enter the streams FFI).
    let size_cb = WRITABLE_STREAMS
        .lock()
        .unwrap()
        .get(&stream_id)
        .map(|s| s.strategy_size_cb)
        .unwrap_or(0);
    let chunk_size = if size_cb != 0 {
        let size =
            JSValue::from_bits(js_closure_call1(size_cb as *const ClosureHeader, chunk).to_bits())
                .to_number();
        if size.is_nan() || size < 0.0 || size.is_infinite() {
            let message = invalid_size_message(size);
            throw_range_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
        }
        size
    } else {
        1.0
    };
    let mut start_write = None;
    let needs_pending_ready;
    {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        let s = match g.get_mut(&stream_id) {
            Some(s) if s.state == WritableState::Writable => s,
            Some(s) if s.state == WritableState::Errored => {
                let e = s.error_value;
                js_promise_reject(promise, f64::from_bits(e));
                return promise;
            }
            _ => {
                reject_type_error(promise, "Stream is closed or closing");
                return promise;
            }
        };
        let before = writable_desired_size(s);
        if s.in_flight {
            s.write_queue
                .push_back((chunk.to_bits(), promise, chunk_size));
        } else {
            s.in_flight = true;
            s.in_flight_size = chunk_size;
            start_write = Some((s.write_cb, chunk, promise));
        }
        let after = writable_desired_size(s);
        needs_pending_ready = before > 0.0 && after <= 0.0;
    }
    if needs_pending_ready {
        install_writable_backpressure_ready(stream_id, writer_id);
    }
    if let Some((cb, chunk, write_promise)) = start_write {
        // Spec/Node tick parity: with no write in flight, `writer.write(chunk)`
        // invokes the sink's `write()` SYNCHRONOUSLY in the same job
        // (WritableStreamDefaultControllerProcessWrite) — deferring it through
        // a microtask cost one tick per write, which let a tee sibling's
        // reader outrun a pipeTo pump (teepipe.js; Next.js cold-start head
        // reorder). All registry locks are released above; `run_writable_write`
        // re-enters the FFI safely (it is the same body the queued microtask
        // ran).
        run_writable_write(stream_id, writer_id, cb, chunk, write_promise);
    }
    promise
}

unsafe fn finish_writable_write_success(stream_id: usize, writer_id: usize, promise: *mut Promise) {
    if !promise.is_null() {
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    }

    let (next, ready, close_now) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.in_flight = false;
                s.in_flight_size = 0.0;
                let next =
                    if s.state == WritableState::Writable || s.state == WritableState::Closing {
                        s.write_queue.pop_front().map(|(chunk, p, size)| {
                            s.in_flight = true;
                            s.in_flight_size = size;
                            (s.write_cb, f64::from_bits(chunk), p)
                        })
                    } else {
                        None
                    };
                let ready = if s.state == WritableState::Writable && writable_desired_size(s) > 0.0
                {
                    s.ready_promise
                } else {
                    std::ptr::null_mut()
                };
                let close_now = s.state == WritableState::Closing
                    && next.is_none()
                    && !s.in_flight
                    && s.write_queue.is_empty();
                (next, ready, close_now)
            }
            None => (None, std::ptr::null_mut(), false),
        }
    };

    if !ready.is_null() {
        js_promise_resolve(ready, f64::from_bits(TAG_UNDEFINED));
    }
    if let Some((cb, chunk, queued_promise)) = next {
        run_writable_write(stream_id, writer_id, cb, chunk, queued_promise);
    } else if close_now {
        finish_writable_close(stream_id);
    }
}

unsafe fn finish_writable_write_error(stream_id: usize, promise: *mut Promise, reason: f64) {
    let (ready, closed, close_request, queued) = {
        let mut g = WRITABLE_STREAMS.lock().unwrap();
        match g.get_mut(&stream_id) {
            Some(s) => {
                s.in_flight = false;
                s.in_flight_size = 0.0;
                s.state = WritableState::Errored;
                s.error_value = reason.to_bits();
                let close_request = s.close_request_promise;
                s.close_request_promise = std::ptr::null_mut();
                s.close_started = false;
                let queued: Vec<*mut Promise> =
                    s.write_queue.drain(..).map(|(_, p, _)| p).collect();
                (s.ready_promise, s.closed_promise, close_request, queued)
            }
            None => (
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                Vec::new(),
            ),
        }
    };

    if !promise.is_null() {
        js_promise_reject(promise, reason);
    }
    for queued_promise in queued {
        if !queued_promise.is_null() {
            js_promise_reject(queued_promise, reason);
        }
    }
    if !ready.is_null() {
        js_promise_reject(ready, reason);
    }
    if !close_request.is_null() {
        js_promise_reject(close_request, reason);
    }
    if !closed.is_null() {
        js_promise_reject(closed, reason);
    }
    super::idalloc::retire_writable_terminal(stream_id);
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_write(writer_handle: f64, chunk: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) if w.locked => w.stream_handle,
        _ => {
            let promise = js_promise_new();
            reject_type_error(promise, "Writer is no longer locked to a stream");
            return promise;
        }
    };
    writable_stream_write(stream_id, writer_id, chunk)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_desired_size(writer_handle: f64) -> f64 {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => return 0.0,
    };
    let g = WRITABLE_STREAMS.lock().unwrap();
    match g.get(&stream_id) {
        Some(s) if s.state == WritableState::Writable => writable_desired_size(s),
        Some(s) if s.state == WritableState::Errored => f64::NAN,
        _ => 0.0,
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_close(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&stream_id) {
        return transform_close(stream_id);
    }
    js_writable_stream_close(stream_id as f64)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_abort(writer_handle: f64, reason: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    let stream_id = match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    js_writable_stream_abort_inner(stream_id as f64, reason, true)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_release_lock(writer_handle: f64) -> f64 {
    let writer_id = writer_handle as usize;
    let stream_id = {
        let mut g = WRITERS.lock().unwrap();
        match g.get_mut(&writer_id) {
            Some(w) => {
                w.locked = false;
                w.stream_handle
            }
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&stream_id) {
        s.writer_handle = None;
    }
    // #5437: writer instance done — drop its expando entries (see the reader
    // release path in streams.rs).
    super::expando::stream_expando_clear(writer_id);
    super::idalloc::retire_writer_if_released(writer_id);
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_closed(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.closed_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_writer_ready(writer_handle: f64) -> *mut Promise {
    let writer_id = writer_handle as usize;
    match WRITERS.lock().unwrap().get(&writer_id) {
        Some(w) => w.ready_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}
