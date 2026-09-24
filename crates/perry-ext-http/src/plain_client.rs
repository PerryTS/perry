//! The raw HTTP/1.1 response parser shared by the `agent.createConnection`
//! socket paths (`client_connect_override.rs`, #2154): status line, headers,
//! decoded body and trailers from the bytes read off a JS-produced socket.
//!
//! It used to also carry a `TE: trailers` bypass over a tokio `TcpStream`,
//! because reqwest's body API dropped trailer blocks. That exchange now runs in
//! `client_turnloop` (`Mode::Trailers`), whose codec hands trailers back as
//! `Event::Trailers`.

/// A parsed HTTP/1.1 response message (status line + headers + decoded body
/// + trailers). Produced by [`parse_http_response`].
pub(crate) struct ParsedHttpResponse {
    pub(crate) status: u16,
    pub(crate) status_message: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) trailers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// `(major, minor)` parsed from the status line (`HTTP/1.1 200 OK`).
    /// Falls back to `(1, 1)` on anything that doesn't parse as
    /// `HTTP/<major>.<minor>` (#10467).
    pub(crate) http_version: (u8, u8),
}

/// Parse a raw HTTP/1.1 response (the bytes read off a socket) into status /
/// headers / decoded body / trailers. Decodes `Transfer-Encoding: chunked`
/// (including a trailer block) and honors `Content-Length`; with neither it
/// treats the remainder as the body (read-until-EOF transports). Used by the
/// #2154 `agent.createConnection` socket path
/// (`client_connect_override::dispatch_request_over_socket`).
pub(crate) fn parse_http_response(raw: &[u8]) -> Result<ParsedHttpResponse, String> {
    let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Err("invalid HTTP response".to_string());
    };
    let head = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut status_parts = status_line.splitn(3, ' ');
    let http_version = status_parts
        .next()
        .and_then(|v| v.strip_prefix("HTTP/"))
        .and_then(|v| v.split_once('.'))
        .and_then(|(maj, min)| Some((maj.parse::<u8>().ok()?, min.parse::<u8>().ok()?)))
        .unwrap_or((1, 1));
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
        http_version,
    })
}
