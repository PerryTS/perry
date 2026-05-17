//! HTTP Server loop and request dispatch
//!
//! Uses the existing Hyper-based HTTP framework for serving requests.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};

use super::{ClosurePtr, FastifyApp, FastifyContext};
use crate::common::{get_handle, register_handle, Handle, RUNTIME};

/// Server handle for managing the running server
pub struct FastifyServerHandle {
    pub port: u16,
    pub app_handle: Handle,
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Pending request waiting for TypeScript handler
pub struct FastifyPendingRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub params: HashMap<String, String>,
    pub response_tx: tokio::sync::oneshot::Sender<FastifyResponse>,
}

/// Response from TypeScript handler
pub struct FastifyResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Start the server and begin listening
#[no_mangle]
pub unsafe extern "C" fn js_fastify_listen(app_handle: Handle, opts: f64, callback: i64) {
    // Extract port from opts
    let port: u16 = {
        let jsv = JSValue::from_bits(opts.to_bits());
        if jsv.is_pointer() {
            let ptr = jsv.as_pointer::<perry_runtime::ObjectHeader>();
            let port_key = js_string_from_bytes(b"port".as_ptr(), 4);
            let port_val = perry_runtime::js_object_get_field_by_name_f64(ptr, port_key);
            let port_jsv = JSValue::from_bits(port_val.to_bits());
            if port_jsv.is_number() {
                port_val as u16
            } else {
                3000
            }
        } else if opts > 0.0 {
            opts as u16
        } else {
            3000
        }
    };

    let (request_tx, mut request_rx) = mpsc::channel::<FastifyPendingRequest>(1024);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let request_tx = Arc::new(request_tx);

    // Clone app for matching in the server task
    let app_for_server = if let Some(app) = get_handle::<FastifyApp>(app_handle) {
        app.routes.clone()
    } else {
        Vec::new()
    };
    let routes_arc = Arc::new(app_for_server);

    // Fastify dispatches request callbacks on tokio worker threads whose
    // stacks the main-thread GC can't scan. Mark GC-unsafe so user-level
    // `gc()` calls from setInterval don't collect objects still
    // referenced from worker stacks (issue #31).
    perry_runtime::gc::js_gc_enter_unsafe_zone();

    // Spawn the server
    let routes_for_spawn = routes_arc.clone();
    RUNTIME.spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to port {}: {}", port, e);
                return;
            }
        };

        let routes = routes_for_spawn;

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let request_tx = request_tx.clone();
                            let routes = routes.clone();

                            tokio::spawn(async move {
                                let service = service_fn(move |req: Request<Incoming>| {
                                    let request_tx = request_tx.clone();
                                    let routes = routes.clone();
                                    async move {
                                        handle_request(req, request_tx, routes).await
                                    }
                                });

                                if let Err(e) = http1::Builder::new()
                                    .serve_connection(io, service)
                                    .await
                                {
                                    // perry#924: hyper surfaces every malformed
                                    // client read as a per-connection error (HTTP/2
                                    // prefaces, scanner garbage). The application
                                    // never sees these requests, so logging them by
                                    // default just floods PM2 error logs. Gate
                                    // behind `PERRY_DEBUG=1` for diagnosis.
                                    if std::env::var_os("PERRY_DEBUG").is_some() {
                                        eprintln!("Connection error: {}", e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("Accept error: {}", e);
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    println!("Server shutting down");
                    break;
                }
            }
        }
    });

    // Store server handle
    let _server_handle = register_handle(FastifyServerHandle {
        port,
        app_handle,
        shutdown_tx: Some(shutdown_tx),
    });

    // Call callback with (null, address)
    if callback != 0 {
        let address = format!("http://0.0.0.0:{}", port);
        let addr_ptr = js_string_from_bytes(address.as_ptr(), address.len() as u32);
        let addr_val = f64::from_bits(JSValue::string_ptr(addr_ptr).bits());

        // Call callback(null, address)
        let closure_ptr = callback as *const perry_runtime::ClosureHeader;
        perry_runtime::js_closure_call2(
            closure_ptr,
            f64::from_bits(JSValue::null().bits()),
            addr_val,
        );
    }

    println!("Server listening on http://0.0.0.0:{}", port);

    // Enter the main event loop
    event_loop(app_handle, &mut request_rx);
}

/// Handle incoming HTTP request - match route and forward to TypeScript
async fn handle_request(
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<FastifyPendingRequest>>,
    routes: Arc<Vec<super::Route>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    // Include query string in the path so FastifyContext can parse it
    let path = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(name.to_string().to_lowercase(), v.to_string());
        }
    }

    // Read body
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

    // Match route
    let mut matched_params = HashMap::new();
    let mut found_route = false;

    for route in routes.iter() {
        if route.method == method {
            if let Some(params) = route.pattern.match_path(&path) {
                matched_params = params;
                found_route = true;
                break;
            }
        }
    }

    if !found_route {
        // Return 404
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from("{\"error\":\"Not Found\"}")))
            .unwrap());
    }

    // Create oneshot channel for response
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<FastifyResponse>();

    // Send request to TypeScript handler
    let pending = FastifyPendingRequest {
        method,
        path,
        headers,
        body,
        params: matched_params,
        response_tx,
    };

    if request_tx.send(pending).await.is_err() {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::from("Server unavailable")))
            .unwrap());
    }

    // Wait for response
    match response_rx.await {
        Ok(fastify_response) => {
            let mut response = Response::builder()
                .status(StatusCode::from_u16(fastify_response.status).unwrap_or(StatusCode::OK));

            for (name, value) in fastify_response.headers {
                response = response.header(name, value);
            }

            Ok(response
                .body(Full::new(Bytes::from(fastify_response.body)))
                .unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("Handler error")))
            .unwrap()),
    }
}

/// Main event loop - process incoming requests
fn event_loop(app_handle: Handle, request_rx: &mut mpsc::Receiver<FastifyPendingRequest>) {
    let app = match get_handle::<FastifyApp>(app_handle) {
        Some(a) => a,
        None => return,
    };

    loop {
        // Process any pending stdlib operations (promises, etc.)
        crate::common::js_stdlib_process_pending();

        // Process any pending microtasks
        perry_runtime::js_promise_run_microtasks();

        // Try to receive a request (non-blocking with small timeout)
        let result = RUNTIME.block_on(async {
            tokio::time::timeout(std::time::Duration::from_millis(10), request_rx.recv()).await
        });

        if let Ok(Some(pending)) = result {
            // Create context
            let ctx = FastifyContext::new(
                0, // request_id
                pending.method.clone(),
                pending.path.clone(),
                pending.headers.clone(),
                pending.body.clone(),
                pending.params.clone(),
            );
            let ctx_handle = register_handle(ctx);

            // Find matching route and call handler
            let mut response_sent = false;

            // NaN-box the context handle with POINTER_TAG for hook calls
            let nanboxed_ctx_for_hooks =
                f64::from_bits(0x7FFD_0000_0000_0000 | (ctx_handle as u64 & 0x0000_FFFF_FFFF_FFFF));

            // Collect hook ptrs (copy i64 values to avoid holding borrow on app during hook execution)
            let on_request_hooks: Vec<ClosurePtr> = app.hooks.on_request.to_vec();
            let pre_handler_hooks: Vec<ClosurePtr> = app.hooks.pre_handler.to_vec();

            // Run onRequest hooks (e.g., auth middleware, rate limiting, CORS)
            for hook in &on_request_hooks {
                if unsafe { call_hook_awaiting(*hook, nanboxed_ctx_for_hooks, ctx_handle) } {
                    response_sent = true;
                    break;
                }
            }

            // Run preHandler hooks (if no response sent yet)
            if !response_sent {
                for hook in &pre_handler_hooks {
                    if unsafe { call_hook_awaiting(*hook, nanboxed_ctx_for_hooks, ctx_handle) } {
                        response_sent = true;
                        break;
                    }
                }
            }

            // Call route handler (if no hook sent a response)
            // undefined NaN-box value: tag 0x7FFC, payload 1
            let undefined_bits: u64 = 0x7FFC_0000_0000_0001;
            let mut final_result: f64 = f64::from_bits(undefined_bits);

            if !response_sent {
                if let Some((route, _)) = app.match_route(&pending.method, &pending.path) {
                    let handler = route.handler;

                    // NaN-box the context handle with POINTER_TAG so it can be dispatched
                    // by js_native_call_method when the handler calls request/reply methods
                    let nanboxed_ctx = nanboxed_ctx_for_hooks; // same value, different name for clarity

                    // Call handler(request, reply) - both are the context handle
                    let result = {
                        let closure_ptr = handler as *const perry_runtime::ClosureHeader;
                        perry_runtime::js_closure_call2(closure_ptr, nanboxed_ctx, nanboxed_ctx)
                    };

                    // Process any async operations
                    crate::common::js_stdlib_process_pending();
                    perry_runtime::js_promise_run_microtasks();

                    // Check if handler returned a promise (NaN-boxed pointer to a Promise)
                    final_result = result;
                    let jsv = JSValue::from_bits(result.to_bits());
                    if jsv.is_pointer() {
                        let ptr = jsv.as_pointer::<perry_runtime::Promise>();
                        // Try to treat it as a promise and wait for it
                        if { perry_runtime::js_is_promise(ptr as *mut perry_runtime::Promise) } != 0
                        {
                            wait_for_promise(ptr as *mut perry_runtime::Promise);
                            // Read state AFTER the wait — `js_promise_value`
                            // returns `(*promise).value` unconditionally and
                            // that field stays at `0.0` for rejected and
                            // pending promises (rejection writes `reason`,
                            // not `value`). Without this branch, an
                            // unhandled rejection inside a route handler
                            // would serialize the literal byte `0` as the
                            // response body — issue #748.
                            let st = {
                                perry_runtime::js_promise_state(ptr as *mut perry_runtime::Promise)
                            };
                            if st == 2 {
                                if let Some(ctx) =
                                    { crate::common::get_handle_mut::<FastifyContext>(ctx_handle) }
                                {
                                    ctx.status_code = 500;
                                    let reason = {
                                        perry_runtime::promise::js_promise_reason(
                                            ptr as *mut perry_runtime::Promise,
                                        )
                                    };
                                    ctx.response_body = Some(render_rejection_body(reason));
                                }
                                final_result = f64::from_bits(0x7FFC_0000_0000_0001);
                            } else {
                                final_result = {
                                    perry_runtime::js_promise_value(
                                        ptr as *mut perry_runtime::Promise,
                                    )
                                };
                            }
                        }
                    }
                }
            }

            // Always send a response (from hook or route handler)
            if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
                let response = FastifyResponse {
                    status: ctx.status_code,
                    headers: ctx.response_headers.clone(),
                    body: ctx.response_body.clone().unwrap_or_else(|| {
                        // If no explicit body, use handler return value
                        build_response_body(final_result)
                    }),
                };

                // Ensure content-type is set
                let mut final_response = response;
                if !final_response
                    .headers
                    .iter()
                    .any(|(k, _)| k.to_lowercase() == "content-type")
                {
                    final_response
                        .headers
                        .push(("content-type".to_string(), "application/json".to_string()));
                }

                let _ = pending.response_tx.send(final_response);
                response_sent = true;
            }

            let _ = response_sent; // suppress unused warning
        }
    }
}

/// Call a hook closure, await any returned Promise, and return whether ctx.sent is true.
/// Returns true if the hook sent a response (e.g., 401 from auth middleware).
unsafe fn call_hook_awaiting(hook: ClosurePtr, ctx_f64: f64, ctx_handle: Handle) -> bool {
    let closure_ptr = hook as *const perry_runtime::ClosureHeader;
    let result = perry_runtime::js_closure_call2(closure_ptr, ctx_f64, ctx_f64);

    // Process pending async operations
    crate::common::js_stdlib_process_pending();
    perry_runtime::js_promise_run_microtasks();

    // If hook returned a Promise, wait for it to resolve/reject
    let jsv = JSValue::from_bits(result.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<perry_runtime::Promise>();
        if perry_runtime::js_is_promise(ptr as *mut perry_runtime::Promise) != 0 {
            wait_for_promise(ptr as *mut perry_runtime::Promise);
        }
    }

    // Return whether the hook sent a response (e.g., auth middleware sent 401)
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        ctx.sent
    } else {
        false
    }
}

/// Wait until a promise settles, driving microtasks, the stdlib pump,
/// and timer ticks every iteration. Blocks on
/// `perry_runtime::event_pump::js_wait_for_event` (a condvar with a 1 s
/// idle cap) instead of `thread::sleep`, so the dispatcher wakes the
/// moment any stdlib worker calls `js_notify_main_thread` — the same
/// wake `js_promise_resolve` / `js_promise_reject` fire when the
/// awaited chain advances.
///
/// Mirrors the codegen-emitted `await` body in
/// `crates/perry-codegen/src/expr.rs` (the "=== wait ===" block at
/// lines ~9645-9665): no fixed iteration limit, condvar-based wait.
///
/// ### Why the old polling loop is wrong (issue #748)
///
/// The previous implementation looped 10_000 × 100 us = ~1 s and then
/// returned regardless of whether the promise had settled. Callers
/// then read `js_promise_value(ptr)` which returns `(*promise).value`
/// — `0.0` for a still-Pending Promise (it's initialized to zero in
/// `Promise::new` and only overwritten by `js_promise_resolve`). The
/// dispatcher serialized that `0.0` as the response body, yielding the
/// literal ASCII byte `0x30` ("0") with HTTP 200 (default
/// `status_code` — `reply.code(201)` was never reached because the
/// handler chain hadn't returned). Routes that do many awaits
/// (argon2 hashing + multi-step DB writes, etc.) routinely exceed
/// 1 s on a cold connection pool; every operation after the timeout
/// silently no-op'd because the dispatcher returned and stopped
/// pumping microtasks for the orphaned chain.
fn wait_for_promise(promise_ptr: *mut perry_runtime::Promise) {
    // First pump synchronously — handles the already-settled case
    // (e.g. `async () => 42` whose promise is fulfilled before this
    // function is even called) without entering the wait path.
    crate::common::js_stdlib_process_pending();
    perry_runtime::js_promise_run_microtasks();
    let mut state = perry_runtime::js_promise_state(promise_ptr);
    if state != 0 {
        return;
    }
    loop {
        // Drive timers, the stdlib pump, and microtasks every tick
        // — mirrors the codegen-emitted await body.
        let _ = perry_runtime::timer::js_timer_tick();
        let _ = perry_runtime::timer::js_callback_timer_tick();
        let _ = perry_runtime::timer::js_interval_timer_tick();
        crate::common::js_stdlib_process_pending();
        perry_runtime::js_promise_run_microtasks();
        // Condvar wait: blocks until a notify arrives or the 1 s
        // idle cap elapses. `js_promise_resolve` / `js_promise_reject`
        // fire `js_notify_main_thread`, so the wake happens the
        // instant the chain advances.
        perry_runtime::event_pump::js_wait_for_event();
        state = perry_runtime::js_promise_state(promise_ptr);
        if state != 0 {
            return;
        }
    }
}

/// Render a Promise rejection reason as a `{ "error": ... }` JSON body
/// for the 500 response surfaced by the dispatcher on async-handler
/// rejection (issue #748).
fn render_rejection_body(reason: f64) -> Vec<u8> {
    let jsv = JSValue::from_bits(reason.to_bits());
    if jsv.is_string() {
        let s = build_response_body(reason);
        let mut out = b"{\"error\":".to_vec();
        out.push(b'"');
        for b in s {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                0x00..=0x1f => out.extend_from_slice(format!("\\u{:04x}", b).as_bytes()),
                _ => out.push(b),
            }
        }
        out.push(b'"');
        out.push(b'}');
        return out;
    }
    if jsv.is_pointer() {
        extern "C" {
            fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
        }
        unsafe {
            let str_ptr = js_json_stringify(reason, 0);
            if !str_ptr.is_null() {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let inner = std::slice::from_raw_parts(data_ptr, len).to_vec();
                let mut out = b"{\"error\":".to_vec();
                out.extend_from_slice(&inner);
                out.push(b'}');
                return out;
            }
        }
    }
    let body = build_response_body(reason);
    let mut out = b"{\"error\":".to_vec();
    if body.is_empty() {
        out.extend_from_slice(b"null");
    } else {
        out.push(b'"');
        for b in body {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                _ => out.push(b),
            }
        }
        out.push(b'"');
    }
    out.push(b'}');
    out
}

/// Build response body from handler return value
fn build_response_body(value: f64) -> Vec<u8> {
    let jsv = JSValue::from_bits(value.to_bits());

    // If undefined/null, return empty object
    if jsv.is_undefined() || jsv.is_null() {
        return b"{}".to_vec();
    }

    // If string, return as-is
    if jsv.is_string() {
        unsafe {
            let ptr = perry_runtime::js_get_string_pointer_unified(value);
            if ptr != 0 {
                let header = ptr as *const StringHeader;
                let len = (*header).byte_len as usize;
                let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                return bytes.to_vec();
            }
        }
    }

    // For objects/arrays (pointers), use js_json_stringify for proper JSON serialization
    if jsv.is_pointer() {
        extern "C" {
            fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
        }
        unsafe {
            let str_ptr = js_json_stringify(value, 0);
            if !str_ptr.is_null() {
                let len = (*str_ptr).byte_len as usize;
                let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data_ptr, len);
                return bytes.to_vec();
            }
        }
    }

    // Fallback: convert to string
    unsafe {
        let json_ptr = perry_runtime::js_jsvalue_to_string(value);
        if !json_ptr.is_null() {
            let len = (*json_ptr).byte_len as usize;
            let data_ptr = (json_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data_ptr, len);
            return bytes.to_vec();
        }
    }

    b"{}".to_vec()
}

/// Close the server
#[no_mangle]
pub unsafe extern "C" fn js_fastify_close(server_handle: Handle) -> bool {
    if let Some(_server) = get_handle::<FastifyServerHandle>(server_handle) {
        // Shutdown will be triggered when the handle is dropped
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fastify::FastifyApp;
    use std::time::Duration;

    /// Test that the HTTP server can start and handle basic routing
    #[test]
    fn test_handle_request_routing() {
        // Create routes
        let mut routes = Vec::new();

        // Add GET / route
        routes.push(super::super::Route {
            method: "GET".to_string(),
            pattern: super::super::RoutePattern::parse("/"),
            handler: 1,
        });

        // Add GET /users/:id route
        routes.push(super::super::Route {
            method: "GET".to_string(),
            pattern: super::super::RoutePattern::parse("/users/:id"),
            handler: 2,
        });

        // Add POST /users route
        routes.push(super::super::Route {
            method: "POST".to_string(),
            pattern: super::super::RoutePattern::parse("/users"),
            handler: 3,
        });

        let routes = Arc::new(routes);

        // Test route matching
        for route in routes.iter() {
            if route.method == "GET" && route.pattern.match_path("/").is_some() {
                assert_eq!(route.handler, 1);
            }
            if route.method == "GET" && route.pattern.match_path("/users/42").is_some() {
                assert_eq!(route.handler, 2);
            }
            if route.method == "POST" && route.pattern.match_path("/users").is_some() {
                assert_eq!(route.handler, 3);
            }
        }
    }

    /// Test the FastifyResponse struct
    #[test]
    fn test_fastify_response() {
        let response = FastifyResponse {
            status: 200,
            headers: vec![
                ("content-type".to_string(), "application/json".to_string()),
                ("x-custom".to_string(), "value".to_string()),
            ],
            body: b"{\"ok\":true}".to_vec(),
        };

        assert_eq!(response.status, 200);
        assert_eq!(response.headers.len(), 2);
        assert_eq!(response.body, b"{\"ok\":true}");
    }

    /// Test building response body from different value types
    #[test]
    fn test_build_response_body() {
        // Test undefined
        let body = build_response_body(f64::from_bits(JSValue::undefined().bits()));
        assert_eq!(body, b"{}");

        // Test null
        let body = build_response_body(f64::from_bits(JSValue::null().bits()));
        assert_eq!(body, b"{}");

        // Test number (these get converted via js_jsvalue_to_string, which returns "{}" for numbers without proper string conversion)
        // In practice, handler return values would be objects that get JSON serialized
    }

    /// Test the context creation
    #[test]
    fn test_context_creation() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("host".to_string(), "localhost:3000".to_string());

        let mut params = HashMap::new();
        params.insert("id".to_string(), "42".to_string());

        let ctx = FastifyContext::new(
            1,
            "GET".to_string(),
            "/users/42?foo=bar".to_string(),
            headers,
            Some(b"{}".to_vec()),
            params,
        );

        assert_eq!(ctx.method, "GET");
        assert_eq!(ctx.url, "/users/42");
        assert_eq!(ctx.query_string, "foo=bar");
        assert_eq!(ctx.params.get("id"), Some(&"42".to_string()));
        assert_eq!(ctx.status_code, 200);
        assert!(!ctx.sent);
    }

    /// Test context query params parsing
    #[test]
    fn test_context_query_params() {
        let ctx = FastifyContext::new(
            1,
            "GET".to_string(),
            "/search?q=hello&page=1&limit=10".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );

        assert_eq!(ctx.get_query_param("q"), Some("hello".to_string()));
        assert_eq!(ctx.get_query_param("page"), Some("1".to_string()));
        assert_eq!(ctx.get_query_param("limit"), Some("10".to_string()));
        assert_eq!(ctx.get_query_param("missing"), None);

        let all_params = ctx.get_query_params();
        assert_eq!(all_params.len(), 3);
    }

    /// Test context header access
    #[test]
    fn test_context_headers() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "application/json".to_string());
        headers.insert("authorization".to_string(), "Bearer token123".to_string());

        let ctx = FastifyContext::new(
            1,
            "POST".to_string(),
            "/api/data".to_string(),
            headers,
            None,
            HashMap::new(),
        );

        assert_eq!(ctx.get_header("content-type"), Some("application/json"));
        assert_eq!(ctx.get_header("authorization"), Some("Bearer token123"));
        assert_eq!(ctx.get_header("missing"), None);
    }

    /// Test context reply methods
    #[test]
    fn test_context_reply() {
        let mut ctx = FastifyContext::new(
            1,
            "GET".to_string(),
            "/".to_string(),
            HashMap::new(),
            None,
            HashMap::new(),
        );

        // Test status setting
        ctx.set_status(201);
        assert_eq!(ctx.status_code, 201);

        // Test header adding
        ctx.add_header("x-custom", "value");
        assert_eq!(ctx.response_headers.len(), 1);
        assert_eq!(
            ctx.response_headers[0],
            ("x-custom".to_string(), "value".to_string())
        );
    }

    /// Test FastifyApp route management
    #[test]
    fn test_app_all_methods() {
        let mut app = FastifyApp::new();

        app.add_route("GET", "/resource", 1);
        app.add_route("POST", "/resource", 2);
        app.add_route("PUT", "/resource", 3);
        app.add_route("DELETE", "/resource", 4);
        app.add_route("PATCH", "/resource", 5);
        app.add_route("HEAD", "/resource", 6);
        app.add_route("OPTIONS", "/resource", 7);

        assert_eq!(app.routes.len(), 7);

        // Verify each method matches correctly
        assert_eq!(app.match_route("GET", "/resource").unwrap().0.handler, 1);
        assert_eq!(app.match_route("POST", "/resource").unwrap().0.handler, 2);
        assert_eq!(app.match_route("PUT", "/resource").unwrap().0.handler, 3);
        assert_eq!(app.match_route("DELETE", "/resource").unwrap().0.handler, 4);
        assert_eq!(app.match_route("PATCH", "/resource").unwrap().0.handler, 5);
        assert_eq!(app.match_route("HEAD", "/resource").unwrap().0.handler, 6);
        assert_eq!(
            app.match_route("OPTIONS", "/resource").unwrap().0.handler,
            7
        );
    }
}
