//! Client raw-socket path for `Expect: 100-continue` (issue #5080).
//!
//! reqwest auto-consumes the interim `100 Continue` response, so a
//! `ClientRequest` carrying `Expect: 100-continue` never surfaces the
//! `'continue'` event through the pooled client. This module speaks
//! HTTP/1.1 over a plain `TcpStream`: it flushes the request head with the
//! body withheld, waits for the server's interim `100 Continue`, emits
//! `'continue'`, then sends the body (handed over from the deferred
//! `req.end()` through a oneshot) and parses the final response with the
//! shared [`crate::parse_http_response`].
//!
//! Plain `http://` only — an `https` 100-continue handshake would need the
//! TLS-wrapped socket path and stays on the reqwest route (no `'continue'`
//! event, matching the pre-#5080 behavior).

use std::collections::HashMap;

use perry_ffi::{spawn_blocking_with_reactor as spawn_blocking, with_handle_mut, Handle};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use bytes::Bytes;

use crate::plain_client::parse_http_response;
use crate::{push_event, ClientInflightGuard, ClientRequestHandle, PendingHttpEvent};

/// Whether the request headers ask for the `100-continue` handshake.
pub(crate) fn wants_continue(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("expect") && value.to_ascii_lowercase().contains("100-continue")
    })
}

/// Queue an `arm_expect_continue` for the next event-loop tick. Node flushes a
/// request's head on `nextTick`, not at construction, so deferring lets a
/// post-construction `setHeader(...)` (including a late `Expect:
/// 100-continue`) reach the wire before the head is snapshotted.
pub(crate) fn defer_arm(handle: Handle) {
    crate::push_event(crate::PendingHttpEvent::DeferredArmContinue {
        request_handle: handle,
    });
}

/// #5080 — if the freshly-built request carries `Expect: 100-continue`
/// (plain `http://` only), flush its head now and arm the deferred-body
/// channel. Node puts the head on the wire before `end()` for a continue
/// request and withholds the body until the server's interim `100 Continue`
/// drives the `'continue'` event. A no-op otherwise; the reqwest path keeps
/// the buffered-dispatch-at-`end()` behavior for every other request.
pub(crate) fn arm_expect_continue(handle: Handle) {
    let snapshot = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        if req.ended || req.expects_continue || !req.url.starts_with("http://") {
            return None;
        }
        if !wants_continue(&req.headers) {
            return None;
        }
        req.expects_continue = true;
        Some((
            req.method.clone(),
            req.url.clone(),
            req.headers.clone(),
            req.timeout_ms,
        ))
    })
    .flatten();
    if let Some((method, url, headers, timeout_ms)) = snapshot {
        dispatch_expect_continue(handle, method, url, headers, timeout_ms);
    }
}

/// Serialize the request head for the continue exchange. The body is
/// withheld, so frame it `Transfer-Encoding: chunked` unless the caller
/// pinned an explicit `Content-Length` / `Transfer-Encoding`; force
/// `Connection: close` (the final response is read until EOF). Drops any
/// caller-supplied `Connection` / `Host` header (`Host` is set from the URL).
fn serialize_continue_head(
    method: &str,
    path: &str,
    host_header: &str,
    headers: &HashMap<String, String>,
    use_chunked: bool,
) -> Vec<u8> {
    let mut head = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("connection") || k.eq_ignore_ascii_case("host") {
            continue;
        }
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    if use_chunked {
        head.push_str("Transfer-Encoding: chunked\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    head.into_bytes()
}

/// Flush the head of an `Expect: 100-continue` request and arm the deferred
/// body channel. Called on the main thread at request-creation time (Node
/// puts the head on the wire before `end()` for a continue request); the
/// actual exchange runs on a tokio task.
pub(crate) fn dispatch_expect_continue(
    request_handle: Handle,
    method: String,
    url: String,
    headers: HashMap<String, String>,
    timeout_ms: Option<u64>,
) {
    let parsed = match reqwest::Url::parse(&url) {
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
    let port = parsed.port_or_known_default().unwrap_or(80);
    let host_header = match parsed.port() {
        Some(p) => format!("{}:{}", host, p),
        None => host.clone(),
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }

    let use_chunked = !headers.iter().any(|(k, _)| {
        k.eq_ignore_ascii_case("content-length") || k.eq_ignore_ascii_case("transfer-encoding")
    });
    let head = serialize_continue_head(&method, &path, &host_header, &headers, use_chunked);

    // Hand-off for the withheld body: `req.end()` sends the buffered body
    // here once the user's `'continue'` handler (or any later end()) runs.
    let (body_tx, body_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    with_handle_mut::<ClientRequestHandle, _, _>(request_handle, |req| {
        req.continue_body_tx = Some(body_tx);
    });

    let deadline = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));

    spawn_blocking(move || {
        // Defeat LTO dead-stripping of tokio's CONTEXT statics — same
        // workaround dispatch_request needs (see spawn_socket_runner).
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
        // #5892 remainder: same in-flight guard as `dispatch_request` — the
        // continue exchange must stay visible to the exit gate for its whole
        // lifetime, not just until the outer spawn closure returns.
        let inflight_guard = ClientInflightGuard::new();
        let jh = handle.spawn(async move {
            let _inflight = inflight_guard;
            if let Err(error_message) = run_exchange(
                request_handle,
                host,
                port,
                head,
                use_chunked,
                body_rx,
                deadline,
            )
            .await
            {
                push_event(PendingHttpEvent::Error {
                    request_handle,
                    error_message,
                });
            }
        });
        std::hint::black_box(&jh);
        std::mem::forget(jh);
    });
}

/// Drive the continue exchange: write the head, observe the interim
/// `100 Continue`, emit `'continue'`, send the deferred body, then read +
/// parse the final response.
async fn run_exchange(
    request_handle: Handle,
    host: String,
    port: u16,
    head: Vec<u8>,
    use_chunked: bool,
    body_rx: tokio::sync::oneshot::Receiver<Vec<u8>>,
    deadline: std::time::Duration,
) -> Result<(), String> {
    let mut stream = tokio::time::timeout(
        deadline,
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| "request timed out".to_string())?
    .map_err(|e| e.to_string())?;
    write_all(&mut stream, &head, deadline).await?;

    // Read until the first complete header block. A 1xx status (e.g.
    // `100 Continue`) drives the `'continue'` event; a final (>=200)
    // response means the server declined the handshake — surface it as-is.
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8 * 1024];
    let mut got_interim = false;
    loop {
        if let Some(pos) = find_header_end(&buf) {
            let status = parse_status_code(&buf[..pos]);
            if (100..200).contains(&status) {
                got_interim = true;
                buf.drain(..pos + 4);
                break;
            } else if status >= 200 {
                // Final response, no continue — leave `buf` intact for the
                // EOF read below to finish + parse.
                break;
            }
        }
        let n = read_chunk(&mut stream, &mut chunk, deadline).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    if got_interim {
        push_event(PendingHttpEvent::Continue { request_handle });
        // Wait for the deferred body handed over by `req.end()`. A dropped
        // sender (request torn down) resolves to an empty body.
        let body = tokio::time::timeout(deadline, body_rx)
            .await
            .map_err(|_| "request timed out".to_string())?
            .unwrap_or_default();
        let framed = if use_chunked {
            frame_chunked(&body)
        } else {
            body
        };
        write_all(&mut stream, &framed, deadline).await?;
    }

    // Read the rest of the (final) response to EOF — the head forces
    // `Connection: close`, so the peer closes once it's done.
    loop {
        let n = read_chunk(&mut stream, &mut chunk, deadline).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let final_bytes = strip_interim_blocks(buf);
    let parsed = parse_http_response(&final_bytes)?;
    // Deliver via the same streaming path the pooled reqwest client uses
    // (`ResponseHead` → `ResponseChunk` → `ResponseEnd`): the head fires the
    // `(res) => …` callback / `'response'` listeners, then the body + end
    // edges drain on later ticks. This matches the normal client's
    // observable ordering and reuses its well-exercised delivery helpers.
    push_event(PendingHttpEvent::ResponseHead {
        request_handle,
        status: parsed.status,
        status_message: parsed.status_message,
        headers: parsed.headers,
    });
    if !parsed.body.is_empty() {
        push_event(PendingHttpEvent::ResponseChunk {
            request_handle,
            chunk: Bytes::from(parsed.body),
        });
    }
    push_event(PendingHttpEvent::ResponseEnd { request_handle });
    Ok(())
}

/// One deadline-bounded `read`, mapping a timeout / IO error to a string.
async fn read_chunk(
    stream: &mut tokio::net::TcpStream,
    chunk: &mut [u8],
    deadline: std::time::Duration,
) -> Result<usize, String> {
    tokio::time::timeout(deadline, stream.read(chunk))
        .await
        .map_err(|_| "request timed out".to_string())?
        .map_err(|e| e.to_string())
}

/// Deadline-bounded `write_all` so a stalled peer can't hang the exchange
/// even when the request set a `timeout`.
async fn write_all(
    stream: &mut tokio::net::TcpStream,
    bytes: &[u8],
    deadline: std::time::Duration,
) -> Result<(), String> {
    tokio::time::timeout(deadline, stream.write_all(bytes))
        .await
        .map_err(|_| "request timed out".to_string())?
        .map_err(|e| e.to_string())
}

/// Frame `body` as a single HTTP/1.1 chunk plus the terminating chunk.
fn frame_chunked(body: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(body.len() + 16);
    if !body.is_empty() {
        framed.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
        framed.extend_from_slice(body);
        framed.extend_from_slice(b"\r\n");
    }
    framed.extend_from_slice(b"0\r\n\r\n");
    framed
}

/// Offset of the `\r\n\r\n` that terminates the header block, if present.
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Parse the numeric status code out of a status line (`HTTP/1.1 100 ...`).
fn parse_status_code(head: &[u8]) -> u16 {
    let text = String::from_utf8_lossy(head);
    text.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0)
}

/// Drop any leading interim (1xx) header blocks so the remainder begins at
/// the final response. Defensive: the read loop already strips the interim
/// `100 Continue` before sending the body, but a server may emit more than
/// one informational response.
fn strip_interim_blocks(mut buf: Vec<u8>) -> Vec<u8> {
    loop {
        let Some(pos) = find_header_end(&buf) else {
            return buf;
        };
        if (100..200).contains(&parse_status_code(&buf[..pos])) {
            buf.drain(..pos + 4);
        } else {
            return buf;
        }
    }
}
