//! `ReadableStream.pipeTo` implementation details.

use super::{
    box_promise, next_id, reject_type_error, transform_close, transform_write, ReadableState,
    WritableState, NEXT_STREAM_ID, READABLE_STREAMS, TAG_UNDEFINED, TRANSFORM_PAIRS,
    WRITABLE_STREAMS,
};
use perry_runtime::{
    js_closure_call0, js_closure_call1, js_nanbox_get_pointer, js_promise_new, js_promise_reject,
    js_promise_resolve, ClosureHeader, JSValue, ObjectHeader, Promise,
};

#[derive(Clone, Copy)]
struct PipeLockIds {
    reader_id: usize,
    writer_id: usize,
}

fn acquire_pipe_locks(readable_id: usize, writable_id: usize) -> Result<PipeLockIds, &'static str> {
    let reader_id = next_id(&NEXT_STREAM_ID);
    let writer_id = next_id(&NEXT_STREAM_ID);
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
    Ok(PipeLockIds {
        reader_id,
        writer_id,
    })
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
        let result = run_readable_stream_pipe_to(r_id, w_id, prevent_close);
        release_pipe_locks(r_id, w_id, locks);
        match result {
            Ok(()) => js_promise_resolve(promise, f64::from_bits(TAG_UNDEFINED)),
            Err(reason) => js_promise_reject(promise, f64::from_bits(reason)),
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

unsafe fn run_readable_stream_pipe_to(
    readable_id: usize,
    writable_id: usize,
    prevent_close: bool,
) -> Result<(), u64> {
    loop {
        let chunk_or_done: Result<u64, bool> = {
            let mut g = READABLE_STREAMS.lock().unwrap();
            match g.get_mut(&readable_id) {
                Some(s) => {
                    if let Some(c) = s.chunks.pop_front() {
                        Ok(c)
                    } else if s.state == ReadableState::Closed {
                        Err(true)
                    } else if s.state == ReadableState::Errored {
                        return Err(s.error_value);
                    } else {
                        Err(true)
                    }
                }
                None => Err(true),
            }
        };
        match chunk_or_done {
            Ok(chunk) => {
                // TransformStream's writable side has write_cb=0, so route
                // through transform_write to run the user transform function.
                if TRANSFORM_PAIRS.lock().unwrap().contains_key(&writable_id) {
                    let _ = transform_write(writable_id, f64::from_bits(chunk));
                } else {
                    let write_cb = WRITABLE_STREAMS
                        .lock()
                        .unwrap()
                        .get(&writable_id)
                        .map(|w| w.write_cb)
                        .unwrap_or(0);
                    if write_cb != 0 {
                        js_closure_call1(write_cb as *const ClosureHeader, f64::from_bits(chunk));
                    }
                }
            }
            Err(_done) => break,
        }
    }

    if prevent_close {
        return Ok(());
    }

    if TRANSFORM_PAIRS.lock().unwrap().contains_key(&writable_id) {
        let _ = transform_close(writable_id);
    } else {
        let close_cb = WRITABLE_STREAMS
            .lock()
            .unwrap()
            .get(&writable_id)
            .map(|w| w.close_cb)
            .unwrap_or(0);
        if close_cb != 0 {
            js_closure_call0(close_cb as *const ClosureHeader);
        }
        if let Some(w) = WRITABLE_STREAMS.lock().unwrap().get_mut(&writable_id) {
            w.state = WritableState::Closed;
            let cp = w.closed_promise;
            js_promise_resolve(cp, f64::from_bits(TAG_UNDEFINED));
        }
    }
    Ok(())
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
