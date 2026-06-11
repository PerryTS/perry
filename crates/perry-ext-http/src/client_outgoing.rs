//! #4909 — client `OutgoingMessage` write/end callback + backpressure +
//! `setTimeout` surface. Mirrors the #4954 server-side fix
//! (`js_node_http_res_write_full` / `js_node_http_res_end_full`) on the
//! `http.ClientRequest` path: the static native-dispatch table routed
//! `req.write` / `req.end` to single-arg entry points that dropped the
//! `(encoding?, callback?)` tail, returned the handle from `write()`
//! (truthy forever, so `while (req.write(buf))` producer loops never
//! terminated), and silently discarded Buffer chunks.

use super::*;

/// Node's default `highWaterMark` for an HTTP `OutgoingMessage` (16 KiB).
/// `req.write()` returns `false` once the buffered body grows past this,
/// signalling backpressure so producer loops terminate.
const DEFAULT_HIGH_WATER_MARK: usize = 16 * 1024;

extern "C" {
    fn js_value_is_closure(value_bits: i64) -> i32;
    fn js_buffer_is_buffer(ptr: i64) -> i32;
}

/// Return the closure pointer carried by `value_bits` if it is a real
/// callable (POINTER_TAG + closure magic), else 0. `js_value_is_closure`
/// keeps a Buffer/object chunk — also POINTER_TAG — from being mistaken
/// for a callback.
pub(crate) fn callback_from_bits(value_bits: i64) -> i64 {
    if unsafe { js_value_is_closure(value_bits) } != 0 {
        (value_bits as u64 & PTR_MASK) as i64
    } else {
        0
    }
}

/// Pick the callback from a `(encoding?, callback?)` trailing arg pair,
/// the later slot first — mirroring Node's `(chunk, encoding, callback)`
/// rule. A string encoding is not callable, so it is skipped.
pub(crate) fn pick_trailing_callback(arg2: i64, arg3: i64) -> i64 {
    let c3 = callback_from_bits(arg3);
    if c3 != 0 {
        c3
    } else {
        callback_from_bits(arg2)
    }
}

/// String / Buffer chunk → body bytes. Buffers must be probed first: a
/// Buffer is POINTER_TAG just like some heap strings, and
/// `extract_string_value` would misread its `BufferHeader` as a
/// `StringHeader` (the same layout confusion as #1124).
pub(crate) unsafe fn chunk_to_bytes(value: f64) -> Option<Vec<u8>> {
    let bits = value.to_bits();
    if bits == TAG_UNDEFINED || bits == TAG_NULL {
        return None;
    }
    if bits >> 48 == 0x7FFD {
        let raw = (bits & PTR_MASK) as i64;
        if js_buffer_is_buffer(raw) != 0 {
            return perry_ffi::read_buffer_bytes(raw as *const perry_ffi::BufferHeader)
                .map(|b| b.to_vec());
        }
    }
    extract_string_value(value).map(String::into_bytes)
}

/// `req.write(chunk[, encoding][, callback])` — the full Node surface
/// routed from the static native dispatch table. The callback is queued
/// (it fires in order when the body flushes at `end()`); returns a
/// NaN-boxed boolean: `false` once the buffered body passes the 16 KiB
/// high-water mark (Node's backpressure signal), else `true`.
///
/// # Safety
///
/// FFI entry; `handle` must be a live `ClientRequestHandle` (or absent).
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_write_full(
    handle: Handle,
    chunk: f64,
    arg2: i64,
    arg3: i64,
) -> f64 {
    let callback = pick_trailing_callback(arg2, arg3);
    let bytes = chunk_to_bytes(chunk);
    let mut below_hwm = true;
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if !req.ended {
            if let Some(b) = &bytes {
                req.body.extend_from_slice(b);
            }
            if callback != 0 {
                req.pending_write_callbacks.push(callback);
            }
            below_hwm = req.body.len() <= DEFAULT_HIGH_WATER_MARK;
        }
    });
    f64::from_bits(if below_hwm { TAG_TRUE } else { TAG_FALSE })
}

/// `req.end([chunk][, encoding][, callback])` — the full Node surface
/// routed from the static native dispatch table. Handles the `end(cb)`
/// form (callback in the first slot) as well as
/// `end(chunk[, encoding][, callback])`. The queued write callbacks, the
/// `'finish'` listeners and the end callback fire on the next drain via
/// `PendingHttpEvent::Flushed` (queued by `client_request_end_impl`).
///
/// # Safety
///
/// FFI entry; `handle` must be a live `ClientRequestHandle` (or absent).
#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_end_full(
    handle: Handle,
    chunk: f64,
    arg2: i64,
    arg3: i64,
) -> Handle {
    let first_cb = callback_from_bits(chunk.to_bits() as i64);
    let (real_chunk, callback) = if first_cb != 0 {
        (f64::from_bits(TAG_UNDEFINED), first_cb)
    } else {
        (chunk, pick_trailing_callback(arg2, arg3))
    };
    if let Some(b) = chunk_to_bytes(real_chunk) {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            if !req.ended {
                req.body.extend_from_slice(&b);
            }
        });
    }
    if callback != 0 {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            if !req.ended {
                req.end_callback = callback;
            }
        });
    }
    client_request_end_impl(handle, f64::from_bits(TAG_UNDEFINED))
}

/// `req.setTimeout(msecs[, callback])` — the full Node surface. The
/// callback registers as a `'timeout'` listener, and a real timer is
/// armed immediately: Node's inactivity timer runs on the socket from
/// assignment, so `'timeout'` must fire even when the request was never
/// `end()`ed (the transport deadline alone only covered dispatched
/// requests — the canonical `req.setTimeout(n); req.on('timeout', …)`
/// pattern on a never-responding server hung forever, #4909).
///
/// # Safety
///
/// FFI entry; `handle` must be a live `ClientRequestHandle` (or absent).
#[no_mangle]
pub unsafe extern "C" fn js_http_set_timeout_full(
    handle: Handle,
    ms: f64,
    callback_bits: i64,
) -> Handle {
    let cb = callback_from_bits(callback_bits);
    if cb != 0 {
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
            req.listeners
                .entry("timeout".to_string())
                .or_default()
                .push(cb);
        });
    }
    client_request_set_timeout_impl(handle, ms);
    let effective =
        with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| req.timeout_ms).flatten();
    if let Some(ms) = effective {
        arm_client_timeout(handle, ms);
    }
    handle
}

/// Arm a one-shot timer that pushes `PendingHttpEvent::Timeout` after
/// `ms` milliseconds. The drain dedupes (`timeout_fired`) and suppresses
/// stale timers (`completed`), so over-arming is harmless — rescheduled
/// `setTimeout` calls and the per-dispatch transport deadline can all
/// race the same request safely.
pub(crate) fn arm_client_timeout(request_handle: Handle, ms: u64) {
    spawn_blocking(move || {
        // Defeat LTO dead-stripping of tokio's CONTEXT statics — same
        // workaround dispatch_request needs (see spawn_socket_runner).
        let try_h = tokio::runtime::Handle::try_current();
        std::hint::black_box(&try_h);
        if try_h.is_err() {
            return;
        }
        let handle = tokio::runtime::Handle::current();
        let jh = handle.spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
            push_event(PendingHttpEvent::Timeout { request_handle });
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}
