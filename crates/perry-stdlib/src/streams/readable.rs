//! `ReadableStream` construction, `getReader()`, reader acquisition, `locked` and `cancel()`.

use super::{
    alloc_readable, alloc_readable_with_strategy, alloc_readable_with_type,
    alloc_uint8array_from_bytes, close_pending, closure_from_bits, ensure_gc_registered,
    insert_reader, internal_promise, invoke_start, js_promise_new, js_promise_reject,
    js_promise_resolve, js_stream_unwrap_handle, js_writable_stream_abort_inner, maybe_pull,
    native, next_stream_id, read_high_water_mark, read_queuing_strategy_size, reject_type_error,
    rs_rec, settle_stream_action_promise, stream_action_promise, stream_id, stream_object_closure,
    stream_object_field, stream_value, tee, throw_type_error, transform_writable_for_readable,
    try_call_stream_action, with_rs, JSValue, Promise, ReadableState, ReaderData, ReaderRef, RsRef,
    TAG_FALSE, TAG_TRUE, TAG_UNDEFINED,
};

/// `new ReadableStream({ start, pull, cancel })` — `start_cb` / `pull_cb`
/// / `cancel_cb` are NaN-boxed `*ClosureHeader` bits (or undefined). The
/// new stream's controller is the stream handle itself; user code calls
/// `controller.enqueue(c)` etc. to drive it.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new(
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    hwm: f64,
) -> f64 {
    js_readable_stream_new_with_source_type(
        start_bits,
        pull_bits,
        cancel_bits,
        hwm,
        f64::from_bits(TAG_UNDEFINED),
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_with_source_type(
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    hwm: f64,
    source_type: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_readable_with_type(
        closure_from_bits(start_bits.to_bits()),
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        hwm,
        value_string_equals(source_type, b"bytes"),
    );
    invoke_start(id);
    maybe_pull(id);
    stream_value(id)
}

/// Runtime registration target for APIs like
/// `fs.promises.FileHandle.readableWebStream()`: create a byte-oriented
/// ReadableStream whose pull callback returns one Uint8Array chunk per call
/// and `undefined` at EOF. Unlike the public constructor path, this does not
/// run an eager initial pull, so file positions are not advanced at stream
/// creation time.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_deferred_byte_source(
    pull_bits: f64,
    cancel_bits: f64,
) -> f64 {
    ensure_gc_registered();
    let id = alloc_readable_with_type(
        0,
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        1.0,
        true,
    );
    if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
        s.started = true;
        s.pull_returns_byte_chunk = true;
    }
    stream_value(id)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_with_strategy_and_source_type(
    start_bits: f64,
    pull_bits: f64,
    cancel_bits: f64,
    strategy: f64,
    source_type: f64,
) -> f64 {
    ensure_gc_registered();
    let is_byte_stream = value_string_equals(source_type, b"bytes");
    let size_cb = if is_byte_stream {
        0
    } else {
        read_queuing_strategy_size(strategy)
    };
    let id = alloc_readable_with_strategy(
        closure_from_bits(start_bits.to_bits()),
        closure_from_bits(pull_bits.to_bits()),
        closure_from_bits(cancel_bits.to_bits()),
        f64::from_bits(read_high_water_mark(strategy)),
        is_byte_stream,
        size_cb,
    );
    invoke_start(id);
    maybe_pull(id);
    stream_value(id)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_new_from_source_object(
    source: f64,
    strategy: f64,
) -> f64 {
    ensure_gc_registered();
    let source_type = stream_object_field(source, b"type");
    let is_byte_stream = value_string_equals(source_type, b"bytes");
    let size_cb = if is_byte_stream {
        0
    } else {
        read_queuing_strategy_size(strategy)
    };
    let id = alloc_readable_with_strategy(
        stream_object_closure(source, b"start"),
        stream_object_closure(source, b"pull"),
        stream_object_closure(source, b"cancel"),
        f64::from_bits(read_high_water_mark(strategy)),
        is_byte_stream,
        size_cb,
    );
    invoke_start(id);
    maybe_pull(id);
    stream_value(id)
}

/// Internal helper: build a single-chunk readable stream from an owned
/// byte buffer. Used by `blob.stream()` and `response.body`.
pub(crate) fn alloc_readable_from_bytes(bytes: Vec<u8>) -> RsRef {
    ensure_gc_registered();
    let id = alloc_readable(0, 0, 0, 1.0);
    unsafe {
        let chunk_bits = alloc_uint8array_from_bytes(&bytes);
        if let Some(s) = rs_rec(RsRef(id)).as_deref_mut() {
            s.started = true;
            s.is_byte_stream = true;
            if !bytes.is_empty() {
                s.push_chunk(chunk_bits, bytes.len() as f64);
            }
            s.state = ReadableState::Closed;
        }
    }
    RsRef(id)
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_get_reader(stream_handle: f64) -> f64 {
    js_readable_stream_get_reader_with_options(stream_handle, f64::from_bits(TAG_UNDEFINED))
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_get_reader_with_options(
    stream_handle: f64,
    options: f64,
) -> f64 {
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] get_reader stream={:#x}", stream_handle.to_bits());
        }
    }
    let stream_handle = js_stream_unwrap_handle(stream_handle);
    let byob_requested = option_string_equals(options, b"mode", b"byob");
    let id = stream_id(stream_handle);
    if byob_requested {
        let is_non_byte_stream = {
            rs_rec(RsRef(id))
                .as_deref()
                .map(|s| !s.is_byte_stream)
                .unwrap_or(false)
        };
        if is_non_byte_stream {
            throw_type_error("ReadableStream BYOB reader requires a byte stream");
        }
    }

    ensure_gc_registered();
    let id = stream_id(stream_handle);
    match acquire_reader(RsRef(id), byob_requested) {
        Ok(reader) => reader.value(),
        Err(AcquireError::Locked) => throw_type_error("ReadableStream is locked"),
        Err(AcquireError::Missing) => f64::from_bits(TAG_UNDEFINED),
    }
}

/// Why AcquireReadableStreamReader / AcquireWritableStreamDefaultWriter
/// failed: the stream already has a reader/writer, or the id names no stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcquireError {
    Locked,
    Missing,
}

/// AcquireReadableStreamDefaultReader / AcquireReadableStreamBYOBReader:
/// mint a reader record locked to `r` (getReader() and pipeTo share it).
pub(crate) unsafe fn acquire_reader(r: RsRef, byob: bool) -> Result<ReaderRef, AcquireError> {
    let Some(mut rec) = rs_rec(r) else {
        return Err(AcquireError::Missing);
    };
    let s = &mut *rec;
    if s.reader_handle.is_some() {
        return Err(AcquireError::Locked);
    }
    let reader = ReaderRef(next_stream_id());
    let closed_p = internal_promise();
    if s.state == ReadableState::Closed {
        js_promise_resolve(closed_p, f64::from_bits(TAG_UNDEFINED));
    } else if s.state == ReadableState::Errored {
        js_promise_reject(closed_p, f64::from_bits(s.error_value));
    }
    s.reader_handle = Some(reader.0);
    insert_reader(reader, ReaderData::new(r.0, closed_p, byob));
    Ok(reader)
}

pub(super) unsafe fn option_string_equals(options: f64, name: &[u8], expected: &[u8]) -> bool {
    let value =
        perry_runtime::value::js_get_property(options, name.as_ptr() as i64, name.len() as i64);
    value_string_equals(value, expected)
}

pub(super) unsafe fn value_string_equals(value: f64, expected: &[u8]) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return false;
    }

    let ptr = perry_runtime::value::js_get_string_pointer_unified(value)
        as *const perry_runtime::StringHeader;
    if !perry_runtime::value::addr_class::is_above_handle_band(ptr as usize) {
        return false;
    }

    let len = (*ptr).byte_len as usize;
    if len != expected.len() {
        return false;
    }

    let data = (ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
    std::slice::from_raw_parts(data, len) == expected
}

pub(super) unsafe fn js_string_value_to_string(value: f64, coerce: bool) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !coerce && !jsval.is_any_string() {
        return None;
    }
    let ptr = if coerce {
        perry_runtime::value::js_jsvalue_to_string(value) as *const perry_runtime::StringHeader
    } else {
        perry_runtime::value::js_get_string_pointer_unified(value)
            as *const perry_runtime::StringHeader
    };
    if !perry_runtime::value::addr_class::is_above_handle_band(ptr as usize) {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<perry_runtime::StringHeader>());
    Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_locked(stream_handle: f64) -> f64 {
    let id = stream_id(stream_handle);
    let locked = with_rs(RsRef(id), |s| s.reader_handle.is_some()).unwrap_or(false);
    f64::from_bits(if locked { TAG_TRUE } else { TAG_FALSE })
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_cancel(
    stream_handle: f64,
    reason: f64,
) -> *mut Promise {
    js_readable_stream_cancel_inner(stream_handle, reason, false)
}

pub(super) unsafe fn js_readable_stream_cancel_inner(
    stream_handle: f64,
    reason: f64,
    allow_locked: bool,
) -> *mut Promise {
    let promise = js_promise_new();
    let id = stream_id(stream_handle);
    let mut locked_reject = false;
    let mut stored_error = None;
    let cb = {
        match rs_rec(RsRef(id)).as_deref_mut() {
            Some(s) => {
                if !allow_locked && s.reader_handle.is_some() {
                    locked_reject = true;
                    0
                } else if s.state == ReadableState::Errored {
                    // ReadableStreamCancel step 3: an errored stream answers
                    // with its stored error; the source (whose algorithms the
                    // error cleared) is not consulted.
                    s.disturbed = true;
                    stored_error = Some(s.error_value);
                    0
                } else {
                    s.canceled = true;
                    s.disturbed = true;
                    s.state = ReadableState::Closed;
                    s.clear_chunks();
                    s.cancel_cb
                }
            }
            None => 0,
        }
    };
    if locked_reject {
        reject_type_error(promise, "ReadableStream is locked");
        return promise;
    }
    if let Some(error) = stored_error {
        js_promise_reject(promise, f64::from_bits(error));
        return promise;
    }
    native::cancel_native(id);
    let tee_cancellation = tee::tee_cancel_branch(id, promise);
    let mut actions = [std::ptr::null_mut(); 2];
    let mut action_count = 0;
    if let Some(writable_id) = transform_writable_for_readable(id) {
        actions[action_count] =
            js_writable_stream_abort_inner(stream_value(writable_id), reason, true);
        action_count += 1;
    }
    if cb != 0 {
        match try_call_stream_action(cb, reason) {
            Ok(result) => {
                if let Some(action) = stream_action_promise(result) {
                    actions[action_count] = action;
                    action_count += 1;
                }
            }
            Err(error) => {
                close_pending(id);
                js_promise_reject(promise, f64::from_bits(error));
                return promise;
            }
        }
    }
    close_pending(id);
    if !tee_cancellation {
        settle_stream_action_promise(promise, &actions[..action_count]);
    }
    promise
}
