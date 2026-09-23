//! #10468 — client-side protocol upgrade (`Connection: Upgrade`). A `101
//! Switching Protocols` response hands the caller the raw socket through
//! `req.on('upgrade', (res, socket, head) => ...)` instead of an ordinary
//! `'response'`. reqwest consumes the connection as a normal response body
//! and never exposes it, so an upgrade request speaks HTTP/1.1 over a raw
//! `TcpStream` instead — the same shape as the trailer-aware bypass in
//! `plain_client.rs` — and, on a `101`, adopts the stream into
//! `perry_ext_net` as a `net.Socket` (mirrors the server's
//! `server/raw_upgrade.rs`).
//!
//! Scope: plain `http://` only — TLS upgrade needs a different transport
//! and falls through to the normal path (pre-#10468 behavior: no upgrade),
//! same as when this module isn't triggered at all (no `Connection:
//! Upgrade`, or an Agent/`createConnection` override already claimed the
//! connection before `dispatch_request` runs).

use std::collections::HashMap;

use perry_ffi::Handle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{push_event, PendingHttpEvent};

/// `true` if `headers` asks for a protocol upgrade — `Connection: Upgrade`
/// as one token of a comma list (RFC 7230 §6.1; Node/undici send it as a
/// bare `Upgrade` value in practice).
pub(crate) fn wants_upgrade(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("connection")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("upgrade"))
    })
}

/// Speak the request over a raw `TcpStream` when it wants a protocol
/// upgrade. `None` means "not applicable" (not an upgrade request, or
/// `https://` — fall through to the normal reqwest path); `Some(Ok(()))`
/// once the exchange has been fully handed off to a `PendingHttpEvent`
/// (`Upgrade` on `101`, `Response` otherwise); `Some(Err(_))` on a
/// transport failure. Mirrors `plain_client::dispatch_plain_http_request`'s
/// bypass contract.
pub(crate) async fn dispatch_upgrade_http_request(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    timeout_ms: Option<u64>,
) -> Option<Result<(), String>> {
    if !wants_upgrade(headers) {
        return None;
    }
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) if u.scheme() == "http" => u,
        // https:// upgrade isn't implemented — let the caller fall through
        // rather than mishandle it here (matches pre-#10468 behavior for TLS).
        _ => return None,
    };
    let host = match parsed.host_str() {
        Some(h) => h.to_string(),
        None => return Some(Err("missing host".to_string())),
    };
    let port = parsed.port_or_known_default().unwrap_or(80);
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(q) = parsed.query() {
        path.push('?');
        path.push_str(q);
    }

    let deadline = std::time::Duration::from_millis(timeout_ms.unwrap_or(30_000));
    let fut = async {
        let mut stream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
        let host_header = if parsed.port().is_some() {
            format!("{}:{}", host, port)
        } else {
            host.clone()
        };
        let mut req = format!("{} {} HTTP/1.1\r\nHost: {}\r\n", method, path, host_header);
        let mut has_content_length = false;
        for (k, v) in headers {
            if k.eq_ignore_ascii_case("content-length") {
                has_content_length = true;
            }
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        if !body.is_empty() && !has_content_length {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).await?;
        if !body.is_empty() {
            stream.write_all(body).await?;
        }

        // Read only up to the end of the header block — a `101` keeps the
        // connection open for the upgraded protocol, so (unlike
        // `plain_client`'s trailer-aware bypass) this must not read to EOF.
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok::<_, std::io::Error>((stream, buf))
    };

    let (stream, buf) = match tokio::time::timeout(deadline, fut).await {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => return Some(Err(e.to_string())),
        Err(_) => return Some(Err("request timed out".to_string())),
    };

    let Some(header_end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Some(Err(
            "invalid HTTP response (no header terminator)".to_string()
        ));
    };
    let head_text = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = head_text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.splitn(3, ' ');
    let http_version = parts
        .next()
        .and_then(|v| v.strip_prefix("HTTP/"))
        .and_then(|v| v.split_once('.'))
        .and_then(|(maj, min)| Some((maj.parse::<u8>().ok()?, min.parse::<u8>().ok()?)))
        .unwrap_or((1, 1));
    let status: u16 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let status_message = parts.next().unwrap_or("").to_string();
    let mut hdrs = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            hdrs.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let rest = buf[header_end + 4..].to_vec();

    if status == 101 {
        // perry-ext-net takes the connection as a plain `std` stream and hands
        // it to the agent's turnloop loop; tokio only has to let go of it.
        let socket_id = stream
            .into_std()
            .map_or(perry_ffi::INVALID_HANDLE, perry_ext_net::adopt_upgraded_tcp_stream);
        push_event(PendingHttpEvent::Upgrade {
            request_handle,
            status,
            status_message,
            headers: hdrs,
            socket_handle: socket_id,
            head: rest,
        });
        return Some(Ok(()));
    }

    // Server declined the upgrade — deliver an ordinary `'response'`. Read
    // the remainder to EOF like the trailer-aware bypass (a non-101 reply
    // to an Upgrade request has no further framing guarantee here).
    let mut stream = stream;
    let mut full = rest;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => full.extend_from_slice(&chunk[..n]),
        }
    }
    push_event(PendingHttpEvent::Response {
        request_handle,
        status,
        status_message,
        headers: hdrs,
        trailers: Vec::new(),
        body: full,
        http_version,
    });
    Some(Ok(()))
}
