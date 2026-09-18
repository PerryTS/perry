//! The turnloop half of `ServerResponse` (P5), plus the `Connection` /
//! `Keep-Alive` decision both transports share.
//!
//! Split out of `response.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap; declared as a `#[path]` child module of
//! `response` so `use super::*` resolves the way it did inline.

use super::*;

impl HyperResponseShape {
    /// Inject Node-compatible default `Connection` / `Keep-Alive` headers
    /// (#2132). Node's HTTP/1.x server appends `Connection: keep-alive` plus
    /// `Keep-Alive: timeout=<keepAliveTimeout/1000>` whenever the connection
    /// is kept alive, and `Connection: close` otherwise. Hyper drives the
    /// transport-level keep-alive itself but does not surface these headers in
    /// the response bytes, so byte-for-byte parity tests — and any client
    /// reading `res.headers.connection` / `res.headers['keep-alive']` — see
    /// them missing. Add them before handing the shape to hyper, unless the
    /// handler already set a `Connection` header explicitly. HTTP/2 manages
    /// connection reuse at the protocol level, so it gets neither header.
    pub fn apply_default_connection_headers(
        &mut self,
        version: http::Version,
        req_connection: Option<&str>,
        keep_alive_timeout_ms: f64,
    ) {
        let wire = match version {
            http::Version::HTTP_10 => 0u8,
            http::Version::HTTP_2 | http::Version::HTTP_3 => 2,
            _ => 1,
        };
        self.apply_default_connection_headers_for(wire, req_connection, keep_alive_timeout_ms);
    }

    /// The transport-independent form: `wire_version` is 0 for HTTP/1.0, 1 for
    /// HTTP/1.1 and 2 for HTTP/2+, matching `turnloop_http::http1::Head::version`
    /// (which is 0 or 1) with 2 reserved for the HTTP/2 path.
    ///
    /// # `keepAliveTimeout = 0`
    ///
    /// Node 26.5.1, measured: `keepAliveTimeout = 0` keeps the connection
    /// **alive** — the response carries `Connection: keep-alive` with **no**
    /// `Keep-Alive` header, and the server never closes the idle connection
    /// (still open after 2 s; a 300 ms timeout FINs at 1303 ms and a 1000 ms
    /// timeout at 2002 ms, i.e. `keepAliveTimeout + keepAliveTimeoutBuffer`,
    /// whose default is 1000 ms). Zero means "no timeout", not "no keep-alive".
    ///
    /// Perry used to fold the two decisions together — `should_keep_alive &&
    /// keep_alive_timeout_ms > 0.0` — so a server that disabled the timeout
    /// got `Connection: close` on every response and no connection reuse at
    /// all. They are now separate: the *reuse* decision comes from the
    /// protocol version and the request's `Connection` tokens, and the
    /// timeout only decides whether a `Keep-Alive: timeout=N` header advertises
    /// one.
    pub fn apply_default_connection_headers_for(
        &mut self,
        wire_version: u8,
        req_connection: Option<&str>,
        keep_alive_timeout_ms: f64,
    ) {
        if self
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("connection"))
        {
            return;
        }
        if wire_version >= 2 {
            return;
        }

        let conn_lower = req_connection.map(str::to_ascii_lowercase);
        let has_token = |tok: &str| {
            conn_lower
                .as_deref()
                .map(|c| c.split(',').any(|t| t.trim() == tok))
                .unwrap_or(false)
        };

        // HTTP/1.0 defaults to close (keep-alive only when explicitly
        // requested); HTTP/1.1 defaults to keep-alive unless asked to close.
        let should_keep_alive = if wire_version == 0 {
            has_token("keep-alive")
        } else {
            !has_token("close")
        };

        if !should_keep_alive {
            self.headers
                .push(("Connection".to_string(), "close".to_string()));
            return;
        }
        self.headers
            .push(("Connection".to_string(), "keep-alive".to_string()));
        if keep_alive_timeout_ms > 0.0 {
            let secs = (keep_alive_timeout_ms / 1000.0).floor().max(0.0) as u64;
            // Fast path: the `Keep-Alive: timeout=N` value is interned for the
            // timeouts servers commonly run with (Node's 5 s default, etc.), so
            // the per-response `format!` only fires for an unusual timeout. The
            // interned string equals `format!("timeout={}", secs)` exactly.
            let value = crate::server::response_fast::keep_alive_header_value(secs)
                .map(str::to_string)
                .unwrap_or_else(|| format!("timeout={}", secs));
            self.headers.push(("Keep-Alive".to_string(), value));
        }
    }
}

/// The `IncomingMessage` handle a response answers, read back after the
/// response's own borrow has ended.
pub(crate) fn req_handle_of(handle: i64) -> i64 {
    get_handle::<ServerResponse>(handle)
        .map(|sr| sr.req_handle)
        .unwrap_or(0)
}

/// Allocate the `ServerResponse` for a request the turnloop HTTP server
/// decoded (P5).
///
/// No oneshot, no `Notify` and no `AtomicBool`: the handler runs on the thread
/// that owns the connection, so `res.end()` encodes and submits the write
/// itself rather than parking a shape for another task to pick up.
pub(crate) fn alloc_server_response_for_turnloop(conn_id: i64, seq: u64, req_handle: i64) -> i64 {
    let (tx, _rx) = oneshot::channel::<HyperResponseShape>();
    let mut response = ServerResponse::new(tx).with_request_handle(req_handle);
    // No hyper task is waiting on the oneshot, and its receiver was dropped
    // above — leaving it in place would make the in-flight reaper's
    // `tx.is_closed()` peer-gone probe true for every turnloop response and
    // abandon every async handler on its first tick. `stream_receiver_gone`
    // answers that probe for this path instead, from the connection handle.
    response.response_tx = None;
    response.turnloop = Some((conn_id, seq));
    register_handle(response)
}

/// True when a streaming response's connection died under it (hyper
/// dropped the body receiver — client disconnect / server close).
pub(crate) fn stream_receiver_gone(handle: i64) -> bool {
    let Some(sr) = get_handle::<ServerResponse>(handle) else {
        return false;
    };
    if let Some((conn, _)) = sr.turnloop {
        // The turnloop equivalent of hyper's dropped body receiver: the
        // connection handle is gone.
        return !perry_ffi::turnloop_net::is_live(conn);
    }
    sr.stream_tx
        .as_ref()
        .map(|tx| tx.is_closed())
        .unwrap_or(false)
}

/// `res.writeContinue()` — acknowledge an `Expect: 100-continue` request.
///
/// #5080: the interim `HTTP/1.1 100 Continue` is written by hyper the moment
/// the request body is polled (`req.collect()` in the service fn), which is
/// what unblocks the client's withheld body before `'checkContinue'` even
/// fires on the main thread. So by the time the handler calls
/// `writeContinue()` the 100 is already on the wire; this entry point exists
/// for API parity (the canonical `checkContinue` handler calls it) and is a
/// confirmation no-op rather than a second 100 line.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_continue(handle: i64) {
    // On the hyper path the interim 100 was already flushed when the body was
    // first polled, so this was a no-op. On turnloop nothing is automatic once
    // a `'checkContinue'` listener has taken the request over — that listener
    // IS the decision — so the call reaches the wire here.
    if let Some((conn, seq)) = get_handle::<ServerResponse>(handle).and_then(|sr| sr.turnloop) {
        crate::server::turnloop_serve::send_interim(conn, seq, b"HTTP/1.1 100 Continue\r\n\r\n");
    }
}

/// `res.writeProcessing()` — emits an HTTP/1.1 102-Processing. Stub.
#[no_mangle]
pub extern "C" fn js_node_http_res_write_processing(handle: i64) {
    if let Some((conn, seq)) = get_handle::<ServerResponse>(handle).and_then(|sr| sr.turnloop) {
        crate::server::turnloop_serve::send_interim(conn, seq, b"HTTP/1.1 102 Processing\r\n\r\n");
    }
}
