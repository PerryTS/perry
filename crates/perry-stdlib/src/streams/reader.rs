//! `ReadableStreamDefaultReader`: `read()`, `releaseLock()`, `closed`, `cancel()`, and the async iterator over a readable stream.

use super::{
    box_promise, build_iter_result, close_pending, expando, idalloc, js_array_alloc, js_array_push,
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    js_nanbox_get_pointer, js_object_alloc, js_object_set_field, js_object_set_keys,
    js_promise_new, js_promise_reason, js_promise_reject, js_promise_resolve, js_promise_state,
    js_promise_then, js_promise_value, js_readable_stream_cancel_inner,
    js_readable_stream_get_reader, js_stream_unwrap_handle, js_string_from_bytes,
    js_value_is_promise, maybe_pull, reader_rec, reject_type_error, rs_rec, stream_id,
    stream_value, transform, ClosureHeader, JSValue, JsFunctionInfo, Promise, ReadableState,
    ReaderRef, RsRef, TAG_UNDEFINED,
};

pub(super) extern "C" fn readable_from_chunk_fulfilled(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let promise = perry_runtime::closure::closure_capture_ref(closure, 0) as *mut Promise;
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(promise);
        let result = build_iter_result(value.to_bits(), false);
        js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
    }
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) extern "C" fn readable_from_chunk_rejected(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    reason: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let promise = perry_runtime::closure::closure_capture_ref(closure, 0) as *mut Promise;
    js_promise_reject(promise, reason);
    f64::from_bits(TAG_UNDEFINED)
}

pub(super) unsafe fn resolve_reader_read_value(promise: *mut Promise, value_bits: u64) {
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] read RESOLVED value_bits={value_bits:#018x}");
        }
    }
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let promise = scope.root_raw_mut_ptr(promise);
    let value_root = scope.root_nanbox_u64(value_bits);
    let value = f64::from_bits(value_root.get_nanbox_u64());
    if js_value_is_promise(value) == 0 {
        let result = build_iter_result(value_bits, false);
        js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
        return;
    }

    let inner = js_nanbox_get_pointer(value) as *mut Promise;
    if inner.is_null() {
        let result = build_iter_result(value_bits, false);
        js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
        return;
    }

    let inner = scope.root_raw_mut_ptr(inner);
    match js_promise_state(inner.get_raw_mut_ptr()) {
        1 => {
            let value = js_promise_value(inner.get_raw_mut_ptr());
            let result = build_iter_result(value.to_bits(), false);
            js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
        }
        2 => {
            js_promise_reject(
                promise.get_raw_mut_ptr(),
                js_promise_reason(inner.get_raw_mut_ptr()),
            );
        }
        _ => {
            let fulfill =
                js_closure_alloc(perry_runtime::fn_info!(readable_from_chunk_fulfilled, 1), 1);
            let fulfill = scope.root_raw_mut_ptr(fulfill);
            let reject =
                js_closure_alloc(perry_runtime::fn_info!(readable_from_chunk_rejected, 1), 1);
            let reject = scope.root_raw_mut_ptr(reject);
            perry_runtime::closure::closure_set_capture_ref(
                fulfill.get_raw_mut_ptr(),
                0,
                promise.get_raw_mut_ptr::<Promise>() as i64,
            );
            perry_runtime::closure::closure_set_capture_ref(
                reject.get_raw_mut_ptr(),
                0,
                promise.get_raw_mut_ptr::<Promise>() as i64,
            );
            let _ = js_promise_then(
                inner.get_raw_mut_ptr(),
                fulfill.get_raw_mut_ptr(),
                reject.get_raw_mut_ptr(),
            );
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_read(reader_handle: f64) -> *mut Promise {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let promise = scope.root_raw_mut_ptr(js_promise_new());
    let reader_handle = js_stream_unwrap_handle(reader_handle);
    let reader_id = stream_id(reader_handle);
    if std::env::var("PERRY_JSON_PATH").as_deref() == Ok("1") {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        if N.fetch_add(1, Ordering::Relaxed) < 40 {
            eprintln!("[STREAM] reader_read reader_id={reader_id:#x}");
        }
    }
    let stream_id = match reader_rec(ReaderRef(reader_id)).as_deref() {
        Some(r) if r.locked => r.stream_handle,
        Some(_) => {
            reject_type_error(
                promise.get_raw_mut_ptr(),
                "Reader is no longer locked to a stream",
            );
            return promise.get_raw_mut_ptr();
        }
        None => {
            reject_type_error(promise.get_raw_mut_ptr(), "Invalid reader");
            return promise.get_raw_mut_ptr();
        }
    };
    let mut closed_rejection: Option<(usize, u64)> = None;
    let mut closed_resolution: Option<usize> = None;
    let outcome: Option<(u64, bool, bool)> = {
        match rs_rec(RsRef(stream_id)).as_deref_mut() {
            Some(s) => {
                s.disturbed = true;
                if let Some(c) = s.pop_chunk() {
                    if s.chunks.is_empty() {
                        if let Some(error) = s.pending_error_after_chunks.take() {
                            s.state = ReadableState::Errored;
                            s.error_value = error;
                            s.clear_algorithms();
                            if let Some(reader_id) = s.reader_handle {
                                closed_rejection = Some((reader_id, error));
                            }
                        } else if s.state == ReadableState::Closed {
                            if let Some(reader_id) = s.reader_handle {
                                closed_resolution = Some(reader_id);
                            }
                        }
                    }
                    Some((c, false, false))
                } else if s.state == ReadableState::Closed {
                    Some((TAG_UNDEFINED, true, false))
                } else if s.state == ReadableState::Errored {
                    Some((s.error_value, false, true))
                } else {
                    s.pending_reads.push_back(promise.get_raw_mut_ptr());
                    None
                }
            }
            None => Some((TAG_UNDEFINED, true, false)),
        }
    };
    let value_root = scope.root_nanbox_u64(outcome.map(|v| v.0).unwrap_or(TAG_UNDEFINED));
    // Consumer progress: if this readable is a transform's output and its
    // queue just drained (pop emptied it, or the read parked on an empty
    // queue), release write promises parked on backpressure.
    transform::transform_release_writes(stream_id);
    if let Some((reader_id, reason)) = closed_rejection {
        let p = reader_rec(ReaderRef(reader_id))
            .as_deref()
            .map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_reject(p, f64::from_bits(reason));
        }
    }
    if let Some(reader_id) = closed_resolution {
        let p = reader_rec(ReaderRef(reader_id))
            .as_deref()
            .map(|r| r.closed_promise);
        if let Some(p) = p {
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
        }
        close_pending(stream_id);
    }
    match outcome {
        Some((_, _, true)) => {
            let value = value_root.get_nanbox_u64();
            js_promise_reject(promise.get_raw_mut_ptr(), f64::from_bits(value));
        }
        Some((_, done, false)) => {
            let value = value_root.get_nanbox_u64();
            if done {
                let result = build_iter_result(value, true);
                js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
            } else {
                resolve_reader_read_value(promise.get_raw_mut_ptr(), value);
            }
        }
        None => {}
    }
    maybe_pull(stream_id);
    promise.get_raw_mut_ptr()
}

pub(super) fn resolved_done_promise() -> f64 {
    unsafe {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise = scope.root_raw_mut_ptr(js_promise_new());
        let result = build_iter_result(TAG_UNDEFINED, true);
        js_promise_resolve(promise.get_raw_mut_ptr(), f64::from_bits(result));
        box_promise(promise.get_raw_mut_ptr())
    }
}

pub(super) fn closure_capture_value(info: *const JsFunctionInfo, value: f64) -> *mut ClosureHeader {
    let closure = js_closure_alloc(info, 1);
    js_closure_set_capture_ptr(closure, 0, value.to_bits() as i64);
    closure
}

pub(super) fn closure_capture_value_get(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let bits = js_closure_get_capture_ptr(closure, 0) as u64;
    f64::from_bits(bits)
}

pub(super) extern "C" fn readable_stream_iterator_next(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let reader = closure_capture_value_get(closure);
    if reader.to_bits() == TAG_UNDEFINED {
        return resolved_done_promise();
    }
    unsafe { box_promise(js_reader_read(reader)) }
}

pub(super) extern "C" fn readable_stream_iterator_return(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let reader = closure_capture_value_get(closure);
    if reader.to_bits() != TAG_UNDEFINED {
        unsafe {
            let _ = js_reader_release_lock(reader);
        }
    }
    resolved_done_promise()
}

pub(super) extern "C" fn readable_stream_iterator_self(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    closure_capture_value_get(closure)
}

pub(super) unsafe fn build_readable_stream_iterator(stream_handle: f64) -> f64 {
    let reader = js_readable_stream_get_reader(stream_handle);
    let obj = js_object_alloc(0, 2);
    let keys = js_array_alloc(2);
    let k_next = js_string_from_bytes(b"next".as_ptr(), 4);
    let k_return = js_string_from_bytes(b"return".as_ptr(), 6);
    js_array_push(keys, JSValue::string_ptr(k_next));
    js_array_push(keys, JSValue::string_ptr(k_return));
    js_object_set_field(
        obj,
        0,
        JSValue::pointer(closure_capture_value(
            perry_runtime::fn_info!(readable_stream_iterator_next, 0; with_declared(0)),
            reader,
        ) as *const u8),
    );
    js_object_set_field(
        obj,
        1,
        JSValue::pointer(closure_capture_value(
            perry_runtime::fn_info!(readable_stream_iterator_return, 0; with_declared(0)),
            reader,
        ) as *const u8),
    );
    js_object_set_keys(obj, keys);
    let iterator = f64::from_bits(JSValue::object_ptr(obj as *mut u8).bits());

    let async_iterator = perry_runtime::symbol::well_known_symbol("asyncIterator");
    if !async_iterator.is_null() {
        let self_closure = closure_capture_value(
            perry_runtime::fn_info!(readable_stream_iterator_self, 0; with_declared(0)),
            iterator,
        );
        let symbol_value = f64::from_bits(JSValue::pointer(async_iterator as *const u8).bits());
        let closure_value = f64::from_bits(JSValue::pointer(self_closure as *const u8).bits());
        perry_runtime::symbol::js_object_set_symbol_property(iterator, symbol_value, closure_value);
    }

    iterator
}

#[no_mangle]
pub unsafe extern "C" fn js_readable_stream_values(stream_handle: f64) -> f64 {
    build_readable_stream_iterator(stream_handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_release_lock(reader_handle: f64) -> f64 {
    let reader_id = stream_id(reader_handle);
    let stream_id = {
        match reader_rec(ReaderRef(reader_id)).as_deref_mut() {
            Some(r) => {
                r.locked = false;
                r.stream_handle
            }
            None => return f64::from_bits(TAG_UNDEFINED),
        }
    };
    if let Some(s) = rs_rec(RsRef(stream_id)).as_deref_mut() {
        s.reader_handle = None;
    }
    // #5437: the reader instance is done — drop any expando entries keyed by
    // its handle id so the table doesn't retain them for the process lifetime.
    expando::stream_expando_clear(reader_id);
    idalloc::retire_reader_if_released(reader_id);
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_closed(reader_handle: f64) -> *mut Promise {
    let reader_id = stream_id(reader_handle);
    match reader_rec(ReaderRef(reader_id)).as_deref() {
        Some(r) => r.closed_promise,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            p
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_reader_cancel(reader_handle: f64, reason: f64) -> *mut Promise {
    let reader_id = stream_id(reader_handle);
    let stream_id = match reader_rec(ReaderRef(reader_id)).as_deref() {
        Some(r) => r.stream_handle,
        None => {
            let p = js_promise_new();
            js_promise_resolve(p, f64::from_bits(TAG_UNDEFINED));
            return p;
        }
    };
    js_readable_stream_cancel_inner(stream_value(stream_id), reason, true)
}
