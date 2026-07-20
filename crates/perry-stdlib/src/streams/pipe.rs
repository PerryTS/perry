//! `ReadableStream.pipeTo` implementation details.

use super::{
    box_promise, idalloc, js_writable_stream_close, maybe_pull, reject_type_error, transform_close,
    writable_stream_write, ReadableState, READABLE_STREAMS, TAG_UNDEFINED, TRANSFORM_PAIRS,
    WRITABLE_STREAMS,
};
use perry_runtime::{
    js_nanbox_get_pointer, js_object_get_field_by_name, js_promise_new, js_promise_reject,
    js_promise_resolve, js_string_from_bytes, ClosureHeader, JSValue, ObjectHeader, Promise,
};

const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[derive(Clone, Copy)]
struct PipeLockIds {
    reader_id: usize,
    writer_id: usize,
}

fn acquire_pipe_locks(readable_id: usize, writable_id: usize) -> Result<PipeLockIds, &'static str> {
    let reader_id = idalloc::next_pipe_lock_id();
    let writer_id = idalloc::next_pipe_lock_id();
    // #6602: on failure the two freshly minted ids were never stamped (or were
    // just unstamped) — recycle them. Runs after every registry guard in
    // `try_acquire_pipe_locks` is released; a quarantine overflow inside the
    // retire takes registry locks for eviction cleanup.
    if let Err(message) = try_acquire_pipe_locks(readable_id, writable_id, reader_id, writer_id) {
        retire_pipe_lock_ids(reader_id, writer_id);
        return Err(message);
    }
    Ok(PipeLockIds {
        reader_id,
        writer_id,
    })
}

fn try_acquire_pipe_locks(
    readable_id: usize,
    writable_id: usize,
    reader_id: usize,
    writer_id: usize,
) -> Result<(), &'static str> {
    {
        let mut readable = READABLE_STREAMS.lock().unwrap();
        match readable.get_mut(&readable_id) {
            Some(s) if s.reader_handle.is_none() => {
                s.reader_handle = Some(reader_id);
            }
            Some(_) => return Err("ReadableStream is locked"),
            None => return Err("Invalid ReadableStream"),
        }
    }
    {
        let mut writable = WRITABLE_STREAMS.lock().unwrap();
        match writable.get_mut(&writable_id) {
            Some(s) if s.writer_handle.is_none() => {
                s.writer_handle = Some(writer_id);
            }
            Some(_) => {
                if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&readable_id) {
                    if s.reader_handle == Some(reader_id) {
                        s.reader_handle = None;
                    }
                }
                return Err("WritableStream is locked");
            }
            None => {
                if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&readable_id) {
                    if s.reader_handle == Some(reader_id) {
                        s.reader_handle = None;
                    }
                }
                return Err("Invalid WritableStream");
            }
        }
    }
    Ok(())
}

/// #6602: pipe lock ids are stamped as lock markers but never own a registry
/// entry, so nothing else retires them — without this every pipeTo burned two
/// band ids for the life of the process. Retirement keys on the allocator's
/// ownership mark, so a duplicate release (close-fulfilled then a late
/// rejection) is a no-op.
fn retire_pipe_lock_ids(reader_id: usize, writer_id: usize) {
    idalloc::retire_pipe_lock_id(reader_id);
    idalloc::retire_pipe_lock_id(writer_id);
}

fn release_pipe_locks(readable_id: usize, writable_id: usize, locks: PipeLockIds) {
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&readable_id) {
        if s.reader_handle == Some(locks.reader_id) {
            s.reader_handle = None;
        }
    }
    if let Some(s) = WRITABLE_STREAMS.lock().unwrap().get_mut(&writable_id) {
        if s.writer_handle == Some(locks.writer_id) {
            s.writer_handle = None;
        }
    }
    retire_pipe_lock_ids(locks.reader_id, locks.writer_id);
}

#[inline]
fn promise_from_capture(closure: *const ClosureHeader, idx: u32) -> *mut Promise {
    let bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as u64;
    perry_runtime::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut Promise
}

fn capture_f64(closure: *const ClosureHeader, idx: u32) -> f64 {
    let bits = perry_runtime::closure::js_closure_get_capture_ptr(closure, idx) as u64;
    f64::from_bits(bits)
}

extern "C" fn readable_stream_pipe_to_microtask(closure: *const ClosureHeader) -> f64 {
    unsafe {
        let r_id = capture_f64(closure, 0) as usize;
        let w_id = capture_f64(closure, 1) as usize;
        let promise = promise_from_capture(closure, 2);
        let locks = PipeLockIds {
            reader_id: capture_f64(closure, 3) as usize,
            writer_id: capture_f64(closure, 4) as usize,
        };
        let prevent_close = perry_runtime::value::js_is_truthy(capture_f64(closure, 5)) != 0;
        pipe_step(r_id, w_id, promise, locks, prevent_close);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn readable_stream_pipe_to_read_fulfilled(
    closure: *const ClosureHeader,
    result: f64,
) -> f64 {
    unsafe {
        let r_id = capture_f64(closure, 0) as usize;
        let w_id = capture_f64(closure, 1) as usize;
        let promise = promise_from_capture(closure, 2);
        let locks = PipeLockIds {
            reader_id: capture_f64(closure, 3) as usize,
            writer_id: capture_f64(closure, 4) as usize,
        };
        let prevent_close = perry_runtime::value::js_is_truthy(capture_f64(closure, 5)) != 0;
        if perry_runtime::promise::js_promise_state(promise) != 0 {
            return f64::from_bits(TAG_UNDEFINED);
        }
        match pipe_iter_result(result) {
            Some((true, _)) => finish_pipe(r_id, w_id, promise, locks, prevent_close),
            Some((false, value)) => {
                pipe_write_then_continue(r_id, w_id, promise, locks, prevent_close, value)
            }
            None => reject_pipe(r_id, w_id, promise, locks, result.to_bits()),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn readable_stream_pipe_to_write_fulfilled(
    closure: *const ClosureHeader,
    _value: f64,
) -> f64 {
    unsafe {
        let r_id = capture_f64(closure, 0) as usize;
        let w_id = capture_f64(closure, 1) as usize;
        let promise = promise_from_capture(closure, 2);
        let locks = PipeLockIds {
            reader_id: capture_f64(closure, 3) as usize,
            writer_id: capture_f64(closure, 4) as usize,
        };
        let prevent_close = perry_runtime::value::js_is_truthy(capture_f64(closure, 5)) != 0;
        pipe_step(r_id, w_id, promise, locks, prevent_close);
    }
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn readable_stream_pipe_to_close_fulfilled(
    closure: *const ClosureHeader,
    _value: f64,
) -> f64 {
    let r_id = capture_f64(closure, 0) as usize;
    let w_id = capture_f64(closure, 1) as usize;
    let promise = promise_from_capture(closure, 2);
    let locks = PipeLockIds {
        reader_id: capture_f64(closure, 3) as usize,
        writer_id: capture_f64(closure, 4) as usize,
    };
    release_pipe_locks(r_id, w_id, locks);
    js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
    f64::from_bits(TAG_UNDEFINED)
}

extern "C" fn readable_stream_pipe_to_rejected(closure: *const ClosureHeader, reason: f64) -> f64 {
    unsafe {
        let r_id = capture_f64(closure, 0) as usize;
        let w_id = capture_f64(closure, 1) as usize;
        let promise = promise_from_capture(closure, 2);
        let locks = PipeLockIds {
            reader_id: capture_f64(closure, 3) as usize,
            writer_id: capture_f64(closure, 4) as usize,
        };
        reject_pipe(r_id, w_id, promise, locks, reason.to_bits());
    }
    f64::from_bits(TAG_UNDEFINED)
}

enum PipeReadStep {
    Chunk(u64),
    Done,
    Pending,
    Error(u64),
}

unsafe fn pipe_next_read(readable_id: usize) -> PipeReadStep {
    let mut g = READABLE_STREAMS.lock().unwrap();
    match g.get_mut(&readable_id) {
        Some(s) => {
            if let Some(c) = s.pop_chunk() {
                PipeReadStep::Chunk(c)
            } else if s.state == ReadableState::Closed {
                PipeReadStep::Done
            } else if s.state == ReadableState::Errored {
                PipeReadStep::Error(s.error_value)
            } else {
                PipeReadStep::Pending
            }
        }
        None => PipeReadStep::Done,
    }
}

unsafe fn pipe_step(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
) {
    if perry_runtime::promise::js_promise_state(promise) != 0 {
        return;
    }
    loop {
        let step = pipe_next_read(readable_id);
        // Pipe progress on this readable is consumer progress: release
        // transform writes parked on backpressure (chained
        // pipeThrough(...).pipeTo(...) drains a transform's readable through
        // here, never through js_reader_read).
        super::transform::transform_release_writes(readable_id);
        match step {
            PipeReadStep::Chunk(chunk) => {
                pipe_write_then_continue(
                    readable_id,
                    writable_id,
                    promise,
                    locks,
                    prevent_close,
                    chunk,
                );
                return;
            }
            PipeReadStep::Done => {
                finish_pipe(readable_id, writable_id, promise, locks, prevent_close);
                return;
            }
            PipeReadStep::Error(reason) => {
                reject_pipe(readable_id, writable_id, promise, locks, reason);
                return;
            }
            PipeReadStep::Pending => {
                wait_for_next_read(readable_id, writable_id, promise, locks, prevent_close);
                return;
            }
        }
    }
}

unsafe fn finish_pipe(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
) {
    if prevent_close {
        release_pipe_locks(readable_id, writable_id, locks);
        js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED));
        return;
    }

    let close_promise = if TRANSFORM_PAIRS.lock().unwrap().contains_key(&writable_id) {
        transform_close(writable_id)
    } else {
        js_writable_stream_close(writable_id as f64)
    };
    let fulfilled = pipe_closure(
        readable_stream_pipe_to_close_fulfilled as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    let rejected = pipe_closure(
        readable_stream_pipe_to_rejected as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_close_fulfilled as *const u8,
        1,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_rejected as *const u8,
        1,
    );
    let _ = perry_runtime::promise::js_promise_then(close_promise, fulfilled, rejected);
}

unsafe fn reject_pipe(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    reason: u64,
) {
    release_pipe_locks(readable_id, writable_id, locks);
    js_promise_reject(promise, f64::from_bits(reason));
}

/// Queue the next pipe cycle directly as a microtask (one tick), mirroring the
/// pipeTo entry's initial scheduling.
unsafe fn schedule_pipe_step(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
) {
    let closure =
        perry_runtime::closure::js_closure_alloc(readable_stream_pipe_to_microtask as *const u8, 6);
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_microtask as *const u8,
        0,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        0,
        (readable_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        1,
        (writable_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        2,
        box_promise(promise).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        3,
        (locks.reader_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        4,
        (locks.writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        5,
        (if prevent_close { 1.0 } else { 0.0f64 }).to_bits() as i64,
    );
    perry_runtime::builtins::js_queue_microtask(closure as i64);
}

unsafe fn pipe_write_then_continue(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
    chunk: u64,
) {
    let write_promise = writable_stream_write(writable_id, locks.writer_id, f64::from_bits(chunk));
    // Spec ReadableStreamPipeTo awaits only BACKPRESSURE (writer.ready), not
    // each write's completion. A sink that accepted the chunk synchronously
    // (write promise already fulfilled) must not cost an extra reaction tick —
    // Node's pipe pump runs read→write in lockstep with a racing consumer
    // (teepipe.js: 1 write/tick; awaiting each write made Perry ~3 ticks/write
    // and let a tee sibling's reader outrun the pipe — Next.js cold-start
    // head reorder). Chain on the write promise only while it is pending.
    if perry_runtime::promise::js_promise_state(write_promise) == 1 {
        // Tick parity (streamsuite teepipe/teepipe2 wcc/waa): when the
        // readable's queue is EMPTY, Node's pump has its next read parked
        // within the write-completion reaction, so the next delivery (a tee
        // fan-out) resolves it directly and the write lands one tick after
        // the sibling's read. Deferring the park through a queued step made
        // that write a tick late. Buffered chunks keep the queued step —
        // popping synchronously would bunch writes and break the 1/tick
        // write cadence.
        let park_now = {
            let g = READABLE_STREAMS.lock().unwrap();
            g.get(&readable_id)
                .map(|s| s.chunks.is_empty() && s.state == ReadableState::Readable)
                .unwrap_or(false)
        };
        if park_now {
            wait_for_next_read(readable_id, writable_id, promise, locks, prevent_close);
        } else {
            schedule_pipe_step(readable_id, writable_id, promise, locks, prevent_close);
        }
        return;
    }
    let fulfilled = pipe_closure(
        readable_stream_pipe_to_write_fulfilled as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    let rejected = pipe_closure(
        readable_stream_pipe_to_rejected as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_write_fulfilled as *const u8,
        1,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_rejected as *const u8,
        1,
    );
    let _ = perry_runtime::promise::js_promise_then(write_promise, fulfilled, rejected);
}

unsafe fn wait_for_next_read(
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
) {
    let read_promise = js_promise_new();
    if let Some(s) = READABLE_STREAMS.lock().unwrap().get_mut(&readable_id) {
        if s.state == ReadableState::Readable {
            s.pending_reads.push_back(read_promise);
        } else if s.state == ReadableState::Closed {
            let result = pipe_iter_result_object(TAG_UNDEFINED, true);
            js_promise_resolve(read_promise, f64::from_bits(result));
        } else {
            js_promise_reject(read_promise, f64::from_bits(s.error_value));
        }
    } else {
        let result = pipe_iter_result_object(TAG_UNDEFINED, true);
        js_promise_resolve(read_promise, f64::from_bits(result));
    }
    maybe_pull(readable_id);

    let fulfilled = pipe_closure(
        readable_stream_pipe_to_read_fulfilled as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    let rejected = pipe_closure(
        readable_stream_pipe_to_rejected as *const u8,
        readable_id,
        writable_id,
        promise,
        locks,
        prevent_close,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_read_fulfilled as *const u8,
        1,
    );
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_rejected as *const u8,
        1,
    );
    let _ = perry_runtime::promise::js_promise_then(read_promise, fulfilled, rejected);
}

unsafe fn pipe_iter_result(result: f64) -> Option<(bool, u64)> {
    let jsval = JSValue::from_bits(result.to_bits());
    if !jsval.is_pointer() {
        return None;
    }
    let obj = js_nanbox_get_pointer(result) as *const ObjectHeader;
    if obj.is_null() {
        return None;
    }
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let done = js_object_get_field_by_name(obj, done_key);
    let value = js_object_get_field_by_name(obj, value_key);
    Some((
        perry_runtime::value::js_is_truthy(f64::from_bits(done.bits())) != 0,
        value.bits(),
    ))
}

unsafe fn pipe_iter_result_object(value_bits: u64, done: bool) -> u64 {
    let obj = perry_runtime::js_object_alloc(0, 2);
    let keys = perry_runtime::js_array_alloc(2);
    let k_value = js_string_from_bytes(b"value".as_ptr(), 5);
    let k_done = js_string_from_bytes(b"done".as_ptr(), 4);
    perry_runtime::js_array_push(keys, JSValue::string_ptr(k_value));
    perry_runtime::js_array_push(keys, JSValue::string_ptr(k_done));
    perry_runtime::js_object_set_field(obj, 0, JSValue::from_bits(value_bits));
    perry_runtime::js_object_set_field(
        obj,
        1,
        JSValue::from_bits(if done { TAG_TRUE } else { TAG_FALSE }),
    );
    perry_runtime::js_object_set_keys(obj, keys);
    JSValue::object_ptr(obj as *mut u8).bits()
}

fn pipe_closure(
    func: *const u8,
    readable_id: usize,
    writable_id: usize,
    promise: *mut Promise,
    locks: PipeLockIds,
    prevent_close: bool,
) -> *mut perry_runtime::ClosureHeader {
    let closure = perry_runtime::closure::js_closure_alloc(func, 6);
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        0,
        (readable_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        1,
        (writable_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        2,
        box_promise(promise).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        3,
        (locks.reader_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        4,
        (locks.writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        5,
        (if prevent_close { 1.0 } else { 0.0f64 }).to_bits() as i64,
    );
    closure
}

/// `readable.pipeTo(writable)` acquires the source/destination locks
/// immediately, then drains the current buffered readable queue into the
/// writable on the next microtask. Deferring the drain keeps `.locked`
/// observable until the returned Promise settles, matching Web Streams'
/// in-flight pipe contract while preserving Perry's buffered model.
#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_pipe_to(
    readable_handle: f64,
    writable_handle: f64,
    options: f64,
) -> *mut Promise {
    let promise = js_promise_new();
    let r_id = readable_handle as usize;
    let w_id = writable_handle as usize;
    let prevent_close = pipe_option_truthy(options, b"preventClose");
    if pipe_signal_is_aborted(options) {
        js_promise_reject(promise, perry_runtime::url::js_abort_error_value());
        return promise;
    }

    let locks = match acquire_pipe_locks(r_id, w_id) {
        Ok(locks) => locks,
        Err(message) => {
            reject_type_error(promise, message);
            return promise;
        }
    };

    let closure =
        perry_runtime::closure::js_closure_alloc(readable_stream_pipe_to_microtask as *const u8, 6);
    perry_runtime::closure::js_register_closure_arity(
        readable_stream_pipe_to_microtask as *const u8,
        0,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 0, (r_id as f64).to_bits() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(closure, 1, (w_id as f64).to_bits() as i64);
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        2,
        box_promise(promise).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        3,
        (locks.reader_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        4,
        (locks.writer_id as f64).to_bits() as i64,
    );
    perry_runtime::closure::js_closure_set_capture_ptr(
        closure,
        5,
        (if prevent_close { 1.0 } else { 0.0f64 }).to_bits() as i64,
    );
    perry_runtime::builtins::js_queue_microtask(closure as i64);

    promise
}

unsafe fn pipe_signal_is_aborted(options: f64) -> bool {
    let signal = pipe_option_value(options, b"signal");
    let jsval = JSValue::from_bits(signal.to_bits());
    if !jsval.is_pointer() {
        return false;
    }
    let signal_ptr = js_nanbox_get_pointer(signal) as *mut ObjectHeader;
    if signal_ptr.is_null() {
        return false;
    }
    perry_runtime::url::js_abort_signal_is_aborted(signal_ptr) != 0
}

unsafe fn pipe_option_truthy(options: f64, name: &[u8]) -> bool {
    let value = pipe_option_value(options, name);
    perry_runtime::value::js_is_truthy(value) != 0
}

unsafe fn pipe_option_value(options: f64, name: &[u8]) -> f64 {
    perry_runtime::value::js_get_property(options, name.as_ptr() as i64, name.len() as i64)
}
