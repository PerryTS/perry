//! Chainable no-op option setters for `net.Socket` / `net.Server` (#1852) and
//! the C-ABI bridge to perry-runtime's Node argument validators (#2013).
//!
//! Split out of `lib.rs` to keep that file under the 2000-line gate. The
//! `#[no_mangle]` functions here are codegen dispatch targets (reached by
//! symbol, not by Rust path), so they re-export transparently at the crate
//! root. The `extern "C"` validator declarations are `pub(crate)` so the
//! `server.listen` / `socket.connect` sites in `lib.rs` can call them.

extern "C" {
    // #2013: Node argument validators (perry-runtime/src/net_validate.rs).
    // perry-ext-net has no Cargo dependency on perry-runtime, so these are
    // reached over the C ABI — the same pattern as the runtime entry points
    // declared in `lib.rs`. Each diverges via `js_throw` on bad input and
    // returns normally for a valid value.
    pub(crate) fn js_net_validate_listen_port(value: f64);
    pub(crate) fn js_net_validate_connect_port(value: f64);
    fn js_net_validate_socket_timeout(value: f64);
    fn js_net_validate_tos(value: f64) -> i32;
    // perry-runtime/src/value/truthy.rs — general JS ToBoolean over the C ABI.
    // Used by `setNoDelay` to coerce its argument the way Node does.
    fn js_is_truthy(value: f64) -> i32;
}

// ─── Chainable no-op socket/server options (issue #1852) ─────────────────────
//
// Node's `net.Socket` and `net.Server` expose a family of configuration
// methods that return the instance (`this`) for chaining — `setNoDelay`,
// `setKeepAlive`, `setTimeout`, `setEncoding`, `pause`, `resume`, `ref`,
// `unref`, `cork`, `uncork`, etc. Perry's TCP transport doesn't model TCP
// socket options (Nagle, keep-alive, idle-timeout) or read-pause yet, but
// the methods still need to *exist and be callable*: pre-fix, calling any
// of them threw "x is not a function" (the radar's "value() missing"
// cluster) and aborted the program before the real I/O ever ran.
//
// These shims accept the receiver handle (the dispatch table declares
// `args: &[]`, so user-supplied option args are evaluated for the call but
// not forwarded) and return it unchanged so `sock.setNoDelay(true)` and
// chained forms like `sock.setKeepAlive().setNoDelay()` both type-check and
// keep flowing. The codegen NaN-boxes the returned id with POINTER_TAG
// (NR_PTR), reproducing the original Socket/Server value shape so a
// subsequent method on the result still dispatches.

/// Chainable no-op for `net.Socket` option setters — returns the socket
/// handle unchanged.
#[no_mangle]
pub extern "C" fn js_net_socket_noop_self(handle: i64) -> i64 {
    handle
}

/// `socket.setTimeout(msecs, callback?)` — validates `msecs` the way Node does
/// (number → `ERR_INVALID_ARG_TYPE`, non-negative finite → `ERR_OUT_OF_RANGE`,
/// #2013) and then behaves as the chainable no-op (the underlying idle-timeout
/// machinery is not modelled), returning the socket handle for chaining. The
/// optional callback is accepted but, like the other option setters, ignored.
///
/// # Safety
///
/// `_callback_i64` is a NaN-boxed JSValue passed as raw bits; it is not
/// dereferenced here.
#[no_mangle]
pub extern "C" fn js_net_socket_set_timeout(handle: i64, msecs: f64, _callback_i64: i64) -> i64 {
    unsafe { js_net_validate_socket_timeout(msecs) };
    // #2549 — record the timeout so `socket.timeout` reflects it. Node clears
    // (reports `undefined`) when the timeout is set to 0.
    if let Some(s) = crate::statics::sockets().lock().unwrap().get_mut(&handle) {
        s.timeout = if msecs > 0.0 {
            Some(msecs as u64)
        } else {
            None
        };
    }
    handle
}

/// Chainable no-op for `net.Server` option setters — returns the server
/// handle unchanged.
#[no_mangle]
pub extern "C" fn js_net_server_noop_self(handle: i64) -> i64 {
    handle
}

#[no_mangle]
pub extern "C" fn js_net_socket_get_type_of_service(handle: i64) -> f64 {
    crate::statics::sockets()
        .lock()
        .ok()
        .and_then(|sockets| sockets.get(&handle).map(|s| s.type_of_service as f64))
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_net_socket_set_type_of_service(handle: i64, tos: f64) -> i64 {
    let tos = unsafe { js_net_validate_tos(tos) } as u8;
    if let Some(s) = crate::statics::sockets().lock().unwrap().get_mut(&handle) {
        s.type_of_service = tos;
    }
    handle
}

/// `socket.setNoDelay([enable])` — toggle Nagle's algorithm (`TCP_NODELAY`)
/// on the live socket. Node coerces the arg as `enable === undefined || !!enable`,
/// so a bare `setNoDelay()` enables nodelay (the connection default), and an
/// explicit `setNoDelay(false)` re-enables Nagle. The owning `TcpStream` lives
/// in `run_socket_task`, so the request is forwarded as a `SetNoDelay` command
/// over the socket's channel; returns the handle for chaining.
///
/// # Safety
///
/// `arg_bits` is a NaN-boxed JSValue passed as raw bits (the codegen NA_F64
/// slot, re-bitcast here); it is not dereferenced.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_no_delay(handle: i64, arg_bits: i64) -> i64 {
    let arg = f64::from_bits(arg_bits as u64);
    let enable = perry_ffi::JsValue::from_bits(arg.to_bits()).is_undefined()
        || unsafe { js_is_truthy(arg) } != 0;
    if let Some(s) = crate::statics::sockets().lock().unwrap().get(&handle) {
        let _ = s.cmd_tx.send(crate::SocketCommand::SetNoDelay(enable));
    }
    handle
}

/// #4973: `socket.setEncoding(enc?)` — record the read encoding so the
/// main-thread pump delivers `'data'` events as decoded strings (Node
/// readable-stream semantics). Calling with no/nullish arg clears it
/// (back to Buffers). Recognized text encodings decode as UTF-8 (lossy);
/// 'hex'/'base64' render their respective text forms in the pump.
///
/// # Safety
/// `enc_ptr` is either 0 or a StringHeader pointer (codegen NA_STR slot).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_set_encoding(handle: i64, enc_ptr: i64) -> i64 {
    match crate::string_from_header_i64(enc_ptr) {
        Some(enc) if !enc.is_empty() => {
            crate::statics::encodings()
                .lock()
                .unwrap()
                .insert(handle, enc.to_ascii_lowercase());
        }
        _ => {
            crate::statics::encodings().lock().unwrap().remove(&handle);
        }
    }
    handle
}
