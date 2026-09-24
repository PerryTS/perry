//! What this lane puts on the wire, and the one thing it reads off it by hand.
//!
//! Requests are serialized here rather than through `turnloop_http`'s
//! `Encoder`, for two reasons the codec cannot accommodate:
//!
//! * **`node:http` is not Fetch.** `client::Request::head` drops a caller's
//!   `Host` and substitutes the URL authority, refuses URLs with credentials
//!   and the `CONNECT`/`TRACE`/`TRACK` methods, and `Header::new` lowercases
//!   every name. Node sends what the caller set, in the caller's case.
//! * **The three raw-socket bypasses this lane absorbs each had their own
//!   head** (`TE: trailers`, `Expect: 100-continue`, `Connection: Upgrade`);
//!   their observable framing — `Connection: close`, a chunked continue body —
//!   is kept, not re-derived.
//!
//! The *response* side stays entirely on the codec (`http1::Decoder`). The
//! only thing read by hand is the reason phrase, which `http1::Head` does not
//! carry: it is lifted from the status line the decoder just consumed, so
//! `res.statusMessage` is the server's text, as in Node — not the canonical
//! phrase reqwest substituted.

use std::collections::HashMap;

use super::Mode;

/// How the body follows the head.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Framing {
    /// Raw bytes after the head: no body, or a `Content-Length` the caller
    /// set or this module added.
    Raw,
    /// `Transfer-Encoding: chunked` — the caller asked for it, or the body is
    /// not known when the head goes out (`Expect: 100-continue`).
    Chunked,
}

/// A serialized request head plus how its body must be framed.
pub(super) struct Serialized {
    pub(super) head: Vec<u8>,
    pub(super) framing: Framing,
    /// The request itself asked the server to close (`Connection: close`,
    /// sent by the caller or forced by the mode). Such a connection is never
    /// returned to the pool whatever the response says.
    pub(super) closes: bool,
}

fn has_header(headers: &HashMap<String, String>, name: &str) -> bool {
    headers.keys().any(|k| k.eq_ignore_ascii_case(name))
}

fn header_token(headers: &HashMap<String, String>, name: &str, token: &str) -> bool {
    headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case(name)
            && v.split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
    })
}

/// The request-target: origin-form, or absolute-form through an HTTP proxy.
pub(super) fn request_target(url: &url::Url, absolute: bool) -> String {
    if absolute {
        let mut url = url.clone();
        url.set_fragment(None);
        return url.as_str().to_owned();
    }
    let mut target = url.path().to_string();
    if target.is_empty() {
        target.push('/');
    }
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    target
}

/// `Host`'s value when the caller did not set one: the URL authority, with the
/// port only when it is not the scheme default — what reqwest (hyper) and
/// Node both send.
pub(super) fn authority(url: &url::Url) -> String {
    let mut out = url.host_str().unwrap_or("").to_string();
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out
}

/// Serialize a request head.
///
/// `extra` are headers this lane adds that the caller did not set (the
/// in-process HTTPS server's forwarding token, a proxy's
/// `Proxy-Authorization`); they follow the caller's own.
pub(super) fn serialize_head(
    method: &str,
    target: &str,
    url: &url::Url,
    headers: &HashMap<String, String>,
    extra: &[(String, String)],
    body_len: usize,
    mode: Mode,
) -> Serialized {
    let mut out = String::with_capacity(256);
    out.push_str(method);
    out.push(' ');
    out.push_str(target);
    out.push_str(" HTTP/1.1\r\n");

    // Host first, as Node writes it. A caller's own `Host` wins — reqwest sent
    // it verbatim, and so does Node.
    let caller_host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.clone());
    out.push_str("Host: ");
    out.push_str(&caller_host.unwrap_or_else(|| authority(url)));
    out.push_str("\r\n");

    // `TE: trailers` and `Expect: 100-continue` read the response to its end
    // on a connection nobody reuses, so they force `Connection: close` exactly
    // as their raw-socket bypasses did. An upgrade carries the caller's own
    // `Connection: Upgrade`.
    let forces_close = matches!(mode, Mode::Trailers | Mode::Continue);
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("host")
            || (forces_close && name.eq_ignore_ascii_case("connection"))
        {
            continue;
        }
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    for (name, value) in extra {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }

    let caller_length = has_header(headers, "content-length");
    let caller_chunked = header_token(headers, "transfer-encoding", "chunked");
    let framing = if caller_length {
        Framing::Raw
    } else if caller_chunked {
        Framing::Chunked
    } else if mode == Mode::Continue {
        // The body is withheld until the interim `100 Continue`, so its length
        // is not known when the head goes out.
        out.push_str("Transfer-Encoding: chunked\r\n");
        Framing::Chunked
    } else {
        if body_len > 0 {
            out.push_str(&format!("Content-Length: {body_len}\r\n"));
        }
        Framing::Raw
    };

    let closes = if forces_close {
        out.push_str("Connection: close\r\n");
        true
    } else if mode == Mode::Upgrade || has_header(headers, "connection") {
        header_token(headers, "connection", "close")
    } else {
        // Node's default agent is keep-alive (v19+) and says so explicitly;
        // servers reading `req.headers.connection` expect it. Unchanged from
        // the reqwest path.
        out.push_str("Connection: keep-alive\r\n");
        false
    };
    out.push_str("\r\n");
    Serialized {
        head: out.into_bytes(),
        framing,
        closes,
    }
}

/// Frame a complete body for the wire.
pub(super) fn frame_body(body: &[u8], framing: Framing) -> Vec<u8> {
    match framing {
        Framing::Raw => body.to_vec(),
        Framing::Chunked => {
            let mut framed = Vec::with_capacity(body.len() + 16);
            if !body.is_empty() {
                framed.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
                framed.extend_from_slice(body);
                framed.extend_from_slice(b"\r\n");
            }
            framed.extend_from_slice(b"0\r\n\r\n");
            framed
        }
    }
}

/// The reason phrase of the status line at the start of `head` (the bytes the
/// decoder just consumed for a `Head`/`Informational` event). Empty when the
/// server sent none — which is also what Node reports.
pub(super) fn reason_phrase(head: &[u8]) -> String {
    let line_end = head
        .windows(2)
        .position(|w| w == b"\r\n")
        .unwrap_or(head.len());
    let line = String::from_utf8_lossy(&head[..line_end]);
    let mut parts = line.splitn(3, ' ');
    let _version = parts.next();
    let _status = parts.next();
    parts.next().unwrap_or("").to_string()
}

/// `Basic` credentials for a proxy URL that carries them, percent-decoded.
pub(super) fn basic_credentials(proxy: &url::Url) -> Option<String> {
    use perry_base64::Engine;
    if proxy.username().is_empty() && proxy.password().is_none() {
        return None;
    }
    let decode = |s: &str| -> Vec<u8> {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 3 <= bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(value) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(value);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        out
    };
    let mut credential = decode(proxy.username());
    credential.push(b':');
    credential.extend(decode(proxy.password().unwrap_or("")));
    Some(format!(
        "Basic {}",
        perry_base64::engine::general_purpose::STANDARD.encode(credential)
    ))
}

/// The `CONNECT` head that opens a tunnel through an HTTP proxy.
pub(super) fn connect_head(target_host: &str, target_port: u16, proxy: &url::Url) -> Vec<u8> {
    let authority = if target_host.contains(':') {
        format!("[{target_host}]:{target_port}")
    } else {
        format!("{target_host}:{target_port}")
    };
    let mut out = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(credentials) = basic_credentials(proxy) {
        out.push_str("Proxy-Authorization: ");
        out.push_str(&credentials);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    fn text(s: &Serialized) -> String {
        String::from_utf8(s.head.clone()).unwrap()
    }

    fn url(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn a_body_gets_a_content_length_and_the_default_keep_alive() {
        let u = url("http://example.invalid:8080/p?q=1");
        let s = serialize_head(
            "POST",
            &request_target(&u, false),
            &u,
            &headers(&[("Content-Type", "text/plain")]),
            &[],
            5,
            Mode::Normal,
        );
        let t = text(&s);
        assert!(t.starts_with("POST /p?q=1 HTTP/1.1\r\nHost: example.invalid:8080\r\n"));
        assert!(t.contains("Content-Type: text/plain\r\n"), "{t}");
        assert!(t.contains("Content-Length: 5\r\n"), "{t}");
        assert!(t.ends_with("Connection: keep-alive\r\n\r\n"), "{t}");
        assert_eq!(s.framing, Framing::Raw);
        assert!(!s.closes);
    }

    #[test]
    fn a_callers_host_and_header_case_reach_the_wire_verbatim() {
        let u = url("http://127.0.0.1:1/");
        let s = serialize_head(
            "GET",
            "/",
            &u,
            &headers(&[("Host", "vhost.invalid"), ("X-Mixed-Case", "v")]),
            &[],
            0,
            Mode::Normal,
        );
        let t = text(&s);
        assert!(t.contains("Host: vhost.invalid\r\n"), "{t}");
        assert_eq!(t.matches("Host:").count(), 1, "{t}");
        assert!(t.contains("X-Mixed-Case: v\r\n"), "{t}");
        assert!(!t.contains("Content-Length"), "{t}");
    }

    #[test]
    fn a_callers_chunked_encoding_frames_the_body_chunked() {
        let u = url("http://h.invalid/");
        let s = serialize_head(
            "PUT",
            "/",
            &u,
            &headers(&[("Transfer-Encoding", "chunked")]),
            &[],
            3,
            Mode::Normal,
        );
        assert_eq!(s.framing, Framing::Chunked);
        assert!(!text(&s).contains("Content-Length"));
        assert_eq!(
            frame_body(b"abc", Framing::Chunked),
            b"3\r\nabc\r\n0\r\n\r\n"
        );
        assert_eq!(frame_body(b"", Framing::Chunked), b"0\r\n\r\n");
    }

    #[test]
    fn the_bypass_modes_keep_their_close_framing() {
        let u = url("http://h.invalid/");
        let trailers = serialize_head(
            "GET",
            "/",
            &u,
            &headers(&[("TE", "trailers"), ("Connection", "keep-alive")]),
            &[],
            0,
            Mode::Trailers,
        );
        let t = text(&trailers);
        assert!(t.ends_with("Connection: close\r\n\r\n"), "{t}");
        assert_eq!(t.matches("Connection").count(), 1, "{t}");
        assert!(trailers.closes);

        let cont = serialize_head(
            "POST",
            "/",
            &u,
            &headers(&[("Expect", "100-continue")]),
            &[],
            0,
            Mode::Continue,
        );
        let t = text(&cont);
        assert!(t.contains("Transfer-Encoding: chunked\r\n"), "{t}");
        assert!(t.ends_with("Connection: close\r\n\r\n"), "{t}");
        assert_eq!(cont.framing, Framing::Chunked);

        let upgrade = serialize_head(
            "GET",
            "/",
            &u,
            &headers(&[("Connection", "Upgrade"), ("Upgrade", "websocket")]),
            &[],
            0,
            Mode::Upgrade,
        );
        let t = text(&upgrade);
        assert!(t.contains("Connection: Upgrade\r\n"), "{t}");
        assert!(!t.contains("keep-alive"), "{t}");
        assert!(!upgrade.closes);
    }

    #[test]
    fn absolute_form_is_used_through_an_http_proxy() {
        let u = url("http://h.invalid:81/a?b#frag");
        assert_eq!(request_target(&u, true), "http://h.invalid:81/a?b");
        assert_eq!(request_target(&u, false), "/a?b");
    }

    #[test]
    fn the_reason_phrase_is_the_servers_own() {
        assert_eq!(
            reason_phrase(b"HTTP/1.1 200 Custom Words\r\nx: y\r\n\r\n"),
            "Custom Words"
        );
        assert_eq!(reason_phrase(b"HTTP/1.1 204\r\n\r\n"), "");
        assert_eq!(
            reason_phrase(b"HTTP/1.0 404 Not Found\r\n\r\n"),
            "Not Found"
        );
    }

    #[test]
    fn proxy_credentials_are_percent_decoded_basic() {
        let p = url("http://us%40er:p%3Aw@proxy.invalid:3128");
        // base64("us@er:p:w")
        assert_eq!(basic_credentials(&p).as_deref(), Some("Basic dXNAZXI6cDp3"));
        assert_eq!(basic_credentials(&url("http://proxy.invalid:3128")), None);
        let head = String::from_utf8(connect_head("h.invalid", 443, &p)).unwrap();
        assert!(head.starts_with("CONNECT h.invalid:443 HTTP/1.1\r\nHost: h.invalid:443\r\n"));
        assert!(head.contains("Proxy-Authorization: Basic dXNAZXI6cDp3\r\n"));
    }
}
