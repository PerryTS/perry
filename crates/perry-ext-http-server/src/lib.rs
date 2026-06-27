//! Native bindings for Node.js's HTTP server modules — `node:http`,
//! `node:https`, `node:http2` (issue #577).
//!
//! Closes #487 as a side effect by exposing a faithful subset of
//! Node's stdlib so that `@hono/node-server`, Express, Koa, Polka,
//! Fastify-on-node-http, h3, etc. can all run unmodified against
//! perry-compiled programs.
//!
//! # Architecture
//!
//! - `http.createServer(handler)` registers a `HttpServer` handle
//!   carrying the user's handler closure (raw `i64`).
//! - `server.listen({ port, host? }, cb?)` binds, spawns a hyper
//!   accept loop on the perry-ffi blocking pool, and enters the
//!   main-thread event loop.
//! - Each incoming request creates an `IncomingMessage` + `ServerResponse`
//!   handle pair, ships them to the main thread via mpsc, the user's
//!   handler runs synchronously (any returned Promise is awaited),
//!   then the response is flushed back through hyper.
//! - Per-request event listeners (`req.on('data', cb)` / `res.on('finish', cb)`)
//!   are stored as raw `i64` pointers on the IncomingMessage /
//!   ServerResponse handles. A GC root scanner pins them across
//!   malloc-triggered sweeps (issue #35 pattern, copied from
//!   perry-ext-fastify).
//!
//! # Modules
//!
//! - `types` — shared NaN-boxing tags, runtime extern declarations,
//!   port/host extraction helpers, body-shape helpers.
//! - `server` — `HttpServer` handle + accept loop + handler dispatch.
//! - `request` — `IncomingMessage` handle + Readable-stream surface.
//! - `response` — `ServerResponse` handle + Writable-stream surface.
//! - `tls` — Phase 2: rustls config loader + ServerConfig builder.
//! - `https_server` — Phase 2: `https.createServer(opts, handler)`
//!   wired to a TLS-wrapped accept loop.
//! - `http2_server` — Phase 3: `http2.createSecureServer` on hyper's
//!   HTTP/2 builder with ALPN negotiation.
//! - `upgrade` — Phase 4: `Server.on('upgrade', ...)` dispatch +
//!   the `tokio-tungstenite` integration that lets `ws`'s
//!   `WebSocketServer({ server })` pattern work.
//!
//! # Punted gaps
//!
//! - **Cluster module** (`node:cluster`) — out of scope per #577.
//! - **HTTP/3 / QUIC** — out of scope.
//! - **Server push (HTTP/2)** — deprioritized; modern frameworks
//!   have moved away from it.
//! - **HTTP/2 WebSocket (RFC 8441)** — separate consideration; may
//!   defer.

use std::sync::Once;

use perry_ffi::{gc_register_mutable_root_scanner_named, iter_handles_of_mut, GcRootVisitor};

mod cluster_bind;
mod dispatch_ext;
// Unit-test binaries do not link the host stdlib/runtime archive that
// provides the perry_ffi async bridge; without these the test link is at the
// mercy of --gc-sections keeping/dropping the perry-ffi references pulled in
// via the perry-ext-net rlib (same shims as perry-ext-net / perry-ext-fetch).
mod handle_dispatch;
mod http2_server;
mod http2_session_settings;
mod http2_settings;
mod http2_stream_props;
mod https_server;
mod raw_upgrade;
mod request;
mod response;
mod response_fast;
mod server;
#[cfg(test)]
mod test_async_shims;
mod tls;
mod types;
mod upgrade;

pub use handle_dispatch::*;
pub use http2_server::*;
pub use http2_settings::*;
pub use https_server::*;
pub use request::*;
pub use response::*;
pub use server::*;

// ============================================================================
// GC root scanner
// ============================================================================

static GC_REGISTERED: Once = Once::new();

/// Register the http-server GC root scanner exactly once. User
/// closures (request handler, per-request event listeners on
/// IncomingMessage / ServerResponse, server-level event listeners)
/// are stored as raw `i64` pointers inside the various server
/// handles. Without this scanner, a malloc-triggered GC between
/// closure registration and callback dispatch would sweep them —
/// same root cause as issue #35 for net.Socket listeners.
pub(crate) fn ensure_gc_scanner_registered() {
    GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-http-server", scan_http_server_roots);
        // #2532 — register the server pump + has-active with perry-runtime
        // directly. In a workspace build perry-stdlib drains these via its
        // `external-http-server-pump` arm, but an out-of-tree install links
        // the prebuilt full stdlib with that arm compiled OUT — so without
        // this the accepted requests would never be dispatched and the
        // program would hang. Registration is idempotent on the runtime
        // side, so the in-tree double-drain is a harmless no-op.
        extern "C" {
            fn js_register_aux_pump(f: extern "C" fn() -> i32);
            fn js_register_aux_has_active(f: extern "C" fn() -> i32);
        }
        unsafe {
            js_register_aux_pump(crate::server::js_node_http_server_process_pending);
            js_register_aux_has_active(crate::server::js_node_http_server_has_active);
        }
        // Wall 10 — register the handle property/method/property-set dispatch
        // extensions so erased-receiver `req.url` / `res.end(...)` etc. route to
        // our handles even when the linked perry-stdlib was built WITHOUT
        // `external-http-server-pump` (the prebuilt `full` stdlib used by
        // out-of-tree installs and `PERRY_NO_AUTO_OPTIMIZE=1`). See
        // `dispatch_ext.rs`.
        crate::dispatch_ext::ensure_dispatch_extensions_registered();
    });
}

/// GC root scanner — walk every registered server / request /
/// response handle and mark every closure pointer they've stashed.
fn scan_http_server_roots(visitor: &mut GcRootVisitor<'_>) {
    fn scan_listener_roots(
        listeners: &mut std::collections::HashMap<String, Vec<i64>>,
        visitor: &mut GcRootVisitor<'_>,
    ) {
        for callbacks in listeners.values_mut() {
            for cb in callbacks.iter_mut() {
                visitor.visit_i64_slot(cb);
            }
        }
    }

    fn scan_base_server_roots(server: &mut HttpServer, visitor: &mut GcRootVisitor<'_>) {
        visitor.visit_i64_slot(&mut server.handler);
        scan_listener_roots(&mut server.listeners, visitor);
        // #4903 — listen callbacks queued for the deferred `'listening'`
        // emit; a GC between `listen()` and the pump tick must not sweep
        // them.
        for cb in server.deferred_listen_cbs.iter_mut() {
            visitor.visit_i64_slot(cb);
        }
    }

    iter_handles_of_mut::<HttpServer, _>(|s| {
        scan_base_server_roots(s, visitor);
    });
    iter_handles_of_mut::<HttpsServer, _>(|s| {
        visitor.visit_i64_slot(&mut s.handler);
        scan_base_server_roots(&mut s.base, visitor);
    });
    iter_handles_of_mut::<Http2SecureServer, _>(|s| {
        visitor.visit_i64_slot(&mut s.handler);
        scan_base_server_roots(&mut s.base, visitor);
    });
    iter_handles_of_mut::<IncomingMessage, _>(|im| {
        scan_listener_roots(&mut im.listeners, visitor);
        visitor.visit_nanbox_f64_slot(&mut im.signal_controller);
        visitor.visit_nanbox_f64_slot(&mut im.signal);
        visitor.visit_nanbox_f64_slot(&mut im.socket_value);
    });
    iter_handles_of_mut::<ServerResponse, _>(|sr| {
        scan_listener_roots(&mut sr.listeners, visitor);
        visitor.visit_nanbox_f64_slot(&mut sr.standalone_socket);
        for cb in sr.pending_write_callbacks.iter_mut() {
            visitor.visit_i64_slot(cb);
        }
    });
    iter_handles_of_mut::<Http2SessionHandle, _>(|session| {
        scan_listener_roots(&mut session.listeners, visitor);
        for cb in session.close_callbacks.iter_mut() {
            visitor.visit_i64_slot(cb);
        }
        for cb in session.pending_callbacks.iter_mut() {
            visitor.visit_i64_slot(cb);
        }
        visitor.visit_i64_slot(&mut session.timeout_callback);
    });
    iter_handles_of_mut::<Http2StreamHandle, _>(|stream| {
        scan_listener_roots(&mut stream.listeners, visitor);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{drop_handle, get_handle, register_handle};
    use std::collections::HashMap;
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            Self::new_with_slots(0)
        }

        fn new_with_slots(slot_count: u32) -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(slot_count);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn young_gc_value() -> f64 {
        f64::from_bits(
            crate::types::POINTER_TAG | (young_gc_root() as u64 & crate::types::PTR_MASK),
        )
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    fn assert_nanbox_rewritten(before: f64, after: f64) {
        assert_ne!(after.to_bits(), before.to_bits());
        let ptr = after.to_bits() & crate::types::PTR_MASK;
        assert!(perry_runtime::arena::pointer_in_nursery(ptr as usize));
    }

    fn listener_map(event: &str, cb: i64) -> HashMap<String, Vec<i64>> {
        HashMap::from([(event.to_string(), vec![cb])])
    }

    fn http_server(handler: i64, listeners: HashMap<String, Vec<i64>>) -> HttpServer {
        let mut s = HttpServer::with_handler(handler);
        s.listeners = listeners;
        s
    }

    /// Issue #2210 — `HttpServer::with_handler` seeds Node's
    /// documented timeout defaults so a fresh server reads back the
    /// same numbers Node returns when no options are passed.
    #[test]
    fn http_server_seeds_node_timeout_defaults() {
        let s = HttpServer::with_handler(0);
        assert_eq!(s.headers_timeout, 60_000.0);
        assert_eq!(s.keep_alive_timeout, 5_000.0);
        assert_eq!(s.keep_alive_timeout_buffer, 1_000.0);
        assert_eq!(s.request_timeout, 300_000.0);
        assert_eq!(s.idle_timeout, 0.0);
        assert_eq!(s.max_headers_count.to_bits(), crate::types::TAG_NULL);
        assert_eq!(s.max_requests_per_socket, 0.0);
        assert!(s.no_delay);
        assert!(!s.keep_alive);
        assert_eq!(s.keep_alive_initial_delay, 0.0);
    }

    /// Issue #2210 — the FFI getter/setter pair round-trips a value
    /// through the per-handle storage. Sanity-pins the macro-expanded
    /// `js_node_http_server_*` exports against future refactors.
    #[test]
    fn http_server_timeout_setter_round_trips() {
        let handle = register_handle(HttpServer::with_handler(0));
        // Sanity: defaults visible through the FFI getter.
        assert_eq!(
            crate::server::js_node_http_server_headers_timeout(handle),
            60_000.0
        );
        assert_eq!(
            crate::server::js_node_http_server_keep_alive_timeout_buffer(handle),
            1_000.0
        );
        // Set then read back.
        crate::server::js_node_http_server_set_headers_timeout(handle, 0.0);
        crate::server::js_node_http_server_set_keep_alive_timeout_buffer(handle, 250.0);
        crate::server::js_node_http_server_set_idle_timeout(handle, 45_000.0);
        crate::server::js_node_http_server_set_max_requests_per_socket(handle, 100.0);
        assert_eq!(
            crate::server::js_node_http_server_headers_timeout(handle),
            0.0
        );
        assert_eq!(
            crate::server::js_node_http_server_keep_alive_timeout_buffer(handle),
            250.0
        );
        assert_eq!(
            crate::server::js_node_http_server_idle_timeout(handle),
            45_000.0
        );
        assert_eq!(
            crate::server::js_node_http_server_max_requests_per_socket(handle),
            100.0,
        );
        // `setTimeout(ms, cb)` updates the idle timeout and registers
        // the cb as a `'timeout'` listener — returns the handle for chaining.
        let chained =
            crate::server::js_node_http_server_set_timeout_method(handle, 9_999.0, 0xCAFE);
        assert_eq!(chained, handle);
        assert_eq!(
            crate::server::js_node_http_server_idle_timeout(handle),
            9_999.0
        );
        let listener_count = get_handle::<HttpServer>(handle)
            .and_then(|s| s.listeners.get("timeout").map(|v| v.len()))
            .unwrap_or(0);
        assert_eq!(listener_count, 1);
        drop_handle(handle);
    }

    #[test]
    fn http_server_options_store_keep_alive_timeout_buffer() {
        let _guard = GcTestGuard::new_with_slots(1);
        let options_json = perry_ffi::alloc_string(
            r#"{"headersTimeout":111,"keepAliveTimeout":222,"keepAliveTimeoutBuffer":321,"requestTimeout":444}"#,
        );
        let options_ptr = options_json.as_raw() as *const perry_runtime::StringHeader;
        let options = unsafe { perry_runtime::json::js_json_parse(options_ptr) };
        perry_runtime::gc::js_shadow_slot_set(0, options.bits());

        let mut server = HttpServer::with_handler(0);
        crate::server::apply_server_options(&mut server, f64::from_bits(options.bits()));

        assert_eq!(server.headers_timeout, 111.0);
        assert_eq!(server.keep_alive_timeout, 222.0);
        assert_eq!(server.keep_alive_timeout_buffer, 321.0);
        assert_eq!(server.request_timeout, 444.0);
    }

    #[test]
    fn gc_mutable_scanner_rewrites_server_wrapper_and_request_response_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named(
            "perry-ext-http-server",
            scan_http_server_roots,
        );

        let http_handler = young_gc_root();
        let http_listener = young_gc_root();
        let http_handle = register_handle(http_server(
            http_handler,
            listener_map("request", http_listener),
        ));

        let https_handler = young_gc_root();
        let https_base_handler = young_gc_root();
        let https_listener = young_gc_root();
        let https_handle = register_handle(HttpsServer {
            handler: https_handler,
            tls_config: None,
            base: http_server(
                https_base_handler,
                listener_map("listening", https_listener),
            ),
        });

        let h2_handler = young_gc_root();
        let h2_base_handler = young_gc_root();
        let h2_listener = young_gc_root();
        let h2_handle = register_handle(Http2SecureServer {
            handler: h2_handler,
            tls_config: None,
            plaintext: false,
            base: http_server(h2_base_handler, listener_map("close", h2_listener)),
        });

        let incoming_listener = young_gc_root();
        let mut incoming = IncomingMessage::new(
            "GET".to_string(),
            "/".to_string(),
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            "127.0.0.1".to_string(),
            1234,
        );
        incoming.listeners = listener_map("data", incoming_listener);
        let incoming_signal_controller = young_gc_value();
        let incoming_signal = young_gc_value();
        incoming.signal_controller = incoming_signal_controller;
        incoming.signal = incoming_signal;
        let incoming_handle = register_handle(incoming);

        let response_listener = young_gc_root();
        let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
        let mut response = ServerResponse::new(response_tx);
        response.listeners = listener_map("finish", response_listener);
        let response_handle = register_handle(response);

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let http = get_handle::<HttpServer>(http_handle).expect("http server");
            assert_rewritten(http_handler, http.handler);
            assert_rewritten(http_listener, http.listeners["request"][0]);

            let https = get_handle::<HttpsServer>(https_handle).expect("https server");
            assert_rewritten(https_handler, https.handler);
            assert_rewritten(https_base_handler, https.base.handler);
            assert_rewritten(https_listener, https.base.listeners["listening"][0]);

            let h2 = get_handle::<Http2SecureServer>(h2_handle).expect("http2 server");
            assert_rewritten(h2_handler, h2.handler);
            assert_rewritten(h2_base_handler, h2.base.handler);
            assert_rewritten(h2_listener, h2.base.listeners["close"][0]);

            let incoming = get_handle::<IncomingMessage>(incoming_handle).expect("incoming");
            assert_rewritten(incoming_listener, incoming.listeners["data"][0]);
            assert_nanbox_rewritten(incoming_signal_controller, incoming.signal_controller);
            assert_nanbox_rewritten(incoming_signal, incoming.signal);

            let response = get_handle::<ServerResponse>(response_handle).expect("response");
            assert_rewritten(response_listener, response.listeners["finish"][0]);
        }

        drop_handle(http_handle);
        drop_handle(https_handle);
        drop_handle(h2_handle);
        drop_handle(incoming_handle);
        drop_handle(response_handle);
    }

    /// CodeRabbit (flagged on #5663) — `requestTimeout` was stored as a
    /// raw `f64` and later cast straight to `u64` to build the in-flight
    /// deadline.
    /// A non-finite or oversized value produced a garbage deadline:
    /// `Infinity as u64` saturates to `u64::MAX` (never times out),
    /// oversized finite values likewise. Sanitizing at the setter keeps
    /// the stored value in Node's `validateInteger(0, MAX_SAFE_INTEGER)`
    /// domain so the downstream cast is always sound — matching the
    /// behavior probed against Node v22 (`createServer({ requestTimeout })`
    /// rejects non-finite/negative/oversized with `ERR_OUT_OF_RANGE`;
    /// lacking a throw path here we coerce to the nearest in-range value).
    #[test]
    fn request_timeout_is_sanitized_to_a_u64_safe_ms_count() {
        use crate::server::sanitize_request_timeout;

        // Non-finite falls back to Node's 300s default rather than
        // saturating the cast.
        assert_eq!(sanitize_request_timeout(f64::INFINITY), 300_000.0);
        assert_eq!(sanitize_request_timeout(f64::NEG_INFINITY), 300_000.0);
        assert_eq!(sanitize_request_timeout(f64::NAN), 300_000.0);
        // Negative clamps to 0 (Node's "disabled" sentinel).
        assert_eq!(sanitize_request_timeout(-1.0), 0.0);
        // Oversized clamps to MAX_SAFE_INTEGER (Node's `kMaxRequestTimeout`).
        assert_eq!(sanitize_request_timeout(1e300), 9_007_199_254_740_991.0);
        assert_eq!(
            sanitize_request_timeout(9_007_199_254_740_992.0),
            9_007_199_254_740_991.0
        );
        // Valid inputs pass through untouched (fractional truncated to ms).
        assert_eq!(sanitize_request_timeout(0.0), 0.0);
        assert_eq!(sanitize_request_timeout(5_000.0), 5_000.0);
        assert_eq!(sanitize_request_timeout(300_000.0), 300_000.0);
        assert_eq!(sanitize_request_timeout(1.9), 1.0);

        // The whole point: every sanitized value is a finite, non-negative
        // ms count whose `as u64` cast yields a real `Duration` — never the
        // `u64::MAX` overflow the raw `Infinity` would have produced.
        for raw in [f64::INFINITY, f64::NAN, -1.0, 1e300, 5_000.0] {
            let ms = sanitize_request_timeout(raw);
            assert!(ms.is_finite() && (0.0..=9_007_199_254_740_991.0).contains(&ms));
            assert!(
                ms as u64 != u64::MAX,
                "raw {raw} must not saturate the cast"
            );
        }
    }

    /// Both setter paths — the `server.requestTimeout = x` property
    /// (FFI) and the `createServer({ requestTimeout })` option — store a
    /// sanitized value. Pre-fix, both stored the raw `f64` verbatim.
    #[test]
    fn request_timeout_setter_paths_store_sanitized_values() {
        // Property-setter path: `Infinity` previously stored verbatim.
        let handle = register_handle(HttpServer::with_handler(0));
        let ret = crate::server::js_node_http_server_set_request_timeout(handle, f64::INFINITY);
        // The setter returns the assigned value (JS `a = b` evaluates to `b`)…
        assert!(ret.is_infinite());
        // …but the *stored* field is sanitized to the safe default.
        assert_eq!(
            crate::server::js_node_http_server_request_timeout(handle),
            300_000.0
        );
        crate::server::js_node_http_server_set_request_timeout(handle, 7_500.0);
        assert_eq!(
            crate::server::js_node_http_server_request_timeout(handle),
            7_500.0
        );
        drop_handle(handle);

        // Option path through `apply_server_options`.
        let _guard = GcTestGuard::new_with_slots(1);
        let options_json =
            perry_ffi::alloc_string(r#"{"requestTimeout":1e300,"headersTimeout":222}"#);
        let options_ptr = options_json.as_raw() as *const perry_runtime::StringHeader;
        let options = unsafe { perry_runtime::json::js_json_parse(options_ptr) };
        perry_runtime::gc::js_shadow_slot_set(0, options.bits());

        let mut server = HttpServer::with_handler(0);
        crate::server::apply_server_options(&mut server, f64::from_bits(options.bits()));
        // Oversized `requestTimeout` clamped; unrelated knob untouched.
        assert_eq!(server.request_timeout, 9_007_199_254_740_991.0);
        assert_eq!(server.headers_timeout, 222.0);
    }
}
