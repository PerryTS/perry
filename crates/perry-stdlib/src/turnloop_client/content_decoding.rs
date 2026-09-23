//! `Content-Encoding` negotiation and decoding for the fetch client (#10475).
//!
//! Both halves follow undici's `fetch` (Node's), which is the oracle:
//!
//! * **Request**: with no caller `Accept-Encoding`, undici sends
//!   `gzip, deflate` over `http:` and `br, gzip, deflate, zstd` over `https:`,
//!   recomputed per redirect hop. A `Range` request additionally appends
//!   `identity`. [`apply_default_accept_encoding`] does exactly that.
//! * **Response**: the header is lower-cased, split on `,`, and decoded in
//!   REVERSE order (the last coding applied is the first removed). More than
//!   five codings rejects the request. A coding undici does not know — which
//!   includes `identity` and an empty token — disables decoding for the whole
//!   body, which is then delivered as received. [`ContentDecoder`] is that
//!   chain; each stage is a `turnloop_http::compression::StreamingDecoder`.
//!
//! The codecs are `turnloop-http`'s own (flate2 + brotli + zstd are
//! unconditional dependencies of that crate), so decoding `br` needs no
//! perry-stdlib feature: a fetch-only program built with nothing but
//! `turnloop-http-client` decodes all four encodings. Do not route this through
//! perry-stdlib's optional `brotli` dependency — that one belongs to the
//! `zlib` / WHATWG-streams codecs and is feature-gated for binary size.

use turnloop_http::compression::StreamingDecoder;
use turnloop_http::http1;

use super::ClientError;

/// undici's `maxContentEncodings`.
const MAX_CONTENT_ENCODINGS: usize = 5;

/// Add undici's default `Accept-Encoding` to an outgoing head. `https` is the
/// scheme of the hop being sent, not of the original URL.
pub(super) fn apply_default_accept_encoding(head: &mut http1::Head, https: bool) {
    let has = |head: &http1::Head, name: &str| {
        head.headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    };
    if has(head, "range") {
        // undici `append`s, so a caller value becomes `<value>, identity`.
        match head
            .headers
            .iter_mut()
            .find(|h| h.name.eq_ignore_ascii_case("accept-encoding"))
        {
            Some(existing) => {
                existing.value.extend_from_slice(b", identity");
            }
            None => head.headers.push(http1::Header::new(
                "accept-encoding",
                b"identity".as_slice(),
            )),
        }
    }
    if !has(head, "accept-encoding") {
        let value: &[u8] = if https {
            b"br, gzip, deflate, zstd"
        } else {
            b"gzip, deflate"
        };
        head.headers
            .push(http1::Header::new("accept-encoding", value));
    }
}

/// A chain of streaming decoders, outermost coding first.
pub(super) struct ContentDecoder {
    stages: Vec<Stage>,
}

/// One decoder plus the input it has not consumed yet.
///
/// `StreamingDecoder::process` leaves input it cannot use yet unconsumed —
/// a gzip header or trailer, or deflate's 2-byte zlib sniff, split across two
/// body chunks answers `consumed = 0` — and its contract is that the CALLER
/// retains it. The single-decoder engine dropped it instead, so a gzip
/// trailer that straddled two socket reads corrupted the body. `carry` is
/// that retained tail, prepended to the next chunk.
struct Stage {
    decoder: StreamingDecoder,
    carry: Vec<u8>,
}

impl ContentDecoder {
    /// Build the decoder chain for a response's `Content-Encoding` value.
    /// `Ok(None)` means "deliver the body as received".
    pub(super) fn for_header(value: &str, limit: usize) -> Result<Option<Self>, ClientError> {
        let value = value.to_ascii_lowercase();
        if value.trim().is_empty() {
            return Ok(None);
        }
        let codings: Vec<&str> = value.split(',').collect();
        if codings.len() > MAX_CONTENT_ENCODINGS {
            return Err(ClientError::new(
                "UND_ERR_INVALID_ARG",
                format!(
                    "too many content-encodings in response: {}, maximum allowed is {}",
                    codings.len(),
                    MAX_CONTENT_ENCODINGS
                ),
            ));
        }
        let mut stages = Vec::with_capacity(codings.len());
        for coding in codings.iter().rev().map(|c| c.trim()) {
            if !matches!(coding, "gzip" | "x-gzip" | "deflate" | "br" | "zstd") {
                return Ok(None);
            }
            match StreamingDecoder::new(coding, limit) {
                Ok(decoder) => stages.push(Stage {
                    decoder,
                    carry: Vec::new(),
                }),
                Err(_) => return Ok(None),
            }
        }
        Ok(Some(Self { stages }))
    }

    /// Decode one body chunk, handing every fully-decoded byte to `emit`.
    pub(super) fn feed(
        &mut self,
        input: &[u8],
        emit: &mut dyn FnMut(&[u8]) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        pump(&mut self.stages, input, emit)
    }

    /// Flush every stage at end of body. Errors are swallowed stage by stage:
    /// a decoder that has already produced everything answers an empty
    /// `end = true` call with "incomplete body", and that must not turn a
    /// complete response into a failed fetch (the single-decoder engine did
    /// the same).
    pub(super) fn finish(&mut self, emit: &mut dyn FnMut(&[u8]) -> Result<(), ClientError>) {
        let mut out = [0u8; 8192];
        for i in 0..self.stages.len() {
            let (stage, rest) = self.stages[i..].split_first_mut().unwrap();
            let input = std::mem::take(&mut stage.carry);
            let mut pos = 0;
            loop {
                match stage.decoder.process(&input[pos..], &mut out, true) {
                    Ok(step) => {
                        pos += step.consumed;
                        if step.written > 0 && pump(rest, &out[..step.written], emit).is_err() {
                            break;
                        }
                        if step.finished || (step.consumed == 0 && step.written == 0) {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

/// Feed `input` through `stages[0]`, recursively passing its output down the
/// chain; bytes leaving the last stage go to `emit`. Input the stage could not
/// consume yet is kept in its `carry` for the next call.
fn pump(
    stages: &mut [Stage],
    input: &[u8],
    emit: &mut dyn FnMut(&[u8]) -> Result<(), ClientError>,
) -> Result<(), ClientError> {
    let Some((stage, rest)) = stages.split_first_mut() else {
        return emit(input);
    };
    let joined;
    let input = if stage.carry.is_empty() {
        input
    } else {
        let mut buf = std::mem::take(&mut stage.carry);
        buf.extend_from_slice(input);
        joined = buf;
        &joined[..]
    };
    let mut pos = 0;
    let mut out = [0u8; 8192];
    loop {
        let step = stage
            .decoder
            .process(&input[pos..], &mut out, false)
            .map_err(|e| ClientError::new(e.code, e.message))?;
        pos += step.consumed;
        if step.written > 0 {
            pump(rest, &out[..step.written], emit)?;
        }
        if step.finished || pos >= input.len() {
            return Ok(());
        }
        if step.consumed == 0 && step.written == 0 {
            // Needs more input than this chunk holds: retain the tail.
            stage.carry.extend_from_slice(&input[pos..]);
            return Ok(());
        }
    }
}
