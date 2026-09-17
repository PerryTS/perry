//! The fastify listen path.
//!
//! Split out of `server.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap; declared as a `#[path]` child module of
//! `server` so `use super::*` resolves the way it did inline.
//!
//! There is one transport: [`perry_http_server`], the shared HTTP/1.1 server
//! core on turnloop. The hyper accept loop that used to live below is gone,
//! and so is the listen-time decision that chose it.
//!
//! # What the decline was, and why it ended
//!
//! An app with `app.server.on("upgrade", …)` handlers kept the whole hyper
//! loop, because the handshake ended in
//! `perry_ext_ws::register_external_ws_stream` and that needed an owned
//! `AsyncRead + AsyncWrite` stream a turnloop connection cannot produce. The
//! blocker was real and it was `perry-ext-ws`'s: its protocol is
//! `turnloop_websocket`'s sans-I/O codec, but its *transport* still took a
//! stream. With that gone, the handshake is
//! [`perry_ext_ws::accept_http_upgrade`] over bytes, `perry-http-server` grew
//! the upgrade hook its own header said would go in "when that is solved, with
//! a caller", and fastify is the second caller. Nothing here needs hyper, an
//! async runtime, or a descriptor handoff.

use super::*;

/// This crate's slot in the runtime's completion-sink registry.
/// `perry-ext-net` owns 0, `perry-ext-http` 1, perry-stdlib's turnloop HTTP
/// client 2 and its SMTP client 3.
pub(crate) const SUBSYSTEM: u8 = 4;

/// fastify's own `keepAliveTimeout` default — **72 s**, not Node's 5 s, and
/// measured against real fastify on the pinned oracle rather than read from
/// the docs: every response from `fastify@5` on Node 26.5.1 carries
/// `Keep-Alive: timeout=72`. Node then FINs an idle keep-alive connection at
/// `keepAliveTimeout + keepAliveTimeoutBuffer` (the buffer defaults to 1 s);
/// see `docs/turnloop/p5-report.md` for that measurement.
///
/// The hyper path advertised neither header and armed no idle close at all.
const KEEP_ALIVE_TIMEOUT_MS: f64 = 72_000.0;
const KEEP_ALIVE_TIMEOUT_BUFFER_MS: u64 = 1_000;

/// The [`perry_http_server::Host`] a turnloop-served fastify app installs.
///
/// Everything here runs inside the completion sink — on the loop thread, after
/// a turn, and **never** running JS. Route matching runs here because it did on
/// the hyper worker too (against a snapshot taken at `listen()` time); what a
/// match produces is a queued `FastifyPendingRequest`, and
/// `js_fastify_process_pending` dispatches it on its own tick.
struct FastifyHost {
    /// The route snapshot taken at `listen()` time — the same snapshot the
    /// hyper worker matched against, so a route registered after `listen()`
    /// does not match on either transport.
    routes: Arc<Vec<RouteMatcher>>,
    /// Where a matched request goes. The app handle is NOT held here: the pump
    /// reads it from the `FastifyServerHandle` that owns this channel's
    /// receiver, and holding a second copy would be a second thing to keep in
    /// step with `app.close()`.
    requests: mpsc::SyncSender<FastifyPendingRequest>,
    /// Shared with the `FastifyServerHandle`: `js_fastify_close` stores false,
    /// and the core then answers `Connection: close` and stops reusing
    /// connections, which is Node's `server.close()` contract.
    listening: Arc<AtomicBool>,
    /// The app, for the `'upgrade'` handlers the pump fires. Unlike
    /// `requests`, an upgrade has no `FastifyServerHandle` to read it back
    /// from at dispatch time — the pending carries it.
    app_handle: Handle,
    /// Whether this app registered any `app.server.on('upgrade', …)` handler
    /// before `listen()`. Read once, at listen time, so the answer is a
    /// property of the server rather than of whichever request arrives —
    /// which is also what Node does: it diverts an upgrade only when the
    /// server has an `'upgrade'` listener, and otherwise serves it as an
    /// ordinary request (#4973).
    takes_upgrades: bool,
    /// Where an accepted upgrade goes, for `js_fastify_process_pending` to
    /// fire on the main thread.
    upgrades: mpsc::SyncSender<FastifyPendingUpgrade>,
    /// How many upgrades are queued and undrained; the runtime keepalive reads
    /// it, because `std`'s `Receiver` has no `is_empty`.
    upgrade_depth: Arc<AtomicUsize>,
}

/// The `404` a route miss is answered with, the same envelope the hyper path
/// built. Answered in the sink, without a main-thread hop, exactly as the hyper
/// service fn answered it without one.
fn not_found() -> FastifyResponse {
    FastifyResponse {
        status: 404,
        headers: vec![("content-type".to_string(), "application/json".to_string())],
        body: br#"{"error":"Not Found"}"#.to_vec(),
    }
}

impl perry_http_server::Host for FastifyHost {
    fn on_request(&self, request: perry_http_server::Request) {
        let mut reply = Reply::Turnloop {
            conn_id: request.conn_id,
            seq: request.seq,
        };
        let path = request.target.clone();
        let Some((dispatch_method, params)) = match_route(&self.routes, &request.method, &path)
        else {
            reply.send(not_found());
            return;
        };
        let mut headers = HashMap::with_capacity(request.headers.len());
        for (name, value) in request.headers {
            // The decoder already lowercased; the hyper path lowercased by hand.
            headers.insert(name, value);
        }
        let body = if request.body.is_empty() {
            None
        } else {
            Some(request.body)
        };
        let pending = FastifyPendingRequest {
            method: dispatch_method,
            path,
            headers,
            body,
            params,
            reply,
        };
        // A full queue is refused, not buffered: `pending`'s Drop answers 503.
        // (`try_send` hands the value back on failure, so the Drop runs here.)
        let _ = self.requests.try_send(pending);
    }

    fn is_closing(&self) -> bool {
        !self.listening.load(Ordering::Acquire)
    }

    fn keep_alive_timeout_ms(&self) -> f64 {
        KEEP_ALIVE_TIMEOUT_MS
    }

    fn takes_upgrades(&self) -> bool {
        self.takes_upgrades
    }

    /// #1113 — `app.server.on('upgrade', (req, wsId, head) => …)`.
    ///
    /// The hyper version of this returned a hand-built `101` synchronously so
    /// hyper would switch protocols, then spawned a task to wait for the
    /// upgraded stream and hand it to `perry-ext-ws`. Both halves are gone:
    /// the handshake is validated and answered here over bytes, and the
    /// connection is adopted in place. What survives unchanged is the queue
    /// hop — this runs in the completion sink and must not run JS, so the
    /// handlers fire from `js_fastify_process_pending` exactly as before.
    fn on_upgrade(&self, request: perry_http_server::Request, leftover: Vec<u8>) {
        let conn_id = request.conn_id;
        let method = request.method.clone();
        let path = request.target.clone();
        let headers: HashMap<String, String> = request.headers.iter().cloned().collect();
        let app_handle = self.app_handle;
        let upgrades = self.upgrades.clone();
        let depth = self.upgrade_depth.clone();

        let accepted = perry_ext_ws::accept_http_upgrade(
            &request,
            &leftover,
            perry_ext_ws::HTTP_SERVER_TRANSPORT,
            &[],
            |ws_id| {
                let pending = FastifyPendingUpgrade {
                    app_handle,
                    method,
                    path,
                    headers,
                    ws_id: ws_id as i64,
                };
                if upgrades.try_send(pending).is_ok() {
                    depth.fetch_add(1, Ordering::AcqRel);
                }
            },
        );
        if accepted.is_err() {
            // `ws` answers a malformed upgrade with a 400 and closes. The
            // refusal bytes come from `perry-ext-ws` so there is one
            // implementation of what "not a WebSocket handshake" looks like.
            let response = accepted
                .err()
                .map(|refusal| refusal.response)
                .unwrap_or_default();
            perry_http_server::write_raw(conn_id, &response);
            perry_http_server::finish(conn_id);
        }
    }

    fn on_upgraded(&self, conn_id: i64, event: perry_http_server::Upgraded<'_>) {
        if perry_ext_ws::drive_http_upgraded(conn_id, event) {
            // A half-close the WebSocket layer is finished with: end our side
            // gracefully rather than cancelling the close frame it just
            // queued, which would make the peer report 1006.
            perry_http_server::finish(conn_id);
        }
    }
}

/// Match `method`/`path` against the snapshot, with Node fastify's HEAD-on-GET
/// shadowing: an unmatched `HEAD` falls back to a `GET` route of the same path,
/// and the handler sees the method as `GET`. The body is dropped on the way out
/// — by the core, which frames a HEAD response as body-forbidden from the real
/// request method.
///
/// Returns the method to dispatch under, plus the path params.
fn match_route(
    routes: &[RouteMatcher],
    method: &str,
    path: &str,
) -> Option<(String, HashMap<String, String>)> {
    for route in routes {
        if route.method == method {
            if let Some(params) = route.pattern.match_path(path) {
                return Some((method.to_string(), params));
            }
        }
    }
    if method == "HEAD" {
        for route in routes {
            if route.method == "GET" {
                if let Some(params) = route.pattern.match_path(path) {
                    return Some(("GET".to_string(), params));
                }
            }
        }
    }
    None
}

/// Turn a handler's response into the core's, deciding the `Content-Length`
/// the hyper path got for free from `Full<Bytes>`.
///
/// `auto_content_length` is **false**: every length here is deliberate, and a
/// HEAD-on-GET response is the reason. Node core synthesizes no length on a
/// body-forbidden response, but fastify sets one on HEAD so a client sees what
/// `GET` would have produced — the behaviour this binding has always had. A
/// status that forbids a body for its own sake (204/304/1xx) gets none because
/// none is added below, not because the core strips it.
pub(crate) fn into_core_response(response: FastifyResponse) -> perry_http_server::Response {
    let FastifyResponse {
        status,
        mut headers,
        body,
    } = response;
    let has_length = headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
    let status_forbids_body = status == 204 || status == 304 || (100..200).contains(&status);
    if !has_length && !status_forbids_body {
        headers.push(("content-length".to_string(), body.len().to_string()));
    }
    // Node's HTTP server sends `Date` on every response and spells it
    // capitalised; hyper supplied a lowercase `date` here. Pushed *after* the
    // length and *before* the core appends `Connection`/`Keep-Alive`, which is
    // the order Node emits (verified against fastify on the pinned oracle).
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("date")) {
        headers.push(("Date".to_string(), perry_http_server::wire::http_date_now()));
    }
    perry_http_server::Response {
        status,
        status_message: None,
        headers,
        body,
        trailers: Vec::new(),
        auto_content_length: false,
    }
}

/// `app.listen({ port }, callback?)` — start the server.
///
/// Returns as soon as the listener is bound and accepting; the TS-visible API
/// is "kick off the server, then live in the event loop". The bind is
/// synchronous on **both** transports so a port clash (`EADDRINUSE`) reaches
/// the `(err, address)` callback instead of being lost inside an accept task
/// after the caller has been told listening succeeded.
///
/// # Safety
///
/// `app_handle` must be a registered `FastifyApp` handle. `callback`
/// is an optional `*const ClosureHeader` (NaN-boxed or raw); pass `0`
/// for "no callback".
#[no_mangle]
pub unsafe extern "C" fn js_fastify_listen(app_handle: Handle, opts: f64, callback: i64) {
    // Extract port — accepts `{ port: 3000 }`, a bare number, or
    // falls back to 3000.
    let port = extract_port(opts);
    // Honor an explicit `{ reusePort: true }` (the Node/Bun listen option) in
    // addition to auto-enabling SO_REUSEPORT for cluster workers.
    let reuse_port = extract_reuse_port(opts) || crate::cluster_bind::is_cluster_worker();

    // Snapshot only route-matching metadata for the accept path. Handler
    // closure pointers stay in the FastifyApp handle and are read by the
    // main-thread pump during dispatch, so a server's lifetime never holds a
    // JS value where the collector cannot see it.
    let routes = Arc::new(
        get_handle::<FastifyApp>(app_handle)
            .map(|app| {
                app.routes
                    .iter()
                    .map(RouteMatcher::from_route)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );
    // Node diverts an upgrade request to `'upgrade'` only when the server has
    // a listener for it, and serves it as an ordinary request otherwise
    // (#4973). Read once, here, so the answer is a property of the listen call
    // rather than of whichever request arrives — which is also what the core's
    // `Host::takes_upgrades` contract asks for.
    let has_upgrade_handlers = get_handle::<FastifyApp>(app_handle)
        .map(|app| !app.upgrade_handlers.is_empty())
        .unwrap_or(false);

    if !perry_http_server::available(SUBSYSTEM) {
        // This agent owns no `turnloop::Loop` — a `worker_threads` agent, or
        // the `tokio-wait-driver` A/B arm. There is no second transport since
        // the hyper accept loop was deleted, so this is an error rather than a
        // silent fallback.
        fire_listen_error(
            callback,
            &std::io::Error::other("no event loop on this thread"),
            port,
        );
        return;
    }
    listen_on_turnloop(
        app_handle,
        callback,
        port,
        reuse_port,
        routes,
        has_upgrade_handlers,
    );
}

/// Bind and accept through [`perry_http_server`]. A bind failure reports
/// itself through the callback.
unsafe fn listen_on_turnloop(
    app_handle: Handle,
    callback: i64,
    port: u16,
    reuse_port: bool,
    routes: Arc<Vec<RouteMatcher>>,
    takes_upgrades: bool,
) {
    let (request_tx, request_rx) = mpsc::sync_channel::<FastifyPendingRequest>(REQUEST_QUEUE_DEPTH);
    let (upgrade_tx, upgrade_rx) = mpsc::sync_channel::<FastifyPendingUpgrade>(UPGRADE_QUEUE_DEPTH);
    let upgrade_depth = Arc::new(AtomicUsize::new(0));
    let listening = Arc::new(AtomicBool::new(true));
    let host = Arc::new(FastifyHost {
        routes,
        requests: request_tx,
        listening: listening.clone(),
        app_handle,
        takes_upgrades,
        upgrades: upgrade_tx,
        upgrade_depth: upgrade_depth.clone(),
    });
    let bound = match perry_http_server::listen(
        SUBSYSTEM,
        host,
        "0.0.0.0",
        port,
        511,
        reuse_port,
        // Node's `http.createServer` defaults `noDelay` to true and applies it
        // to every accepted connection. It is a separate argument from
        // `reuse_port` on purpose: P5's own listen path had this value sitting
        // in `reuse_port`'s slot for its whole life, so every turnloop HTTP
        // listener bound with SO_REUSEPORT on and Nagle on.
        true,
        KEEP_ALIVE_TIMEOUT_MS as u64 + KEEP_ALIVE_TIMEOUT_BUFFER_MS,
    ) {
        Ok(bound) => bound,
        Err(err) => {
            fire_listen_error(callback, &std::io::Error::other(err.message()), port);
            return;
        }
    };
    crate::cluster_bind::notify_listening("0.0.0.0", bound.port);
    let _server_handle = register_handle(FastifyServerHandle {
        port: bound.port,
        app_handle,
        listener_id: bound.listener_id,
        request_rx: Mutex::new(Some(request_rx)),
        upgrade_rx: Mutex::new(Some(upgrade_rx)),
        upgrade_depth,
        listening,
    });
    fire_listen_callback(callback, bound.port);
    println!("Server listening on http://0.0.0.0:{}", bound.port);
}

unsafe fn fire_listen_callback(callback: i64, port: u16) {
    if callback == 0 {
        return;
    }
    let raw = if (callback as u64 & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (callback as u64 & PTR_MASK) as *const RawClosureHeader
    } else {
        callback as *const RawClosureHeader
    };
    let address = format!("http://0.0.0.0:{}", port);
    let addr_str = alloc_string(&address);
    let addr_val = JsValue::from_string_ptr(addr_str.as_raw());
    let null_val = f64::from_bits(TAG_NULL);
    let closure = JsClosure::from_raw(raw);
    if !closure.is_null() {
        let _ = closure.call2(null_val, f64::from_bits(addr_val.bits()));
    }
}
