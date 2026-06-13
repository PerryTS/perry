//! Raw-socket HTTP/1.1 client path used when the request asks for response
//! trailers (`TE: trailers`) — reqwest's body API drops trailer blocks, so
//! this bypass speaks HTTP/1.1 over a plain TcpStream and parses the
//! response (chunked decoding + trailer block) itself. The parser is shared
//! with the #2154 `agent.createConnection` socket path in `lib.rs`.

use std::collections::HashMap;

use perry_ffi::Handle;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{push_event, PendingHttpEvent};

fn expects_response_trailers(headers: &HashMap<String, String>) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("te")
            && value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("trailers"))
    })
}

pub(crate) async fn dispatch_plain_http_request(
    request_handle: Handle,
    method: &str,
    url: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
    timeout_ms: Option<u64>,
) -> Option<Result<(), String>> {
    if !expects_response_trailers(headers) {
        return None;
    }
    let parsed = match reqwest::Url::parse(url) {
        Ok(u) if u.scheme() == "http" => u,
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
            if k.eq_ignore_ascii_case("connection") {
                // The raw trailer-aware path reads until EOF after the final
                // chunk/trailer block. Force close here so an explicit
                // `Connection: keep-alive` cannot hang until timeout.
                continue;
            }
            req.push_str(k);
            req.push_str(": ");
            req.push_str(v);
            req.push_str("\r\n");
        }
        req.push_str("Connection: close\r\n");
        if !body.is_empty() && !has_content_length {
            req.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).await?;
        if !body.is_empty() {
            stream.write_all(body).await?;
        }

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await?;
        Ok::<Vec<u8>, std::io::Error>(raw)
    };

    let raw = match timeout_ms {
        Some(ms) => match tokio::time::timeout(std::time::Duration::from_millis(ms), fut).await {
            Ok(r) => r,
            Err(_) => return Some(Err("request timed out".to_string())),
        },
        None => match tokio::time::timeout(std::time::Duration::from_secs(30), fut).await {
            Ok(r) => r,
            Err(_) => return Some(Err("request timed out".to_string())),
        },
    };
    let raw = match raw {
        Ok(r) => r,
        Err(e) => return Some(Err(e.to_string())),
    };

    match parse_http_response(&raw) {
        Ok(parsed) => {
            push_event(PendingHttpEvent::Response {
                request_handle,
                status: parsed.status,
                status_message: parsed.status_message,
                headers: parsed.headers,
                trailers: parsed.trailers,
                body: parsed.body,
            });
            Some(Ok(()))
        }
        Err(e) => Some(Err(e)),
    }
}

/// A parsed HTTP/1.1 response message (status line + headers + decoded body
/// + trailers). Produced by [`parse_http_response`].
pub(crate) struct ParsedHttpResponse {
    pub(crate) status: u16,
    pub(crate) status_message: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) trailers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

/// Parse a raw HTTP/1.1 response (the bytes read off a socket) into status /
/// headers / decoded body / trailers. Decodes `Transfer-Encoding: chunked`
/// (including a trailer block) and honors `Content-Length`; with neither it
/// treats the remainder as the body (read-until-EOF transports). Shared by
/// the trailer-aware reqwest-bypass path ([`dispatch_plain_http_request`])
/// and the #2154 `agent.createConnection` socket path
/// ([`dispatch_request_over_socket`]).
pub(crate) fn parse_http_response(raw: &[u8]) -> Result<ParsedHttpResponse, String> {
    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("invalid HTTP response".to_string());
    };
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut status_parts = status_line.splitn(3, ' ');
    let _version = status_parts.next();
    let status = status_parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let status_message = status_parts.next().unwrap_or("").to_string();
    let mut hdrs = Vec::new();
    let mut is_chunked = false;
    let mut content_length: Option<usize> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                is_chunked = true;
            }
            if name == "content-length" {
                content_length = value.parse::<usize>().ok();
            }
            hdrs.push((name, value));
        }
    }
    let payload = &raw[header_end + 4..];
    let mut decoded = Vec::new();
    let mut trailers = Vec::new();
    if is_chunked {
        let mut pos = 0;
        while pos < payload.len() {
            let Some(line_end_rel) = payload[pos..].windows(2).position(|w| w == b"\r\n") else {
                break;
            };
            let line_end = pos + line_end_rel;
            let size_line = String::from_utf8_lossy(&payload[pos..line_end]);
            let size_hex = size_line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_hex, 16).unwrap_or(0);
            pos = line_end + 2;
            if size == 0 {
                if pos <= payload.len() {
                    let rest = &payload[pos..];
                    let trailer_end = rest
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .unwrap_or(rest.len());
                    let trailer_text = String::from_utf8_lossy(&rest[..trailer_end]);
                    for line in trailer_text.split("\r\n") {
                        if let Some((name, value)) = line.split_once(':') {
                            trailers
                                .push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
                        }
                    }
                }
                break;
            }
            if pos + size > payload.len() {
                break;
            }
            decoded.extend_from_slice(&payload[pos..pos + size]);
            pos += size + 2;
        }
    } else if let Some(len) = content_length {
        decoded.extend_from_slice(&payload[..payload.len().min(len)]);
    } else {
        decoded.extend_from_slice(payload);
    }

    Ok(ParsedHttpResponse {
        status,
        status_message,
        headers: hdrs,
        trailers,
        body: decoded,
    })
}
