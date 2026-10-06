//! G2 (STREAM-PAYLOAD-DESIGN): node's writable machinery for every classic
//! stream — `writeOrBuffer`, `doWrite`, `onwrite`, `afterWrite`,
//! `clearBuffer` (`lib/internal/streams/writable.js`) and Transform's
//! `_write`/`_read` (`kCallback`, `lib/internal/streams/transform.js`).
//!
//! * Writes are serialized: while a write is in flight (`writing`), or the
//!   stream is corked, a write joins the buffered-write array; `onwrite`
//!   starts the next one (`clearBuffer`).
//! * A write that completes synchronously defers its callback (and `drain`)
//!   to a tick, as node does; one that completes later runs them at once.
//! * A Transform holds its write callback while the readable side is at or
//!   above its highWaterMark (`kCallback`); the readable side draining
//!   (`readable_maybe_read_more`, node's `_read`) releases it.
//!
//! Every way of writing reaches this one path: `write()`, `end(chunk)`,
//! `Readable.pipe` (`write_chunk_to_pipe_destinations`), `pipeline`, the web
//! adapters, and a native-payload stream's codec (`native_hooks`).

use super::*;

pub(super) const WRITABLE_WRITING_KEY: &[u8] = b"__perryWritableWriting";
pub(super) const WRITABLE_SYNC_KEY: &[u8] = b"__perryWritableSync";
pub(super) const WRITABLE_BUFFER_PROCESSING_KEY: &[u8] = b"__perryWritableBufferProcessing";
/// A Transform's held write completion (node's `kCallback`): a closure that
/// finishes the write when the readable side has room again.
pub(super) const TRANSFORM_HELD_CALLBACK_KEY: &[u8] = b"__perryTransformHeldCallback";

#[inline]
fn flag(stream: f64, key: &'static [u8]) -> bool {
    has_truthy_hidden(stream, hidden_key(key))
}

#[inline]
fn set_flag(stream: f64, key: &'static [u8], on: bool) {
    set_internal_value(
        stream,
        key,
        f64::from_bits(if on { TAG_TRUE } else { TAG_FALSE }),
    );
}

/// node's `state.writing`: a write is in flight.
pub(super) fn writable_writing(stream: f64) -> bool {
    flag(stream, WRITABLE_WRITING_KEY)
}

/// node's `state.sync`: the current `_write` has not returned yet.
pub(super) fn writable_sync(stream: f64) -> bool {
    flag(stream, WRITABLE_SYNC_KEY)
}

/// `writable.write(chunk, encoding, cb)`.
pub(super) fn write_writable_chunk(stream: f64, chunk: f64, enc: f64, cb: f64) -> f64 {
    if stream_hidden_ended(stream)
        || has_truthy_hidden(stream, hidden_transform_end_pending_key())
        || has_truthy_hidden(stream, hidden_transform_finishing_key())
    {
        let err = writable_write_after_end_error();
        let _ = emit_stream_event(stream, literal_string_value(b"error"), &[err]);
        return f64::from_bits(TAG_FALSE);
    }
    if JSValue::from_bits(chunk.to_bits()).is_null() {
        throw_writable_null_chunk();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let (chunk, enc, callback) = normalize_write_args(stream, chunk, enc, cb);
    let chunk = scope.root_nanbox_f64(chunk);
    let enc = scope.root_nanbox_f64(enc);
    let callback = scope.root_nanbox_f64(callback);
    let len = writable_chunk_len(s.get_nanbox_f64(), chunk.get_nanbox_f64());
    add_writable_length(s.get_nanbox_f64(), len);
    write_or_buffer(
        s.get_nanbox_f64(),
        chunk.get_nanbox_f64(),
        enc.get_nanbox_f64(),
        len,
        callback.get_nanbox_f64(),
    );
    // node computes the return after `_write`: a write that completed
    // synchronously has already given its length back.
    writable_backpressure_return(s.get_nanbox_f64())
}

/// A runtime-built write record (a native stream's `.flush(kind)`): length
/// accounting plus `writeOrBuffer`, without the argument normalization.
pub(super) fn write_record(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    add_writable_length(stream, len);
    write_or_buffer(stream, chunk, enc, len, callback);
}

/// node's `writeOrBuffer` after the length accounting: buffer while a write is
/// in flight or the stream is corked, otherwise write now.
fn write_or_buffer(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    #[cfg(test)]
    if super::native_hooks::stream_sabotage("pipe_bypass_writing") {
        if let Some(hooks) = super::native_hooks::native_write_target(stream) {
            super::native_hooks::begin_write(stream, hooks, chunk, enc, len, callback);
            return;
        }
    }
    if writable_writing(stream) || writable_corked_count(stream) > 0.0 {
        buffer_writable_write(stream, chunk, enc, len, callback);
        return;
    }
    do_write(stream, chunk, enc, len, callback);
}

/// node's `doWrite`: one write in flight, its completion is [`complete_writable_write`].
fn do_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let chunk = scope.root_nanbox_f64(chunk);
    set_flag(s.get_nanbox_f64(), WRITABLE_WRITING_KEY, true);
    set_flag(s.get_nanbox_f64(), WRITABLE_SYNC_KEY, true);
    if let Some(hooks) = super::native_hooks::native_write_target(s.get_nanbox_f64()) {
        super::native_hooks::begin_write(
            s.get_nanbox_f64(),
            hooks,
            chunk.get_nanbox_f64(),
            enc,
            len,
            callback,
        );
    } else if is_transform_stream(s.get_nanbox_f64()) {
        invoke_transform_write(
            s.get_nanbox_f64(),
            chunk.get_nanbox_f64(),
            enc,
            len,
            callback,
        );
    } else {
        invoke_writable_write(
            s.get_nanbox_f64(),
            chunk.get_nanbox_f64(),
            enc,
            len,
            callback,
        );
        emit_writable_chunk(s.get_nanbox_f64(), chunk.get_nanbox_f64());
    }
    set_flag(s.get_nanbox_f64(), WRITABLE_SYNC_KEY, false);
}

/// node's `onwrite`: the write in flight completed (`err` undefined / null on
/// success).
pub(super) fn complete_writable_write(stream: f64, len: f64, callback: f64, err: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    let err = scope.root_nanbox_f64(err);
    let sync = writable_sync(s.get_nanbox_f64());
    set_flag(s.get_nanbox_f64(), WRITABLE_WRITING_KEY, false);
    subtract_writable_length(s.get_nanbox_f64(), len);
    let e = err.get_nanbox_f64();
    if e.to_bits() != TAG_UNDEFINED && e.to_bits() != TAG_NULL {
        if is_callable_value(callback.get_nanbox_f64()) {
            call_write_callback(callback.get_nanbox_f64(), e);
        }
        destroy_stream(s.get_nanbox_f64(), e);
        return;
    }
    if buffered_record_count(s.get_nanbox_f64()) > 0 {
        clear_buffer(s.get_nanbox_f64());
    }
    if sync {
        let closure = js_closure_alloc(crate::fn_info!(ns_after_write_tick, 0), 2);
        js_closure_set_capture_f64(closure, 0, s.get_nanbox_f64());
        js_closure_set_capture_f64(closure, 1, callback.get_nanbox_f64());
        crate::builtins::js_queue_microtask(closure as i64);
    } else {
        after_write(s.get_nanbox_f64(), callback.get_nanbox_f64());
    }
}

extern "C" fn ns_after_write_tick(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let callback = js_closure_get_capture_f64(closure, 1);
    after_write(stream, callback);
    f64::from_bits(TAG_UNDEFINED)
}

fn call_write_callback(callback: f64, arg: f64) {
    let args = [arg];
    unsafe {
        let _ = crate::closure::js_native_call_value(
            callback,
            crate::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len(),
        );
    }
}

/// node's `afterWrite`: `drain` (when the buffer emptied below the mark),
/// then the write's callback, then `finishMaybe`.
fn after_write(stream: f64, callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    if writable_length(s.get_nanbox_f64()) == 0.0 {
        let should_emit_drain = writable_need_drain_raw(s.get_nanbox_f64())
            && !stream_hidden_ended(s.get_nanbox_f64())
            && !has_truthy_hidden(s.get_nanbox_f64(), hidden_key(b"destroyed"));
        set_writable_need_drain(s.get_nanbox_f64(), false);
        if should_emit_drain {
            let _ = emit_stream_event(s.get_nanbox_f64(), literal_string_value(b"drain"), &[]);
        }
    }
    if is_callable_value(callback.get_nanbox_f64()) {
        call_write_callback(callback.get_nanbox_f64(), f64::from_bits(TAG_NULL));
    }
    if writable_length(s.get_nanbox_f64()) == 0.0 && !writable_writing(s.get_nanbox_f64()) {
        finish_pending_pipe_destination_if_ready(s.get_nanbox_f64());
        schedule_pending_writable_finish_if_ready(s.get_nanbox_f64());
    }
}

/// The number of buffered write records (4 slots each).
fn buffered_record_count(stream: f64) -> u32 {
    let Some(buffered) = buffered_writable_writes(stream) else {
        return 0;
    };
    let raw = raw_ptr_from_value(buffered);
    if raw < 0x10000 {
        return 0;
    }
    crate::array::js_array_length(raw as *const crate::array::ArrayHeader) / 4
}

/// node's `clearBuffer`: start the buffered writes, one at a time while each
/// completes synchronously; a write left in flight leaves the rest buffered.
/// Several records and a `_writev` go out as one `writev`.
pub(super) fn clear_buffer(stream: f64) {
    if writable_corked_count(stream) > 0.0
        || stream_destroyed(stream)
        || flag(stream, WRITABLE_BUFFER_PROCESSING_KEY)
        || writable_writing(stream)
    {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let st = || s.get_nanbox_f64();
    set_flag(st(), WRITABLE_BUFFER_PROCESSING_KEY, true);
    loop {
        let Some(buffered) = buffered_writable_writes(st()) else {
            break;
        };
        let records = scope.root_nanbox_f64(buffered);
        let arr =
            || raw_ptr_from_value(records.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        if raw_ptr_from_value(records.get_nanbox_f64()) < 0x10000 {
            break;
        }
        let len = crate::array::js_array_length(arr());
        if len == 0 {
            break;
        }
        // The records are ours from here; writes made meanwhile buffer anew.
        set_hidden_value(
            st(),
            hidden_writable_buffered_key(),
            box_pointer(crate::array::js_array_alloc(0) as *const u8),
        );
        if len > 4 && writable_hidden_writev(st()).is_some() {
            set_flag(st(), WRITABLE_WRITING_KEY, true);
            let chunks = build_writev_chunks(arr(), len);
            invoke_writable_writev(st(), chunks);
            set_flag(st(), WRITABLE_WRITING_KEY, false);
            let mut i = 0;
            while i < len {
                let chunk = crate::array::js_array_get_f64(arr(), i);
                let write_len = crate::array::js_array_get_f64(arr(), i + 2);
                let callback = crate::array::js_array_get_f64(arr(), i + 3);
                emit_writable_chunk(st(), chunk);
                complete_writable_write(st(), write_len, callback, f64::from_bits(TAG_UNDEFINED));
                i += 4;
            }
            continue;
        }
        let mut i = 0;
        while i < len {
            if writable_writing(st()) || stream_destroyed(st()) {
                break;
            }
            let chunk = crate::array::js_array_get_f64(arr(), i);
            let enc = crate::array::js_array_get_f64(arr(), i + 1);
            let write_len = crate::array::js_array_get_f64(arr(), i + 2);
            let callback = crate::array::js_array_get_f64(arr(), i + 3);
            do_write(st(), chunk, enc, write_len, callback);
            i += 4;
        }
        if i < len {
            // A write is in flight: the unstarted records go back in front
            // of anything buffered meanwhile.
            requeue_front(st(), arr(), i, len);
            break;
        }
        if writable_writing(st()) {
            break;
        }
    }
    set_flag(st(), WRITABLE_BUFFER_PROCESSING_KEY, false);
}

/// Put `records[from..len]` back in front of the stream's buffered array.
fn requeue_front(stream: f64, records: *const crate::array::ArrayHeader, from: u32, len: u32) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let records = scope.root_raw_mut_ptr(records as *mut crate::array::ArrayHeader);
    let later = scope.root_nanbox_f64(
        buffered_writable_writes(s.get_nanbox_f64())
            .unwrap_or_else(|| box_pointer(crate::array::js_array_alloc(0) as *const u8)),
    );
    let mut merged = scope.root_raw_mut_ptr(crate::array::js_array_alloc(len - from));
    for i in from..len {
        let value = crate::array::js_array_get_f64(
            records.get_raw_const_ptr::<crate::array::ArrayHeader>(),
            i,
        );
        let grown = crate::array::js_array_push_f64(
            merged.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            value,
        );
        merged = scope.root_raw_mut_ptr(grown);
    }
    let later_raw = raw_ptr_from_value(later.get_nanbox_f64());
    if later_raw >= 0x10000 {
        let n = crate::array::js_array_length(later_raw as *const crate::array::ArrayHeader);
        for i in 0..n {
            let value = crate::array::js_array_get_f64(
                raw_ptr_from_value(later.get_nanbox_f64()) as *const crate::array::ArrayHeader,
                i,
            );
            let grown = crate::array::js_array_push_f64(
                merged.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
                value,
            );
            merged = scope.root_raw_mut_ptr(grown);
        }
    }
    set_hidden_value(
        s.get_nanbox_f64(),
        hidden_writable_buffered_key(),
        box_pointer(merged.get_raw_const_ptr::<crate::array::ArrayHeader>() as *const u8),
    );
}

/// `uncork()` / `end()`'s full uncork: run the buffer once the stream is no
/// longer corked and no write is in flight.
pub(super) fn flush_writable_buffered(stream: f64) {
    if writable_corked_count(stream) > 0.0 {
        set_writable_corked_count(stream, 0.0);
    }
    clear_buffer(stream);
}

// ─── Transform: `_write` with kCallback, `_read` ─────────────────────────────

/// Transform's `_write`: run the transform with a callback that pushes its
/// value and completes the write, or holds the completion while the readable
/// side is full.
pub(super) fn invoke_transform_write(stream: f64, chunk: f64, enc: f64, len: f64, callback: f64) {
    let readable_length = readable_buffered_length(stream);
    if has_truthy_hidden(stream, hidden_transform_passthrough_key()) {
        // PassThrough's `_transform` is `cb(null, chunk)`.
        transform_write_done(stream, len, callback, readable_length, chunk);
        return;
    }
    if let Some(transform) = transform_hidden_callback(stream) {
        let cb = js_closure_alloc(
            crate::fn_info!(transform_write_callback, 2; with_declared(2)),
            4,
        );
        js_closure_set_capture_f64(cb, 0, stream);
        js_closure_set_capture_f64(cb, 1, len);
        js_closure_set_capture_f64(cb, 2, callback);
        js_closure_set_capture_f64(cb, 3, readable_length);
        let cb_value = f64::from_bits(JSValue::pointer(cb as *const u8).bits());
        let args = [chunk, enc, cb_value];
        unsafe {
            let _ = crate::closure::native_call_value_this(
                transform,
                crate::closure::JsThis::from_f64(stream),
                args.as_ptr(),
                args.len(),
            );
        }
        return;
    }
    throw_missing_stream_method("The _transform() method is not implemented");
}

fn readable_buffered_length(stream: f64) -> f64 {
    get_hidden_value(stream, hidden_buffered_key()).unwrap_or(0.0)
}

extern "C" fn transform_write_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
    value: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let len = js_closure_get_capture_f64(closure, 1);
    let callback = js_closure_get_capture_f64(closure, 2);
    let length_before = js_closure_get_capture_f64(closure, 3);
    if err.to_bits() != TAG_UNDEFINED && err.to_bits() != TAG_NULL {
        complete_writable_write(stream, len, callback, err);
        return f64::from_bits(TAG_UNDEFINED);
    }
    transform_write_done(stream, len, callback, length_before, value);
    f64::from_bits(TAG_UNDEFINED)
}

/// The tail of Transform's `_write` callback (`transform.js`).
fn transform_write_done(stream: f64, len: f64, callback: f64, length_before: f64, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    push_callback_value(s.get_nanbox_f64(), value);
    let st = s.get_nanbox_f64();
    let rlen = readable_buffered_length(st);
    let hwm = get_hidden_value(st, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    if has_truthy_hidden(st, hidden_key(b"readableEnded")) {
        // `push(null)` ran in the transform: node delays the callback a tick
        // so the new state propagates first.
        let held = js_closure_alloc(crate::fn_info!(ns_transform_held_callback, 0), 3);
        js_closure_set_capture_f64(held, 0, st);
        js_closure_set_capture_f64(held, 1, len);
        js_closure_set_capture_f64(held, 2, callback.get_nanbox_f64());
        crate::builtins::js_queue_microtask(held as i64);
    } else if stream_hidden_ended(st) || length_before == rlen || rlen < hwm {
        complete_writable_write(
            st,
            len,
            callback.get_nanbox_f64(),
            f64::from_bits(TAG_UNDEFINED),
        );
    } else {
        // node's `this[kCallback] = callback`.
        let held = js_closure_alloc(crate::fn_info!(ns_transform_held_callback, 0), 3);
        js_closure_set_capture_f64(held, 0, s.get_nanbox_f64());
        js_closure_set_capture_f64(held, 1, len);
        js_closure_set_capture_f64(held, 2, callback.get_nanbox_f64());
        set_internal_value(
            s.get_nanbox_f64(),
            TRANSFORM_HELD_CALLBACK_KEY,
            box_pointer(held as *const u8),
        );
    }
}

extern "C" fn ns_transform_held_callback(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let stream = js_closure_get_capture_f64(closure, 0);
    let len = js_closure_get_capture_f64(closure, 1);
    let callback = js_closure_get_capture_f64(closure, 2);
    complete_writable_write(stream, len, callback, f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(TAG_UNDEFINED)
}

/// node's Transform `_read` (and a native stream's resume): the readable side
/// was consumed below its highWaterMark, so a held write completion runs and
/// a parked codec resumes. Called wherever the runtime consumes readable data.
pub(super) fn readable_maybe_read_more(stream: f64) {
    if !js_node_stream_has_writable_side(stream) {
        return;
    }
    let rlen = readable_buffered_length(stream);
    let hwm = get_hidden_value(stream, hidden_hwm_key()).unwrap_or_else(|| default_hwm(false));
    if rlen >= hwm {
        return;
    }
    if let Some(held) = get_hidden_value(stream, hidden_key(TRANSFORM_HELD_CALLBACK_KEY))
        .filter(|v| is_callable_value(*v))
    {
        set_internal_value(
            stream,
            TRANSFORM_HELD_CALLBACK_KEY,
            f64::from_bits(TAG_UNDEFINED),
        );
        unsafe {
            let _ = crate::closure::js_native_call_value(
                held,
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            );
        }
    }
    super::native_hooks::resume_parked(stream);
}
