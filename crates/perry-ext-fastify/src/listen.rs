//! The fastify listen path: both transports, and the decision between them.
//!
//! Split out of `server.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap; declared as a `#[path]` child module of
//! `server` so `use super::*` resolves the way it did inline.
//!
//! The default is turnloop, through [`perry_http_server`]. The hyper accept
//! loop below survives for one case and declines at listen time when it
//! applies: an app with `app.server.on("upgrade", …)` handlers, whose
//! handshake ends in `perry_ext_ws::register_external_ws_stream` and needs an
//! owned `AsyncRead + AsyncWrite` stream a turnloop connection cannot produce.

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
    // An app with `'upgrade'` handlers keeps the hyper accept loop: the
    // handshake ends in `perry_ext_ws::register_external_ws_stream`, which
    // needs an owned `AsyncRead + AsyncWrite` stream that a turnloop
    // connection cannot produce. Read once, here, so the decision is a
    // property of the listen call rather than of whichever request arrives.
    let has_upgrade_handlers = get_handle::<FastifyApp>(app_handle)
        .map(|app| !app.upgrade_handlers.is_empty())
        .unwrap_or(false);

    if !has_upgrade_handlers && perry_http_server::available(SUBSYSTEM) {
        if listen_on_turnloop(app_handle, callback, port, reuse_port, routes) {
            return;
        }
        // A bind failure has already reported itself through the callback.
        return;
    }
    listen_on_hyper(app_handle, callback, port, reuse_port, routes);
}

/// Bind and accept through [`perry_http_server`]. Returns false only when the
/// listen failed *and* the failure was already reported to the callback.
unsafe fn listen_on_turnloop(
    app_handle: Handle,
    callback: i64,
    port: u16,
    reuse_port: bool,
    routes: Arc<Vec<RouteMatcher>>,
) -> bool {
    let (request_tx, request_rx) = mpsc::sync_channel::<FastifyPendingRequest>(REQUEST_QUEUE_DEPTH);
    let listening = Arc::new(AtomicBool::new(true));
    let host = Arc::new(FastifyHost {
        routes,
        requests: request_tx,
        listening: listening.clone(),
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
            return false;
        }
    };
    crate::cluster_bind::notify_listening("0.0.0.0", bound.port);
    let _server_handle = register_handle(FastifyServerHandle {
        port: bound.port,
        app_handle,
        shutdown_tx: None,
        listener_id: bound.listener_id,
        request_rx: Mutex::new(Some(request_rx)),
        upgrade_rx: Mutex::new(None),
        upgrade_depth: Arc::new(AtomicUsize::new(0)),
        listening,
    });
    fire_listen_callback(callback, bound.port);
    println!("Server listening on http://0.0.0.0:{}", bound.port);
    true
}

/// The hyper accept loop, for an app with `'upgrade'` handlers (see
/// `js_fastify_listen`). Unchanged from the pre-turnloop path except that the
/// response channel is now carried in a [`Reply`].
unsafe fn listen_on_hyper(
    app_handle: Handle,
    callback: i64,
    port: u16,
    reuse_port: bool,
    routes: Arc<Vec<RouteMatcher>>,
) {
    // Bind synchronously, BEFORE registering the server or firing the success
    // callback, so a bind failure (e.g. EADDRINUSE) reaches the `(err, address)`
    // callback as an error instead of being silently dropped inside the accept
    // task while the caller has already been told listening succeeded. Only
    // `from_std` needs a runtime context, so it stays in the spawned task below;
    // the bind + `set_nonblocking` that actually fail on a port clash run here.
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let std_listener = match crate::cluster_bind::bind_listener(addr, reuse_port) {
        Ok(l) => l,
        Err(e) => {
            fire_listen_error(callback, &e, port);
            return;
        }
    };
    if let Err(e) = std_listener.set_nonblocking(true) {
        fire_listen_error(callback, &e, port);
        return;
    }
    // `listen(0)` asks the OS for an ephemeral port; read the real one back so
    // the registered handle + callback report the actual bound port.
    let actual_port = std_listener.local_addr().map(|a| a.port()).unwrap_or(port);

    let (request_tx, request_rx) = mpsc::sync_channel::<FastifyPendingRequest>(REQUEST_QUEUE_DEPTH);
    // #1113 — separate channel for WebSocket upgrade events so a busy
    // request stream can't starve them.
    let (upgrade_tx, upgrade_rx) = mpsc::sync_channel::<FastifyPendingUpgrade>(256);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let upgrade_depth = Arc::new(AtomicUsize::new(0));

    let request_tx_for_spawn = request_tx.clone();
    let upgrade_tx_for_spawn = upgrade_tx.clone();
    let routes_for_spawn = routes.clone();
    let upgrade_depth_for_spawn = upgrade_depth.clone();

    // The accept loop must run as a cooperative task on the shared
    // multi-thread runtime. A plain `spawn_blocking` thread does not
    // reliably carry the runtime's reactor/worker context: with
    // `Handle::current().block_on(accept_loop)` the listener bound and
    // accepted connections, but the per-connection
    // `tokio::spawn(serve_connection)` tasks below were never driven — the
    // request bytes sat unread and every response hung. `spawn_blocking_with_reactor`
    // runs the closure inside a worker task, so `tokio::spawn`-ing the accept
    // loop drives it and its fan-out serve tasks on the worker pool.
    perry_ffi::spawn_blocking_with_reactor(move || {
        tokio::spawn(async move {
            // The bind already succeeded on the caller thread (so a port clash
            // was reported to the listen callback). Here we only report the
            // bound address for `cluster.on('listening')` and adopt the std
            // listener into the tokio reactor — `from_std` is the one step that
            // needs the runtime context this task provides.
            crate::cluster_bind::notify_listening("0.0.0.0", actual_port);
            let listener = match TcpListener::from_std(std_listener) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("[fastify] adopting listener failed: {}", e);
                    return;
                }
            };
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let request_tx = request_tx_for_spawn.clone();
                                let upgrade_tx = upgrade_tx_for_spawn.clone();
                                let routes = routes_for_spawn.clone();
                                let depth = upgrade_depth_for_spawn.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let upgrade_tx = upgrade_tx.clone();
                                        let routes = routes.clone();
                                        let depth = depth.clone();
                                        async move {
                                            handle_request(app_handle, req, request_tx, upgrade_tx, depth, routes).await
                                        }
                                    });
                                    // #1113: `.with_upgrades()` is REQUIRED for
                                    // `hyper::upgrade::on(&mut req)` to resolve.
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .with_upgrades()
                                        .await
                                    {
                                        // perry#924: hyper surfaces every malformed
                                        // client read as a per-connection error
                                        // (HTTP/2 prefaces, scanner garbage), which
                                        // the application never sees. Gate the noise.
                                        if std::env::var_os("PERRY_DEBUG").is_some() {
                                            eprintln!("Connection error: {}", e);
                                        }
                                    }
                                });
                            }
                            Err(e) => eprintln!("Accept error: {}", e),
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });
    });

    let _server_handle = register_handle(FastifyServerHandle {
        port: actual_port,
        app_handle,
        shutdown_tx: Some(shutdown_tx),
        listener_id: 0,
        request_rx: Mutex::new(Some(request_rx)),
        upgrade_rx: Mutex::new(Some(upgrade_rx)),
        upgrade_depth,
        listening: Arc::new(AtomicBool::new(true)),
    });

    fire_listen_callback(callback, actual_port);
    println!("Server listening on http://0.0.0.0:{}", actual_port);
}

/// Fire the user's `(err, address) => { … }` callback with a null error.
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

/// Hyper service function — match the route, hand the request to the
/// main thread via mpsc, await the response.
async fn handle_request(
    app_handle: Handle,
    req: Request<Incoming>,
    request_tx: mpsc::SyncSender<FastifyPendingRequest>,
    upgrade_tx: mpsc::SyncSender<FastifyPendingUpgrade>,
    upgrade_depth: Arc<AtomicUsize>,
    routes: Arc<Vec<RouteMatcher>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let path = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };

    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(name.to_string().to_lowercase(), v.to_string());
        }
    }

    // #1113: detect WebSocket upgrade requests. The user's pattern
    //
    //   import { WebSocketServer } from "ws";
    //   const wss = new WebSocketServer({ noServer: true });
    //   app.server.on("upgrade", (req, socket, head) => {
    //     wss.handleUpgrade(req, socket, head, (sock) => { ... });
    //   });
    //
    // expects the fastify accept loop to surface upgrade requests via
    // `app.server`'s registered `"upgrade"` handler. Branch into the
    // handshake path: build the 101 response synchronously and spawn
    // a task that awaits hyper's upgraded stream, completes the
    // tungstenite server handshake, registers the WebSocketStream
    // with perry-ext-ws, and queues a `FastifyPendingUpgrade` for the
    // main-thread pump to fire the registered handlers. Mirror of
    // perry-ext-http's #577 Phase 4 path.
    if crate::upgrade::is_websocket_upgrade(&req) {
        return handle_fastify_websocket_upgrade(
            app_handle,
            req,
            method,
            path,
            headers,
            upgrade_tx,
            upgrade_depth,
        )
        .await;
    }

    let body = match req.collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            if bytes.is_empty() {
                None
            } else {
                Some(bytes.to_vec())
            }
        }
        Err(_) => None,
    };

    // Match: first try the exact method, then — for HEAD — fall back to
    // a GET route with the same path. Node fastify auto-handles HEAD
    // against any registered GET (via `app.head` shadowing) by running
    // the GET handler and dropping the body before sending. We do the
    // same: rewrite the method to GET so the handler sees a vanilla
    // request, then strip the body on the way out (see `head_for_get`
    // below). #1120 part 2.
    let mut matched_params = HashMap::new();
    let mut found_route = false;
    let mut head_for_get = false;
    for route in routes.iter() {
        if route.method == method {
            if let Some(params) = route.pattern.match_path(&path) {
                matched_params = params;
                found_route = true;
                break;
            }
        }
    }
    if !found_route && method == "HEAD" {
        for route in routes.iter() {
            if route.method == "GET" {
                if let Some(params) = route.pattern.match_path(&path) {
                    matched_params = params;
                    found_route = true;
                    head_for_get = true;
                    break;
                }
            }
        }
    }

    if !found_route {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"Not Found"}"#)))
            .unwrap());
    }

    let (response_tx, response_rx) = oneshot::channel::<FastifyResponse>();
    // When fronting a GET handler for an inbound HEAD, surface the
    // method as `GET` to the handler — Node fastify's shadowing
    // semantics. The body-drop happens below in the hyper response
    // assembly.
    let dispatch_method = if head_for_get {
        "GET".to_string()
    } else {
        method.clone()
    };
    let pending = FastifyPendingRequest {
        method: dispatch_method,
        path,
        headers,
        body,
        params: matched_params,
        reply: Reply::Hyper(response_tx),
    };

    if request_tx.try_send(pending).is_err() {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::from("Server unavailable")))
            .unwrap());
    }

    // Wake the main thread so it doesn't wait on its 10ms timeout.
    perry_ffi::notify_main_thread();

    match response_rx.await {
        Ok(fr) => {
            let body_len = fr.body.len();
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(fr.status).unwrap_or(StatusCode::OK));
            let mut had_content_length = false;
            for (name, value) in fr.headers {
                if name.eq_ignore_ascii_case("content-length") {
                    had_content_length = true;
                }
                builder = builder.header(name, value);
            }
            let body_bytes = if head_for_get {
                // HEAD response: no body on the wire, but expose the
                // would-have-been size via Content-Length so clients
                // (curl -I, browsers, monitoring) see what GET would
                // produce. Mirror of Node fastify's HEAD-on-GET path.
                if !had_content_length {
                    builder = builder.header("content-length", body_len.to_string());
                }
                Bytes::new()
            } else {
                Bytes::from(fr.body)
            };
            Ok(builder.body(Full::new(body_bytes)).unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("Handler error")))
            .unwrap()),
    }
}

/// #1113 — WebSocket upgrade dispatch (mirror of perry-ext-http's
/// `handle_websocket_upgrade`, issue #577 Phase 4).
///
/// Synchronously builds the 101 response (so hyper drives the protocol
/// switch) and spawns a tokio task that awaits the upgraded stream,
/// finishes the handshake server-side via
/// `tokio_tungstenite::WebSocketStream::from_raw_socket`, registers
/// the stream with perry-ext-ws, and queues a `FastifyPendingUpgrade`
/// on the per-server channel; the main-thread pump fires the
/// `app.server.on("upgrade", …)` handlers with `(req, ws_id, head)`.
async fn handle_fastify_websocket_upgrade(
    app_handle: Handle,
    mut req: Request<Incoming>,
    method: String,
    path: String,
    headers: HashMap<String, String>,
    upgrade_tx: mpsc::SyncSender<FastifyPendingUpgrade>,
    upgrade_depth: Arc<AtomicUsize>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Compute the Sec-WebSocket-Accept value before consuming req.
    let accept_value = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(|k| tokio_tungstenite::tungstenite::handshake::derive_accept_key(k.as_bytes()))
        .unwrap_or_default();

    // Spawn a task that waits for hyper to perform the protocol
    // switch, completes the tungstenite handshake, and hands the
    // resulting stream to perry-ext-ws.
    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(&mut req).await {
            Ok(u) => u,
            Err(_) => return,
        };
        let io = TokioIo::new(upgraded);
        let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
            io,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        let ws_id = perry_ext_ws::register_external_ws_stream(ws);
        let pending = FastifyPendingUpgrade {
            app_handle,
            method,
            path,
            headers,
            ws_id,
        };
        if upgrade_tx.try_send(pending).is_ok() {
            upgrade_depth.fetch_add(1, Ordering::AcqRel);
        }
        perry_ffi::notify_main_thread();
    });

    Ok(Response::builder()
        .status(101)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", accept_value)
        .body(Full::new(Bytes::new()))
        .unwrap())
}
