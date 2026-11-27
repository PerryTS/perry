//! `ReadableStreamDefaultController`: `enqueue()`, `close()`, `error()`, `desiredSize`, and queuing-strategy size validation.

use super::{
    build_iter_result, byob, close_pending, controller_stream_id, error_pending,
    is_byte_stream_enqueue_chunk, js_closure_call1, js_promise_reject, js_promise_resolve, native,
    reader_rec, rs_rec, stream_id, stream_range_error_with_code, tee, tee_branches_of,
    tee_error_branches, throw_invalid_arg_type, throw_type_error_with_code, ClosureHeader, JSValue,
    ReadableState, ReaderRef, RsRef, TAG_UNDEFINED,
};

pub(super) fn invalid_size_message(size: f64) -> String {
    let received = if size.is_nan() {
        "NaN".to_string()
    } else if size == f64::INFINITY {
        "Infinity".to_string()
    } else if size == f64::NEG_INFINITY {
        "-Infinity".to_string()
    } else {
        format!("{}", size)
    };
    format!("The argument 'size' is invalid. Received {}", received)
}

pub(super) unsafe fn readable_strategy_size_to_number(value: f64) -> f64 {
    JSValue::from_bits(value.to_bits()).to_number()
}

pub(super) unsafe fn error_readable_stream(stream_id: usize, reason_bits: u64) {
    let reader_id = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) => {
                s.state = ReadableState::Errored;
                s.error_value = reason_bits;
                s.pending_error_after_chunks = None;
                s.clear_chunks();
                s.reader_handle
            }
            None => return,
        }
    };
    error_pending(stream_id, reason_bits);
    if let Some(rid) = reader_id {
        let p = reader_rec(ReaderRef(rid))
            .as_deref()
            .map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_reject(p, f64::from_bits(reason_bits));
        }
    }
}

pub(super) unsafe fn throw_invalid_readable_strategy_size(stream_id: usize, size: f64) -> ! {
    let message = invalid_size_message(size);
    let err = stream_range_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
    error_readable_stream(stream_id, err.value().to_bits());
    perry_runtime::exception::throw_core(err)
}

pub(super) fn readable_controller_enqueue_error(stream_id: usize) -> Option<&'static str> {
    match rs_rec(RsRef(stream_id)).as_deref() {
        Some(stream) if stream.state == ReadableState::Readable => None,
        _ => Some("Invalid state: Controller is already closed"),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_enqueue(
    stream_handle: f64,
    chunk: f64,
) -> f64 {
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!(
                "[STREAM] enqueue stream={:#x} chunk_bits={:#018x}",
                stream_id(stream_handle),
                chunk.to_bits()
            );
        }
    }
    let id = controller_stream_id(stream_handle);
    if let Some(message) = readable_controller_enqueue_error(id) {
        throw_type_error_with_code(message, "ERR_INVALID_STATE");
    }
    let chunk_bits = chunk.to_bits();
    let is_byte_stream = {
        rs_rec(RsRef(id))
            .as_deref()
            .map(|s| s.is_byte_stream)
            .unwrap_or(false)
    };
    if is_byte_stream && !is_byte_stream_enqueue_chunk(chunk_bits) {
        throw_invalid_arg_type(
            "The \"buffer\" argument must be an instance of Buffer, TypedArray, or DataView",
        );
    }
    // #5989 + Node tick parity: a tee'd source's enqueue stays in the SOURCE
    // queue; branches receive chunks only through the demand-driven pull
    // cycle (see `tee::tee_source_enqueue`).
    if tee::tee_source_enqueue(id, chunk, chunk_bits, is_byte_stream) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if native::collect_chunk(id, chunk_bits) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // A pending BYOB read (byte streams only) takes the chunk before the
    // default-read queue: the bytes land directly in the caller's view.
    if is_byte_stream && byob::service_pending_with_chunk(id, chunk_bits) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let (popped, strategy_size_cb) = {
        match rs_rec(RsRef(id)).as_deref_mut() {
            Some(s) if s.state == ReadableState::Readable => {
                if let Some(p) = s.pending_reads.pop_front() {
                    (Some(p), 0)
                } else {
                    (None, s.strategy_size_cb)
                }
            }
            _ => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(p) = popped {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(p);
        let result = build_iter_result(chunk_bits, false);
        js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
    } else {
        // Per spec the strategy's size(chunk) runs once at enqueue time; the
        // result is the chunk's contribution to desiredSize accounting.
        let size = if strategy_size_cb != 0 {
            let size = readable_strategy_size_to_number(js_closure_call1(
                strategy_size_cb as *const ClosureHeader,
                perry_runtime::closure::plain_call_receiver(),
                chunk,
            ));
            if size.is_nan() || size < 0.0 || size.is_infinite() {
                throw_invalid_readable_strategy_size(id, size);
            }
            size
        } else if is_byte_stream {
            byob::chunk_byte_length(chunk_bits)
        } else {
            1.0
        };
        if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
            if s.state == ReadableState::Readable {
                s.push_chunk(chunk_bits, size);
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_close(stream_handle: f64) -> f64 {
    let id = controller_stream_id(stream_handle);
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] controller_close stream={id:#x}");
        }
    }
    // #5989 + Node tick parity: closing a tee'd source marks it Closed; the
    // branch close is DISCOVERED by the pull cycle draining the source queue
    // empty (byte tee: prompt; default tee: nextTick-deferred) — never
    // delivered eagerly ahead of undrained chunks.
    if tee_branches_of(id).is_some() {
        {
            if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
                if s.state == ReadableState::Readable {
                    s.state = ReadableState::Closed;
                }
            }
        }
        tee::tee_schedule_pull(id);
        return f64::from_bits(TAG_UNDEFINED);
    }
    {
        if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
            if s.state == ReadableState::Readable {
                s.state = ReadableState::Closed;
                s.pending_error_after_chunks = None;
            }
        }
    }
    // Reader.closed promise resolves when stream closes and queue empties.
    let (queue_empty, reader_id) = {
        match rs_rec(RsRef(id)).as_deref() {
            Some(s) => (s.chunks.is_empty(), s.reader_handle),
            None => (true, None),
        }
    };
    if queue_empty {
        if let Some(rid) = reader_id {
            let p = reader_rec(ReaderRef(rid))
                .as_deref()
                .map(|r| r.closed_promise);
            if let Some(p) = p {
                js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            }
        }
        close_pending(id);
    }
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_error(
    stream_handle: f64,
    reason: f64,
) -> f64 {
    let id = controller_stream_id(stream_handle);
    // #5989: erroring a tee'd source errors both branches.
    if tee_branches_of(id).is_some() {
        tee_error_branches(id, reason.to_bits());
        return f64::from_bits(TAG_UNDEFINED);
    }
    error_readable_stream(id, reason.to_bits());
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_controller_desired_size(stream_handle: f64) -> f64 {
    let id = controller_stream_id(stream_handle);
    match rs_rec(RsRef(id)).as_deref() {
        Some(s) if s.state == ReadableState::Readable => s.high_water_mark - s.queue_total_size,
        Some(s) if s.state == ReadableState::Errored => f64::NAN,
        _ => 0.0,
    }
}
