//! HTTP Server implementation
//!
//! Uses hyper for high-performance HTTP serving.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::common::{get_handle, register_handle, Handle, RUNTIME};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// Request ID counter
static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Pending request waiting for a response
pub struct PendingRequest {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub response_tx: tokio::sync::oneshot::Sender<HttpResponse>,
}

/// HTTP response to send back
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// HTTP Server handle
pub struct HttpServerHandle {
    pub port: u16,
    pub request_rx: Arc<tokio::sync::Mutex<mpsc::Receiver<PendingRequest>>>,
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Request handle for TypeScript access
pub struct RequestHandle {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub response_tx: Option<tokio::sync::oneshot::Sender<HttpResponse>>,
}

/// Create a new HTTP server
///
/// Returns a server handle that can accept connections.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_create(port: f64) -> Handle {
    let port = port as u16;
    let (request_tx, request_rx) = mpsc::channel::<PendingRequest>(1024);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let request_tx = Arc::new(request_tx);
    let request_rx = Arc::new(tokio::sync::Mutex::new(request_rx));

    // Spawn the server task
    let request_tx_clone = request_tx.clone();
    RUNTIME.spawn(async move {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));

        let listener = match TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind to port {}: {}", port, e);
                return;
            }
        };

        println!("Server listening on http://0.0.0.0:{}", port);

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let request_tx = request_tx_clone.clone();

                            tokio::spawn(async move {
                                let service = service_fn(move |req: Request<Incoming>| {
                                    let request_tx = request_tx.clone();
                                    async move {
                                        handle_request(req, request_tx).await
                                    }
                                });

                                if let Err(e) = http1::Builder::new()
                                    .serve_connection(io, service)
                                    .await
                                {
                                    eprintln!("Connection error: {}", e);
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

    register_handle(HttpServerHandle {
        port,
        request_rx,
        shutdown_tx: Some(shutdown_tx),
    })
}

/// Handle an incoming HTTP request
async fn handle_request(
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<PendingRequest>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);

    // Extract request details
    let method = req.method().to_string();
    let uri = req.uri();
    let path = uri.path().to_string();

    // Extract headers
    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(name.to_string(), v.to_string());
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

    // Create oneshot channel for response
    let (response_tx, response_rx) = tokio::sync::oneshot::channel::<HttpResponse>();

    // Send request to TypeScript handler
    let pending = PendingRequest {
        id,
        method,
        path,
        headers,
        body,
        response_tx,
    };

    if request_tx.send(pending).await.is_err() {
        // Channel closed, return 503
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::from("Server unavailable")))
            .unwrap());
    }

    // Wait for response from TypeScript handler
    match response_rx.await {
        Ok(http_response) => {
            let mut response = Response::builder()
                .status(StatusCode::from_u16(http_response.status).unwrap_or(StatusCode::OK));

            for (name, value) in http_response.headers {
                response = response.header(name, value);
            }

            Ok(response
                .body(Full::new(Bytes::from(http_response.body)))
                .unwrap())
        }
        Err(_) => {
            // Handler dropped without responding
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Full::new(Bytes::from("Handler error")))
                .unwrap())
        }
    }
}

/// Accept the next request (blocking)
///
/// Returns a request handle, or -1 if no request available.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_accept(server_handle: Handle) -> Handle {
    if let Some(server) = get_handle::<HttpServerHandle>(server_handle) {
        let request_rx = server.request_rx.clone();

        // Block on receiving the next request
        let result = RUNTIME.block_on(async {
            let mut rx = request_rx.lock().await;
            rx.recv().await
        });

        if let Some(pending) = result {
            // Parse query string from path
            let (path, query) = match pending.path.split_once('?') {
                Some((p, q)) => (p.to_string(), q.to_string()),
                None => (pending.path.clone(), String::new()),
            };

            return register_handle(RequestHandle {
                id: pending.id,
                method: pending.method,
                path,
                query,
                headers: pending.headers,
                body: pending.body,
                response_tx: Some(pending.response_tx),
            });
        }
    }
    -1
}

/// Get request method
#[no_mangle]
pub unsafe extern "C" fn js_http_request_method(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.method.as_ptr(), req.method.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request path
#[no_mangle]
pub unsafe extern "C" fn js_http_request_path(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.path.as_ptr(), req.path.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request query string
#[no_mangle]
pub unsafe extern "C" fn js_http_request_query(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.query.as_ptr(), req.query.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request header by name
#[no_mangle]
pub unsafe extern "C" fn js_http_request_header(
    req_handle: Handle,
    name_ptr: *const StringHeader,
) -> *mut StringHeader {
    let name = match string_from_header(name_ptr) {
        Some(n) => n.to_lowercase(),
        None => return std::ptr::null_mut(),
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(value) = req.headers.get(&name) {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get request body as string
#[no_mangle]
pub unsafe extern "C" fn js_http_request_body(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(ref body) = req.body {
            return js_string_from_bytes(body.as_ptr(), body.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Send response to a request
#[no_mangle]
pub unsafe extern "C" fn js_http_respond(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
    content_type_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_default();
    let content_type =
        string_from_header(content_type_ptr).unwrap_or_else(|| "text/plain".to_string());

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        // Take the response channel (can only respond once)
        // Note: This is a limitation of our handle system - we can't mutably borrow
        // For now, we'll work around by storing response_tx as Option
        // In a real impl, we'd use a different pattern

        // Create response
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), content_type);

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        // We need to take ownership of response_tx
        // This is tricky with our handle system...
        // For now, let's use a different approach with a global response map

        // Actually, let's restructure - store pending responses in a global map
        // and look up by request ID
        if let Some(tx) = PENDING_RESPONSES.remove(&req.id) {
            let _ = tx.1.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

// Global map of pending responses
use dashmap::DashMap;
use once_cell::sync::Lazy;

pub static PENDING_RESPONSES: Lazy<DashMap<u64, tokio::sync::oneshot::Sender<HttpResponse>>> =
    Lazy::new(|| DashMap::new());

/// Modified accept that stores response channel in global map
#[no_mangle]
pub unsafe extern "C" fn js_http_server_accept_v2(server_handle: Handle) -> Handle {
    if let Some(server) = get_handle::<HttpServerHandle>(server_handle) {
        let request_rx = server.request_rx.clone();

        let result = RUNTIME.block_on(async {
            let mut rx = request_rx.lock().await;
            rx.recv().await
        });

        if let Some(pending) = result {
            let (path, query) = match pending.path.split_once('?') {
                Some((p, q)) => (p.to_string(), q.to_string()),
                None => (pending.path.clone(), String::new()),
            };

            let id = pending.id;

            // Store response channel in global map
            PENDING_RESPONSES.insert(id, pending.response_tx);

            return register_handle(RequestHandle {
                id,
                method: pending.method,
                path,
                query,
                headers: pending.headers,
                body: pending.body,
                response_tx: None, // Stored in global map instead
            });
        }
    }
    -1
}

/// Shutdown the server
#[no_mangle]
pub unsafe extern "C" fn js_http_server_close(server_handle: Handle) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let Some(_server) = get_handle::<HttpServerHandle>(server_handle) {
        // Note: Can't take ownership from handle, but we can drop it
        // The shutdown channel will be dropped when server handle is freed
        return f64::from_bits(TAG_TRUE);
    }
    f64::from_bits(TAG_FALSE)
}
