//! `res.end()` — the tail of a server response: flush, then run the queued
//! write callbacks, the end callback and the `'finish'` / `'close'` listeners
//! in Node's order.
//!
//! Split out of `response.rs` (which sits at the 2,000-line lint gate) when
//! #8163 gave this family its rooting discipline. The four entry points share
//! `EndTail`, which parks every snapshot the sequence consumes in the
//! runtime's transient-root stack before any of it crosses a JS call.

use hyper::StatusCode;
use perry_ffi::{get_handle, get_handle_mut, JsClosure, JsValue, RawClosureHeader};

use crate::server::request::emit_no_arg_to_listeners;
use crate::server::response::{
    callback_from_bits, finalize_buffered_end, pick_trailing_callback, socket_write_str,
    take_event_listeners, ServerResponse,
};
use crate::server::types::{jsvalue_to_body_bytes, TAG_UNDEFINED};

/// `res.end([chunk][, encoding][, callback])` — the full Node surface routed
/// from the static native dispatch table. Handles the `end(cb)` form (callback
/// in the first slot) as well as `end(chunk[, encoding][, callback])`. Queued
/// write callbacks fire first (in order), then the end callback, then the
/// `'finish'`/`'close'` listeners — Node's ordering where `'finish'` never
/// precedes the end callback (#4909).
///
/// # Safety
/// FFI entry; `handle` must be a live `ServerResponse` handle (or absent).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_end_full(handle: i64, chunk: f64, arg2: i64, arg3: i64) {
    // `end(cb)` passes the callback as the first arg; otherwise it trails.
    let first_cb = callback_from_bits(chunk.to_bits() as i64);
    let (real_chunk, callback) = if first_cb != 0 {
        (f64::from_bits(TAG_UNDEFINED), first_cb)
    } else {
        (chunk, pick_trailing_callback(arg2, arg3))
    };

    let is_standalone = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.standalone)
        .unwrap_or(false);
    if is_standalone {
        // standalone_end already runs write cbs → end cb → listeners in order.
        standalone_end(handle, real_chunk, callback);
        return;
    }

    let (finish_listeners, close_listeners) =
        finalize_buffered_end(handle, real_chunk).unwrap_or_default();
    let write_cbs = get_handle_mut::<ServerResponse>(handle)
        .map(|sr| std::mem::take(&mut sr.pending_write_callbacks))
        .unwrap_or_default();
    // #8163: every snapshot crosses the JS calls below — root them all first.
    let scope = perry_ffi::TransientRootScope::enter();
    let tail = EndTail::root(
        &scope,
        &write_cbs,
        callback,
        &finish_listeners,
        &close_listeners,
    );
    // Node order: queued write callbacks flush, then `'finish'` listeners, then
    // the end callback, then `'close'`. The end cb fires *after* `'finish'` so
    // a `res.on('finish')` handler that inspects end-callback state sees the
    // same interleaving as Node.
    tail.run_write_callbacks();
    tail.emit_finish();
    tail.run_end_callback();
    tail.emit_close();
}

/// `res.end(chunk?)` — append final chunk + flush the response back
/// to hyper through the oneshot channel + fire `'finish'` and
/// `'close'` listeners.
#[no_mangle]
pub extern "C" fn js_node_http_res_end(handle: i64, chunk: f64) {
    if let Some((finish_listeners, close_listeners)) = finalize_buffered_end(handle, chunk) {
        // #8163: the `'close'` snapshot crosses the `'finish'` emits.
        let scope = perry_ffi::TransientRootScope::enter();
        let tail = EndTail::root(&scope, &[], 0, &finish_listeners, &close_listeners);
        tail.emit_finish();
        tail.emit_close();
    }
}

/// `res.end([chunk][, callback])` — callback-aware variant. Standalone
/// responses flush through the assigned socket; everything else takes the
/// existing hyper-oneshot path. Queued write callbacks run first, in
/// order, then the end callback (#4904).
#[no_mangle]
pub unsafe extern "C" fn js_node_http_res_end_with_cb(handle: i64, chunk: f64, callback: i64) {
    let is_standalone = get_handle::<ServerResponse>(handle)
        .map(|sr| sr.standalone)
        .unwrap_or(false);
    if is_standalone {
        standalone_end(handle, chunk, callback);
        return;
    }
    // #4909 — Node's flush ordering, matching `js_node_http_res_end_full`:
    // queued write callbacks → `'finish'` → end callback → `'close'`. The
    // previous code fired `'finish'`/`'close'` (via `js_node_http_res_end`)
    // before any callback ran.
    let (finish_listeners, close_listeners) =
        finalize_buffered_end(handle, chunk).unwrap_or_default();
    let write_cbs = get_handle_mut::<ServerResponse>(handle)
        .map(|sr| std::mem::take(&mut sr.pending_write_callbacks))
        .unwrap_or_default();
    // #8163: every snapshot crosses the JS calls below — root them all first.
    let scope = perry_ffi::TransientRootScope::enter();
    let tail = EndTail::root(
        &scope,
        &write_cbs,
        callback,
        &finish_listeners,
        &close_listeners,
    );
    tail.run_write_callbacks();
    tail.emit_finish();
    tail.run_end_callback();
    tail.emit_close();
}

/// Flush a standalone response: serialize the head + buffered body and
/// write them through the assigned socket's JS `write` method — one write
/// for head+body, then the zero-length finish chunk Node's corked flush
/// emits. The body is suppressed for HEAD requests.
unsafe fn standalone_end(handle: i64, chunk: f64, callback: i64) {
    let v = JsValue::from_bits(chunk.to_bits());
    let final_chunk = if v.is_undefined() || v.is_null() {
        None
    } else {
        jsvalue_to_body_bytes(chunk)
    };

    let (socket, payload, write_cbs, finish_listeners, close_listeners);
    {
        let sr = match get_handle_mut::<ServerResponse>(handle) {
            Some(s) => s,
            None => return,
        };
        if sr.writable_ended {
            return;
        }
        if let Some(c) = final_chunk {
            sr.buffered_body.extend_from_slice(&c);
        }
        sr.headers_sent = true;
        sr.writable_ended = true;
        sr.ensure_content_length();
        let body = std::mem::take(&mut sr.buffered_body);
        // Fast path: with no custom `statusMessage`, a common status code has a
        // precomputed `HTTP/1.1 <code> <canonical reason>\r\n` status line,
        // skipping the per-response `format!`. The interned bytes equal exactly
        // what the `format!` produced for `(code, canonical reason)`. A custom
        // message, or an uncommon code, falls back so its reason still reaches
        // the wire byte-for-byte.
        let mut head = match sr.status_message.as_deref() {
            None => {
                crate::server::response_fast::status_line_bytes(sr.status_code).map(str::to_string)
            }
            Some(_) => None,
        }
        .unwrap_or_else(|| {
            let reason = sr.status_message.clone().unwrap_or_else(|| {
                StatusCode::from_u16(sr.status_code)
                    .ok()
                    .and_then(|s| s.canonical_reason())
                    .unwrap_or("")
                    .to_string()
            });
            format!("HTTP/1.1 {} {}\r\n", sr.status_code, reason)
        });
        for (k, v) in sr.snapshot_headers() {
            head.push_str(&k);
            head.push_str(": ");
            head.push_str(&v);
            head.push_str("\r\n");
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        if sr.standalone_req_method.as_deref() != Some("HEAD") {
            bytes.extend_from_slice(&body);
        }
        payload = bytes;
        socket = sr.standalone_socket;
        write_cbs = std::mem::take(&mut sr.pending_write_callbacks);
        finish_listeners = take_event_listeners(sr, "finish");
        close_listeners = take_event_listeners(sr, "close");
        sr.writable_finished = true;
    }
    // #8163: `socket_write_str` calls the socket's JS `write`, so every
    // snapshot taken above is already crossing JS from here on — root first.
    let scope = perry_ffi::TransientRootScope::enter();
    let tail = EndTail::root(
        &scope,
        &write_cbs,
        callback,
        &finish_listeners,
        &close_listeners,
    );
    if !JsValue::from_bits(socket.to_bits()).is_undefined() {
        socket_write_str(socket, &String::from_utf8_lossy(&payload));
        socket_write_str(socket, "");
    }
    tail.run_write_callbacks();
    tail.run_end_callback();
    tail.emit_finish();
    tail.emit_close();
}

/// The `res.end()` tail — queued write callbacks, the end callback, and the
/// `'finish'` / `'close'` listeners — with every snapshot ROOTED for the whole
/// sequence (#8163).
///
/// The listener vectors are taken OUT of the `ServerResponse` handle
/// (`take_event_listeners`), the write callbacks are `mem::take`n, and the end
/// callback is a raw argument: from that moment the registered mutable-root
/// scanner (`scan_http_server_roots`) no longer sees any of them. Each of the
/// four steps runs JS and can therefore trigger a moving collection, so a plain
/// `Vec<i64>` snapshot used AFTER an earlier step is a pre-move address — the
/// production Next App Route fixture faulted on exactly this: `'finish'`
/// listeners ran, a copying minor moved the `'close'` listener closure, and
/// `emit_no_arg_to_listeners(&close_listeners)` dereferenced the retired
/// from-space copy. Rooting inside `emit_no_arg_to_listeners` alone is not
/// enough — it roots what it is *handed*, and it was handed a stale snapshot.
///
/// Every address lives in the runtime's transient-root stack (marked AND
/// rewritten on evacuation) and is re-read at each use. The caller decides the
/// order — the buffered path fires the end callback *after* `'finish'` (Node
/// registers `end(cb)` as a `'finish'` listener behind the existing ones,
/// #4909), the standalone path fires it before.
struct EndTail<'a> {
    _scope: &'a perry_ffi::TransientRootScope,
    write_cbs: Vec<perry_ffi::TransientRootedAddr>,
    callback: perry_ffi::TransientRootedAddr,
    finish: Vec<perry_ffi::TransientRootedAddr>,
    close: Vec<perry_ffi::TransientRootedAddr>,
}

impl<'a> EndTail<'a> {
    /// Root every snapshot. Must be called before ANY of them crosses a JS
    /// call — i.e. immediately after they are taken out of the handle.
    fn root(
        scope: &'a perry_ffi::TransientRootScope,
        write_cbs: &[i64],
        callback: i64,
        finish: &[i64],
        close: &[i64],
    ) -> Self {
        Self {
            _scope: scope,
            write_cbs: scope.root_addrs(write_cbs),
            callback: scope.root_addr(callback),
            finish: scope.root_addrs(finish),
            close: scope.root_addrs(close),
        }
    }

    fn run_write_callbacks(&self) {
        for cb in &self.write_cbs {
            call_closure0(cb.get());
        }
    }

    fn run_end_callback(&self) {
        call_closure0(self.callback.get());
    }

    fn emit_finish(&self) {
        emit_no_arg_to_listeners(&Self::current(&self.finish));
    }

    fn emit_close(&self) {
        emit_no_arg_to_listeners(&Self::current(&self.close));
    }

    /// The post-collection addresses, read at the moment of use.
    fn current(rooted: &[perry_ffi::TransientRootedAddr]) -> Vec<i64> {
        rooted.iter().map(|cb| cb.get()).collect()
    }
}

/// Call a closure pointer with no args, ignoring the result.
pub(crate) fn call_closure0(callback: i64) {
    if callback == 0 {
        return;
    }
    unsafe {
        let closure = JsClosure::from_raw(callback as *const RawClosureHeader);
        if !closure.is_null() {
            let _ = closure.call0();
        }
    }
}
