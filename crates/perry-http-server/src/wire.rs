//! Response serialization: the framing decision, the status line and the head.
//!
//! The head and its framing come from `turnloop_http::http1::Encoder` — the
//! same codec [`crate::conn`] parses requests with — so chunk encoding and
//! trailers are the protocol crate's, not ours.
//!
//! Three Node behaviours the encoder cannot express on its own, all handled
//! here. They were found by P5 against Node 26.5.1 and are reported upstream
//! (`docs/turnloop/p5-report.md`, "turnloop gaps found"); this module is that
//! work, lifted out of `perry-ext-http` so a second binding does not have to
//! rediscover any of them:
//!
//! 1. **A custom reason phrase.** `reply.code(404)` with a message is
//!    observable on the wire. `Encoder::start` always writes the IANA
//!    canonical reason, so the status line is patched afterwards.
//! 2. **A close-delimited body.** An HTTP/1.0 response with neither
//!    `Content-Length` nor chunked framing ends at EOF, and `BodyLength` has
//!    no variant for it, so that head is written directly.
//! 3. **A body-forbidden response.** A HEAD response advertises the
//!    `Content-Length` it *would* have sent and emits no body, which
//!    `Encoder::start(…, Known(0))` rejects as a conflict.

use turnloop_http::http1::{BodyLength, Encoder, Head, Header};

/// Node's `Date` response header value, in RFC 7231 IMF-fixdate form.
///
/// Node's HTTP server sends `Date` on every response unless `sendDate` is
/// false, and spells the name capitalised. It is not synthesized by the core —
/// a caller pushes it where it wants it in the header order, because the order
/// is observable and Node's is `…, Content-Length, Date, Connection,
/// Keep-Alive`.
pub fn http_date_now() -> String {
    httpdate::fmt_http_date(std::time::SystemTime::now())
}

/// How the body of one response is framed on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    /// `Content-Length` (possibly zero).
    Sized(u64),
    /// The status or the request method forbids a body: the head is written
    /// exactly as the handler set it — including the `Content-Length` a HEAD
    /// response still advertises — and not one byte follows.
    NoBody,
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// HTTP/1.0 close-delimited: the body ends when the connection does.
    UntilClose,
}

/// A serialized head plus the encoder that frames the body that follows.
pub struct EncodedHead {
    pub bytes: Vec<u8>,
    /// `None` for [`Framing::UntilClose`] and [`Framing::NoBody`], whose bodies
    /// are written raw or not at all.
    pub encoder: Option<Encoder>,
}

pub(crate) fn header_value(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

/// Whether a response with this status, for this request method, may carry a
/// body at all (RFC 9110 §6.4.1 plus Node's `_http_server` rules).
pub fn body_forbidden(status: u16, request_method: &str) -> bool {
    request_method.eq_ignore_ascii_case("HEAD")
        || status == 204
        || status == 304
        || (100..200).contains(&status)
}

/// Decide the framing for a response, from the headers the handler committed.
///
/// `known_len` is `Some` for a fully buffered body and `None` while streaming,
/// which is exactly Node's condition for falling back to chunked on HTTP/1.1.
pub fn framing_for(
    headers: &[(String, String)],
    status: u16,
    request_method: &str,
    request_version: u8,
    known_len: Option<u64>,
    eof_framed: bool,
) -> Framing {
    if body_forbidden(status, request_method) {
        return Framing::NoBody;
    }
    if eof_framed {
        return Framing::UntilClose;
    }
    if header_value(headers, "transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    {
        return Framing::Chunked;
    }
    if let Some(cl) =
        header_value(headers, "content-length").and_then(|v| v.trim().parse::<u64>().ok())
    {
        return Framing::Sized(cl);
    }
    if let Some(len) = known_len {
        return Framing::Sized(len);
    }
    // Streaming with no declared length. HTTP/1.1 chunks; HTTP/1.0 has no
    // chunked encoding, so the body is close-delimited exactly as Node does.
    if request_version == 0 {
        Framing::UntilClose
    } else {
        Framing::Chunked
    }
}

/// Bring the headers into line with the framing that was chosen, the way Node
/// writes them.
///
/// * **`Transfer-Encoding: chunked` in Node's casing.** `Encoder::start`
///   synthesizes the header itself when the framing is chunked and no header
///   says so — in lowercase, because that is how the crate spells its own
///   output. Node writes `Transfer-Encoding`. Adding it here means the encoder
///   finds one and emits ours.
/// * **No synthesized `Content-Length` where Node sends none** — on 204, 304,
///   1xx, a HEAD response, or a close-delimited HTTP/1.0 body. `_hasBody` is
///   false for the first four, so Node never computes one. A length the
///   *handler* set is left alone, which Node also keeps; that is what
///   `auto_content_length` distinguishes.
pub fn align_headers(
    headers: &mut Vec<(String, String)>,
    framing: Framing,
    auto_content_length: bool,
) {
    if framing == Framing::Chunked
        && !headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("transfer-encoding"))
    {
        headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
    }
    if auto_content_length && matches!(framing, Framing::NoBody | Framing::UntilClose) {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case("content-length"));
    }
}

/// Serialize the status line and headers, and open the body encoder.
///
/// `headers` is emitted verbatim, in order, with the case the handler used —
/// Node preserves both, and so does `http1::Encoder`.
pub fn encode_head(
    status: u16,
    status_message: Option<&str>,
    headers: &[(String, String)],
    framing: Framing,
) -> Result<EncodedHead, String> {
    let head = Head {
        method: String::new(),
        target: String::new(),
        status,
        version: 1,
        headers: headers
            .iter()
            .map(|(name, value)| Header {
                name: name.clone(),
                value: value.as_bytes().to_vec(),
            })
            .collect(),
        // Every caller commits an explicit `Connection` header before it gets
        // here (see [`crate::connection_headers`]), so the encoder never has to
        // synthesize one; this field only gates that synthesis.
        keep_alive: true,
    };

    if matches!(framing, Framing::UntilClose | Framing::NoBody) {
        let mut bytes = Vec::with_capacity(256);
        write_status_line(&mut bytes, status, status_message);
        for (name, value) in headers {
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(b": ");
            bytes.extend_from_slice(value.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"\r\n");
        return Ok(EncodedHead {
            bytes,
            encoder: None,
        });
    }

    let length = match framing {
        Framing::Sized(n) => BodyLength::Known(n),
        Framing::Chunked => BodyLength::Chunked,
        Framing::UntilClose | Framing::NoBody => unreachable!("handled above"),
    };
    let mut bytes = Vec::with_capacity(256);
    let encoder = Encoder::start(&head, length, &mut bytes).map_err(|e| e.to_string())?;
    if let Some(message) = status_message {
        patch_status_line(&mut bytes, status, message);
    }
    Ok(EncodedHead {
        bytes,
        encoder: Some(encoder),
    })
}

fn write_status_line(out: &mut Vec<u8>, status: u16, message: Option<&str>) {
    let reason = message
        .map(str::to_string)
        .or_else(|| {
            http::StatusCode::from_u16(status)
                .ok()
                .and_then(|s| s.canonical_reason())
                .map(str::to_string)
        })
        .unwrap_or_default();
    out.extend_from_slice(b"HTTP/1.1 ");
    out.extend_from_slice(status.to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(reason.as_bytes());
    out.extend_from_slice(b"\r\n");
}

/// Replace the encoder's canonical reason phrase with the handler's.
fn patch_status_line(bytes: &mut Vec<u8>, status: u16, message: &str) {
    let Some(eol) = bytes.windows(2).position(|w| w == b"\r\n") else {
        return;
    };
    let mut line = Vec::with_capacity(16 + message.len());
    write_status_line(&mut line, status, Some(message));
    bytes.splice(..eol + 2, line);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_head_carries_the_handler_headers_verbatim() {
        let headers = vec![
            ("Content-Type".to_string(), "text/plain".to_string()),
            ("Content-Length".to_string(), "5".to_string()),
        ];
        let encoded = encode_head(200, None, &headers, Framing::Sized(5)).expect("head");
        let text = String::from_utf8(encoded.bytes).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text:?}");
        assert!(text.contains("Content-Type: text/plain\r\n"), "{text:?}");
        assert!(text.ends_with("\r\n\r\n"), "{text:?}");
        assert!(encoded.encoder.is_some());
    }

    #[test]
    fn a_custom_reason_phrase_replaces_the_canonical_one() {
        let headers = vec![("Content-Length".to_string(), "0".to_string())];
        let encoded =
            encode_head(404, Some("Nope Nope"), &headers, Framing::Sized(0)).expect("head");
        let text = String::from_utf8(encoded.bytes).expect("utf8");
        assert!(text.starts_with("HTTP/1.1 404 Nope Nope\r\n"), "{text:?}");
        assert!(text.contains("Content-Length: 0\r\n"), "{text:?}");
    }

    #[test]
    fn chunked_framing_writes_chunks_and_a_terminator() {
        let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        let mut encoded = encode_head(200, None, &headers, Framing::Chunked).expect("head");
        let mut out = Vec::new();
        let encoder = encoded.encoder.as_mut().expect("chunked encoder");
        encoder.body(b"hello", &mut out).expect("body");
        encoder.finish(&[], &mut out).expect("finish");
        assert_eq!(
            String::from_utf8(out).expect("utf8"),
            "5\r\nhello\r\n0\r\n\r\n"
        );
    }

    #[test]
    fn a_head_response_keeps_its_content_length_and_sends_no_body() {
        let headers = vec![("Content-Length".to_string(), "5".to_string())];
        assert_eq!(
            framing_for(&headers, 200, "HEAD", 1, Some(5), false),
            Framing::NoBody
        );
        let encoded = encode_head(200, None, &headers, Framing::NoBody).expect("head");
        let text = String::from_utf8(encoded.bytes).expect("utf8");
        assert!(text.contains("Content-Length: 5\r\n"), "{text:?}");
        assert!(encoded.encoder.is_none(), "no body may follow");
    }

    /// `encoder: None` means "this framing writes its body raw, or not at
    /// all" — it does **not** mean the head was refused. Both close-delimited
    /// and body-forbidden framings produce it, so a caller that inferred
    /// "refused" from it would tear down a perfectly good HTTP/1.0 streaming
    /// response the instant its head reached the wire.
    #[test]
    fn an_absent_encoder_is_a_framing_not_a_failure() {
        for framing in [Framing::UntilClose, Framing::NoBody] {
            let encoded = encode_head(200, None, &[], framing).expect("head");
            assert!(encoded.encoder.is_none(), "{framing:?}");
            let text = String::from_utf8(encoded.bytes).expect("utf8");
            assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text:?}");
        }
        // ...and a framing that DOES frame its own body keeps one.
        assert!(encode_head(200, None, &[], Framing::Chunked)
            .expect("head")
            .encoder
            .is_some());
    }

    #[test]
    fn until_close_emits_no_framing_headers_of_its_own() {
        let headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
        let encoded = encode_head(200, None, &headers, Framing::UntilClose).expect("head");
        let text = String::from_utf8(encoded.bytes).expect("utf8");
        assert!(
            !text.to_ascii_lowercase().contains("content-length"),
            "{text:?}"
        );
        assert!(
            !text.to_ascii_lowercase().contains("transfer-encoding"),
            "{text:?}"
        );
    }

    #[test]
    fn a_204_carries_neither_a_body_nor_a_synthesized_length() {
        let encoded = encode_head(204, None, &[], Framing::NoBody).expect("head");
        assert_eq!(
            String::from_utf8(encoded.bytes).expect("utf8"),
            "HTTP/1.1 204 No Content\r\n\r\n"
        );
    }

    #[test]
    fn align_headers_drops_a_synthesized_length_where_node_sends_none() {
        let mut synthesized = vec![("Content-Length".to_string(), "0".to_string())];
        align_headers(&mut synthesized, Framing::NoBody, true);
        assert!(synthesized.is_empty(), "{synthesized:?}");

        let mut eof = vec![("Content-Length".to_string(), "5".to_string())];
        align_headers(&mut eof, Framing::UntilClose, true);
        assert!(eof.is_empty(), "{eof:?}");

        let mut explicit = vec![("Content-Length".to_string(), "0".to_string())];
        align_headers(&mut explicit, Framing::NoBody, false);
        assert_eq!(explicit.len(), 1, "a handler-set length survives");
    }

    #[test]
    fn align_headers_spells_transfer_encoding_the_way_node_does() {
        let mut headers: Vec<(String, String)> = Vec::new();
        align_headers(&mut headers, Framing::Chunked, false);
        assert_eq!(
            headers,
            vec![("Transfer-Encoding".to_string(), "chunked".to_string())]
        );
        let encoded = encode_head(200, None, &headers, Framing::Chunked).expect("head");
        let text = String::from_utf8(encoded.bytes).expect("utf8");
        assert!(text.contains("Transfer-Encoding: chunked\r\n"), "{text:?}");
        assert!(!text.contains("transfer-encoding:"), "{text:?}");
    }

    #[test]
    fn streaming_without_a_length_is_chunked_on_11_and_close_delimited_on_10() {
        let headers: Vec<(String, String)> = Vec::new();
        assert_eq!(
            framing_for(&headers, 200, "GET", 1, None, false),
            Framing::Chunked
        );
        assert_eq!(
            framing_for(&headers, 200, "GET", 0, None, false),
            Framing::UntilClose
        );
    }

    #[test]
    fn an_explicit_content_length_wins_over_the_buffered_length() {
        let headers = vec![("Content-Length".to_string(), "3".to_string())];
        assert_eq!(
            framing_for(&headers, 200, "GET", 1, Some(9), false),
            Framing::Sized(3)
        );
    }
}
