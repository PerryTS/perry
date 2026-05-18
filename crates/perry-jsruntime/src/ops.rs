//! Custom Deno ops for Perry runtime integration
//!
//! These ops allow JavaScript code to call back into native Perry code.

use deno_core::{extension, op2};
use deno_error::JsErrorBox;
use std::collections::HashMap;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

#[op2]
#[string]
fn op_perry_log(#[string] message: String) -> String {
    log::info!("[JS] {}", message);
    message
}

#[op2]
#[serde]
fn op_perry_call_native(
    #[string] func_name: String,
    #[serde] args: Vec<serde_json::Value>,
) -> serde_json::Value {
    log::debug!("Native call: {} with {} args", func_name, args.len());
    // TODO: Look up function in registry and call it
    serde_json::Value::Null
}

#[op2(fast)]
fn op_perry_print(#[string] message: String) {
    let mut stdout = io::stdout();
    let _ = writeln!(stdout, "{}", message);
    let _ = stdout.flush();
}

/// Synchronous HTTP fetch op for V8's fetch() polyfill.
/// Uses ureq (blocking) to avoid Tokio runtime conflicts when called
/// from within js_await_js_promise's block_on context.
#[op2]
#[serde]
fn op_perry_fetch(
    #[string] url: String,
    #[string] method: String,
    #[string] body: String,
    #[serde] headers: HashMap<String, String>,
) -> Result<serde_json::Value, JsErrorBox> {
    let agent = ureq::agent();
    let method_upper = method.to_uppercase();

    let mut req = agent.request(&method_upper, &url);

    for (key, value) in &headers {
        req = req.set(key, value);
    }

    let resp = if !body.is_empty() {
        req.set("Content-Type", "application/json")
            .send_string(&body)
    } else {
        req.call()
    };

    match resp {
        Ok(resp) => {
            let status = resp.status();
            let status_text = resp.status_text().to_string();

            let mut resp_headers = serde_json::Map::new();
            for name in resp.headers_names() {
                if let Some(value) = resp.header(&name) {
                    resp_headers.insert(
                        name.to_string(),
                        serde_json::Value::String(value.to_string()),
                    );
                }
            }

            let resp_body = resp.into_string().unwrap_or_default();

            Ok(serde_json::json!({
                "status": status,
                "statusText": status_text,
                "headers": resp_headers,
                "body": resp_body,
            }))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let resp_body = resp.into_string().unwrap_or_default();
            Ok(serde_json::json!({
                "status": code,
                "statusText": "Error",
                "headers": {},
                "body": resp_body,
            }))
        }
        Err(e) => Err(JsErrorBox::generic(format!("fetch error: {}", e))),
    }
}

// ============================================================================
// node:http createServer support for the V8 fallback path (issue follow-up
// to #678 — express/fastify need a real HTTP server inside the V8 sandbox).
//
// Architecture:
//
//   JS side (modules.rs `node:http` stub)                  Rust side (ops)
//   ┌─────────────────────────────────┐                   ┌────────────────┐
//   │ const s = http.createServer(h)  │                   │                │
//   │ s.listen(port, cb)              │ op_perry_http_    │ tokio TcpListener
//   │   ↳ accept loop:                │    listen(port)   │   + hyper http1│
//   │     async loop {                │ ────────────────► │   spawned on    │
//   │       const r = await           │                   │   shared tokio  │
//   │         op_accept(serverId)     │ op_perry_http_    │   runtime       │
//   │       run user handler(r,res)   │    accept(id)     │                │
//   │       op_respond(reqId, status, │ ◄──────────────── │ pushes pending  │
//   │                  headers, body) │                   │ via mpsc        │
//   │     }                           │                   │                │
//   └─────────────────────────────────┘                   └────────────────┘
//
// Goal: enough HTTP server semantics that express's `.listen(port, cb)`
// + a single `(req, res)` handler that calls `res.writeHead` + `res.end`
// can be smoke-tested. Bodies are passed as strings (utf-8 lossy);
// streaming bodies are NOT supported here. For richer needs, the native
// HTTP server in perry-ext-http-server is the path.
// ============================================================================

static NEXT_SERVER_ID: AtomicI32 = AtomicI32::new(1);
static NEXT_REQUEST_ID: AtomicI32 = AtomicI32::new(1);
static ACTIVE_SERVERS: AtomicI32 = AtomicI32::new(0);

struct PerryHttpServer {
    /// Pending incoming requests pushed by the hyper service fn.
    /// tokio::sync::Mutex so we can hold the lock across an `.await`.
    request_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<PendingHttpRequest>>,
    /// Drops cause the accept loop to exit.
    shutdown_tx: Mutex<Option<oneshot::Sender<()>>>,
    /// Set true once `op_perry_http_close` runs (clears the active counter).
    closed: Mutex<bool>,
}

struct PendingHttpRequest {
    id: i32,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    /// Oneshot the hyper service fn waits on for the response.
    response_tx: oneshot::Sender<HttpResponseShape>,
}

#[derive(Default)]
struct HttpResponseShape {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn server_registry() -> &'static Mutex<HashMap<i32, Arc<PerryHttpServer>>> {
    static REG: OnceLock<Mutex<HashMap<i32, Arc<PerryHttpServer>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_response_registry() -> &'static Mutex<HashMap<i32, oneshot::Sender<HttpResponseShape>>> {
    static REG: OnceLock<Mutex<HashMap<i32, oneshot::Sender<HttpResponseShape>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns >0 while any V8-fallback http.Server is bound and not yet
/// closed. Lets the outer pump keep the program alive while a server
/// is listening even if no other async work is pending.
pub(crate) fn perry_http_active_count() -> i32 {
    ACTIVE_SERVERS.load(Ordering::SeqCst)
}

async fn handle_request_for_perry_http(
    server_id: i32,
    req: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let url = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };
    let mut headers: Vec<(String, String)> = Vec::new();
    for (name, value) in req.headers().iter() {
        if let Ok(v) = value.to_str() {
            headers.push((name.as_str().to_string(), v.to_string()));
        }
    }
    let body_bytes: Vec<u8> = match req.collect().await {
        Ok(c) => c.to_bytes().to_vec(),
        Err(_) => Vec::new(),
    };

    let (response_tx, response_rx) = oneshot::channel::<HttpResponseShape>();
    let req_id = NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst);

    // Lookup server, push pending request.
    let server = {
        let reg = server_registry().lock().unwrap();
        reg.get(&server_id).cloned()
    };
    let Some(server) = server else {
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server not registered")))
            .unwrap());
    };

    let pending = PendingHttpRequest {
        id: req_id,
        method,
        url,
        headers,
        body: body_bytes,
        response_tx,
    };

    // Push to the request mpsc; the JS-side accept loop polls.
    let push_result = {
        let reg = server_senders().lock().unwrap();
        if let Some(tx) = reg.get(&server_id) {
            tx.send(pending).map_err(|_| ())
        } else {
            Err(())
        }
    };
    if push_result.is_err() {
        return Ok(Response::builder()
            .status(503)
            .body(Full::new(Bytes::from("Server closed")))
            .unwrap());
    }
    let _ = server; // keep server Arc alive for the body length of this fn

    match response_rx.await {
        Ok(shape) => {
            let mut builder = Response::builder().status(shape.status);
            for (k, v) in &shape.headers {
                builder = builder.header(k, v);
            }
            Ok(builder.body(Full::new(Bytes::from(shape.body))).unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(500)
            .body(Full::new(Bytes::from("Handler error")))
            .unwrap()),
    }
}

// Separate map for senders so PerryHttpServer can own the receiver mutably
// inside the JS-side accept op.
fn server_senders() -> &'static Mutex<HashMap<i32, mpsc::UnboundedSender<PendingHttpRequest>>> {
    static REG: OnceLock<Mutex<HashMap<i32, mpsc::UnboundedSender<PendingHttpRequest>>>> =
        OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `op_perry_http_listen(port, host)` — bind a TCP listener, spawn a
/// hyper http1 acceptor, register a new server id. Returns the server
/// id on success. Async to allow tokio bind to run on the shared runtime.
///
/// Important: deno_core's executor doesn't run inside Perry's shared
/// `TOKIO_RUNTIME`, so naked `TcpListener::bind` / `tokio::spawn` calls
/// here would panic with "no reactor running". Every tokio-touching
/// future is wrapped in the runtime handle via `_enter` or
/// `tokio_rt.spawn(...)`. See `crate::get_tokio_runtime`.
#[op2]
#[smi]
async fn op_perry_http_listen(port: i32, #[string] host: String) -> Result<i32, JsErrorBox> {
    let bind_host = if host.is_empty() {
        "0.0.0.0".to_string()
    } else {
        host
    };
    let bind_str = format!("{}:{}", bind_host, port);
    let addr: SocketAddr = bind_str
        .parse()
        .map_err(|e| JsErrorBox::generic(format!("invalid bind address {}: {}", bind_str, e)))?;

    let tokio_rt = crate::get_tokio_runtime();
    // Bind on the shared runtime so the listener is registered with its reactor.
    let listener = tokio_rt
        .spawn(async move { TcpListener::bind(addr).await })
        .await
        .map_err(|e| JsErrorBox::generic(format!("spawn bind task failed: {}", e)))?
        .map_err(|e| JsErrorBox::generic(format!("bind {} failed: {}", bind_str, e)))?;

    let (request_tx, request_rx) = mpsc::unbounded_channel::<PendingHttpRequest>();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    let server_id = NEXT_SERVER_ID.fetch_add(1, Ordering::SeqCst);
    let server = Arc::new(PerryHttpServer {
        request_rx: tokio::sync::Mutex::new(request_rx),
        shutdown_tx: Mutex::new(Some(shutdown_tx)),
        closed: Mutex::new(false),
    });
    server_registry()
        .lock()
        .unwrap()
        .insert(server_id, server.clone());
    server_senders()
        .lock()
        .unwrap()
        .insert(server_id, request_tx);

    ACTIVE_SERVERS.fetch_add(1, Ordering::SeqCst);

    tokio_rt.spawn(async move {
        let tokio_rt = crate::get_tokio_runtime();
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _peer)) => {
                            let io = TokioIo::new(stream);
                            tokio_rt.spawn(async move {
                                let service = service_fn(move |req: Request<Incoming>| async move {
                                    handle_request_for_perry_http(server_id, req).await
                                });
                                if let Err(e) = http1::Builder::new()
                                    .serve_connection(io, service)
                                    .await
                                {
                                    let _ = e; // client disconnects etc — silenced
                                }
                            });
                        }
                        Err(e) => {
                            eprintln!("[perry-http] accept error: {}", e);
                            break;
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }
        // Accept loop exit — drop senders so accept op resolves null.
        server_senders().lock().unwrap().remove(&server_id);
    });

    Ok(server_id)
}

/// `op_perry_http_accept(serverId)` — async, resolves with the next
/// pending request, or with `{ id: 0 }` when the server is closed.
#[op2]
#[serde]
async fn op_perry_http_accept(#[smi] server_id: i32) -> serde_json::Value {
    let server = {
        let reg = server_registry().lock().unwrap();
        reg.get(&server_id).cloned()
    };
    let Some(server) = server else {
        return serde_json::json!({ "id": 0 });
    };

    let pending_opt: Option<PendingHttpRequest> = {
        // tokio::sync::Mutex lets us hold the lock across `.await`.
        let mut rx_guard = server.request_rx.lock().await;
        rx_guard.recv().await
    };

    let Some(pending) = pending_opt else {
        // Server shut down.
        return serde_json::json!({ "id": 0 });
    };

    // Stash the response oneshot keyed by request id so op_respond can
    // resolve it.
    pending_response_registry()
        .lock()
        .unwrap()
        .insert(pending.id, pending.response_tx);

    let body_str = String::from_utf8_lossy(&pending.body).to_string();
    let headers_obj = serde_json::Value::Object(
        pending
            .headers
            .iter()
            .map(|(k, v)| (k.to_lowercase(), serde_json::Value::String(v.clone())))
            .collect(),
    );
    let raw_headers = serde_json::Value::Array(
        pending
            .headers
            .iter()
            .flat_map(|(k, v)| {
                vec![
                    serde_json::Value::String(k.clone()),
                    serde_json::Value::String(v.clone()),
                ]
            })
            .collect(),
    );

    serde_json::json!({
        "id": pending.id,
        "method": pending.method,
        "url": pending.url,
        "headers": headers_obj,
        "rawHeaders": raw_headers,
        "body": body_str,
    })
}

/// `op_perry_http_respond(reqId, status, headersJson, body)` — sync.
/// `headersJson` is a JSON-encoded array of [name, value] pairs (preserves
/// duplicates / order — needed for Set-Cookie etc).
#[op2(fast)]
fn op_perry_http_respond(
    #[smi] req_id: i32,
    #[smi] status: i32,
    #[string] headers_json: &str,
    #[string] body: &str,
) {
    let headers: Vec<(String, String)> =
        match serde_json::from_str::<Vec<(String, String)>>(headers_json) {
            Ok(v) => v,
            Err(_) => Vec::new(),
        };
    let tx = pending_response_registry().lock().unwrap().remove(&req_id);
    if let Some(tx) = tx {
        let shape = HttpResponseShape {
            status: status as u16,
            headers,
            body: body.as_bytes().to_vec(),
        };
        let _ = tx.send(shape);
    }
}

/// `op_perry_http_close(serverId)` — drop the shutdown channel; the
/// accept loop notices and exits. Future accept ops resolve `{id:0}`.
#[op2(fast)]
fn op_perry_http_close(#[smi] server_id: i32) {
    let server = {
        let reg = server_registry().lock().unwrap();
        reg.get(&server_id).cloned()
    };
    if let Some(server) = server {
        let already_closed = {
            let mut closed = server.closed.lock().unwrap();
            let was = *closed;
            *closed = true;
            was
        };
        // Fire shutdown.
        if let Some(tx) = server.shutdown_tx.lock().unwrap().take() {
            let _ = tx.send(());
        }
        // Drop the sender so accept resolves null after draining.
        server_senders().lock().unwrap().remove(&server_id);
        if !already_closed {
            ACTIVE_SERVERS.fetch_sub(1, Ordering::SeqCst);
        }
    }
    // Remove from registry — pending response oneshots for this server
    // will simply never fire; their hyper service fns receive Err and
    // return 500. Acceptable for shutdown.
    server_registry().lock().unwrap().remove(&server_id);
}

extension!(
    perry_ops,
    ops = [
        op_perry_log,
        op_perry_call_native,
        op_perry_print,
        op_perry_fetch,
        op_perry_http_listen,
        op_perry_http_accept,
        op_perry_http_respond,
        op_perry_http_close,
    ],
);
