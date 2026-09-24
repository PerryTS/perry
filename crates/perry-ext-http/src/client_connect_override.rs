//! Client requests routed over a caller-supplied raw socket instead of
//! the default transport: both `agent.createConnection`/`agent.createSocket` (#2154) and
//! the request option's own `createConnection` (#10469, honored only when
//! `agent_handle == 0`) end up here. Split out of `lib.rs` to stay under
//! the file-size cap; the closure storage/invocation and the `{ host, port,
//! path, keepAlive, keepAliveInitialDelay }` options object still live in
//! `agent.rs` alongside the pre-existing Agent-level override.

use std::collections::HashMap;

use perry_ffi::{spawn_blocking_with_reactor as spawn_blocking, Handle};

use super::agent;
use crate::{parse_http_response, push_event, ClientInflightGuard, PendingHttpEvent};

/// Look up `request_handle`'s own `createConnection` (if any) and, when
/// set, dispatch over it. `None` means "not set / not usable" — the
/// caller (only reached when `agent_handle == 0`) falls back to the default
/// transport.
pub(crate) fn dispatch_for_handle(request_handle: Handle, url: &str) -> Option<i64> {
    let cc = perry_ffi::with_handle_mut::<crate::ClientRequestHandle, _, _>(request_handle, |r| {
        r.request_create_connection
    })
    .unwrap_or(0);
    if cc == 0 {
        return None;
    }
    request_create_connection_socket(cc, url)
}

/// The whole "no explicit Agent, but the request's own `createConnection`
/// is set" path: resolve `(host, port, path)` from `url`, invoke the
/// override on the main thread, and attach raw mode on the socket it
/// returns (so no inbound byte gets dispatched as a JS `'data'` event
/// before `dispatch_request_over_socket`'s task takes over — mirrors the
/// Agent-override path in `dispatch_request_snapshot`). `None` means "not
/// handled", so the caller falls back to the default transport.
pub(crate) fn request_create_connection_socket(
    request_create_connection: i64,
    url: &str,
) -> Option<i64> {
    let (host, port, path) = super::socket_connect_target(url)?;
    let socket_id = unsafe {
        agent::try_request_create_connection_socket(request_create_connection, &host, port, &path)
    }?;
    if let Some(vt) = perry_ffi::raw_net() {
        (vt.attach)(socket_id);
    }
    Some(socket_id)
}

/// Serialize an HTTP/1.1 request (request line + headers + body) into the
/// bytes to write onto a socket. Ordinary responses force `Connection: close`
/// because this path reads until EOF. Upgrade requests preserve the caller's
/// `Connection: Upgrade` header so a `101` can hand the live socket back to
/// JavaScript. `Host` is always derived from the URL.
fn serialize_http_request(
    method: &str,
    path: &str,
    host_header: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Vec<u8> {
    let wants_upgrade = crate::client_upgrade::wants_upgrade(headers);
    let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
    let mut has_content_length = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if k.eq_ignore_ascii_case("host")
            || (k.eq_ignore_ascii_case("connection") && !wants_upgrade)
        {
            continue;
        }
        req.push_str(k);
        req.push_str(": ");
        req.push_str(v);
        req.push_str("\r\n");
    }
    if !wants_upgrade {
        req.push_str("Connection: close\r\n");
    }
    if !body.is_empty() && !has_content_length {
        req.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    req.push_str("\r\n");
    let mut out = req.into_bytes();
    out.extend_from_slice(body);
    out
}

/// #2154 — run an HTTP exchange over a socket that a `createConnection`
/// override (Agent-level or, since #10469, request-level) produced
/// (`socket_id`), instead of the default transport. Ordinary responses force
/// `Connection: close` and read to EOF. A `101` response to an upgrade request
/// detaches the still-live socket from the raw reader and pushes `Upgrade` with
/// any bytes following the header block. Other responses are parsed with
/// [`parse_http_response`] and produce the same `Response` / `Error` events as
/// the default transport.
///
/// The socket I/O goes through perry-ffi's raw-net vtable (published by
/// perry-ext-net), so this crate needs no link edge to perry-ext-net. If no
/// net backend is linked the request errors out (the override couldn't have
/// produced a socket without `net`, so this is a defensive guard).
pub(crate) fn dispatch_request_over_socket(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
    timeout_ms: Option<u64>,
    socket_id: i64,
) {
    let parsed = match url::Url::parse(&url) {
        Ok(u) => u,
        Err(e) => {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: e.to_string(),
            });
            return;
        }
    };
    let host = parsed.host_str().unwrap_or("localhost").to_string();
    let host_header = match parsed.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host,
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }
    let req_bytes = serialize_http_request(&method, &path, &host_header, &headers, &body);
    let wants_upgrade = crate::client_upgrade::wants_upgrade(&headers);
    let deadline = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));

    spawn_blocking(move || {
        let try_h = tokio::runtime::Handle::try_current();
        std::hint::black_box(&try_h);
        if try_h.is_err() {
            push_event(PendingHttpEvent::Error {
                request_handle,
                error_message: "http client runtime unavailable".to_string(),
            });
            return;
        }
        let handle = tokio::runtime::Handle::current();
        // #5779 follow-up: keep this fetch counted in-flight for its whole
        // lifetime so the idle-kick recovers a lost worker-unpark.
        let inflight_guard = ClientInflightGuard::new(request_handle);
        let jh = handle.spawn(async move {
            let _inflight = inflight_guard;
            let vtable = match perry_ffi::raw_net() {
                Some(v) => v,
                None => {
                    push_event(PendingHttpEvent::Error {
                        request_handle,
                        error_message: "agent.createConnection requires node:net (not linked)"
                            .to_string(),
                    });
                    return;
                }
            };
            // Attach is idempotent — the request path also attaches on the
            // main thread before this task runs, to close any data race.
            (vtable.attach)(socket_id);
            if (vtable.write)(socket_id, req_bytes.as_ptr(), req_bytes.len()) == 0 {
                push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message: "failed to write request to agent socket".to_string(),
                });
                return;
            }

            let mut raw = Vec::new();
            let mut chunk = [0u8; 16 * 1024];
            let start = tokio::time::Instant::now();
            loop {
                let n = (vtable.poll_read)(socket_id, chunk.as_mut_ptr(), chunk.len());
                if n > 0 {
                    raw.extend_from_slice(&chunk[..n as usize]);
                    if wants_upgrade {
                        if let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header_end = header_end + 4;
                            let status = std::str::from_utf8(&raw[..header_end])
                                .ok()
                                .and_then(|head| head.lines().next())
                                .and_then(|line| line.split_whitespace().nth(1))
                                .and_then(|code| code.parse::<u16>().ok());
                            if status == Some(101) {
                                match parse_http_response(&raw[..header_end]) {
                                    Ok(parsed) => {
                                        (vtable.detach)(socket_id);
                                        push_event(PendingHttpEvent::Upgrade {
                                            request_handle,
                                            status: parsed.status,
                                            status_message: parsed.status_message,
                                            headers: parsed.headers,
                                            socket_handle: socket_id,
                                            head: raw[header_end..].to_vec(),
                                        });
                                    }
                                    Err(error_message) => {
                                        (vtable.close)(socket_id);
                                        push_event(PendingHttpEvent::Error {
                                            request_handle,
                                            error_message,
                                        });
                                    }
                                }
                                return;
                            }
                        }
                    }
                } else if n == 0 {
                    break; // clean EOF — peer closed after the response
                } else {
                    if start.elapsed() >= deadline {
                        (vtable.close)(socket_id);
                        push_event(PendingHttpEvent::Timeout { request_handle });
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
            (vtable.close)(socket_id);

            match parse_http_response(&raw) {
                Ok(parsed) => push_event(PendingHttpEvent::Response {
                    request_handle,
                    status: parsed.status,
                    status_message: parsed.status_message,
                    headers: parsed.headers,
                    trailers: parsed.trailers,
                    body: parsed.body,
                    http_version: parsed.http_version,
                }),
                Err(error_message) => push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message,
                }),
            }
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}

#[cfg(test)]
mod tests {
    use super::serialize_http_request;
    use std::collections::HashMap;

    #[test]
    fn websocket_upgrade_keeps_connection_header() {
        let headers = HashMap::from([
            ("Connection".to_string(), "Upgrade".to_string()),
            ("Upgrade".to_string(), "websocket".to_string()),
        ]);
        let request = String::from_utf8(serialize_http_request(
            "GET",
            "/socket",
            "localhost:1234",
            &headers,
            &[],
        ))
        .unwrap();
        assert!(request.contains("Connection: Upgrade\r\n"), "{request}");
        assert!(!request.contains("Connection: close\r\n"), "{request}");
    }
}
