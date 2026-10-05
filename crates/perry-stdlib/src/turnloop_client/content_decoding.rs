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
    waiting: bool,
    finished: bool,
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
                    waiting: true,
                    finished: false,
                }),
                Err(_) => return Ok(None),
            }
        }
        Ok(Some(Self { stages }))
    }

    /// Retain only transport input; decoded output is pulled in 8 KiB blocks.
    pub(super) fn push_input(&mut self, input: &[u8]) {
        if let Some(stage) = self.stages.first_mut() {
            stage.carry.extend_from_slice(input);
            stage.waiting = false;
        }
    }

    pub(super) fn finished(&self) -> bool {
        self.stages.iter().all(|s| s.finished)
    }

    /// Advance downstream stages first, so every intermediate window is at
    /// most one output block. A full consumer queue can suspend a decoder even
    /// when one compressed input block expands to megabytes.
    pub(super) fn next_chunk(&mut self, end: bool) -> Result<Option<Vec<u8>>, ClientError> {
        let mut out = [0u8; 8192];
        loop {
            let mut progress = false;
            for i in (0..self.stages.len()).rev() {
                let ending = if i == 0 {
                    end
                } else {
                    self.stages[i - 1].finished
                };
                let stage = &mut self.stages[i];
                if stage.finished || (stage.waiting && !ending) {
                    continue;
                }
                // Undici uses a sync-flush finish: a missing compressed tail
                // is tolerated, while malformed input/checksums still fail.
                // Always process available input before asking for EOF so
                // partial output is delivered and corrupt input isn't hidden.
                let mut step = stage
                    .decoder
                    .process(&stage.carry, &mut out, false)
                    .map_err(|e| ClientError::new(e.code, e.message))?;
                if ending && step.consumed == 0 && step.written == 0 && !step.finished {
                    match stage.decoder.process(&stage.carry, &mut out, true) {
                        Ok(final_step) => step = final_step,
                        Err(error) if error.code == "UND_ERR_SOCKET" => {
                            stage.finished = true;
                            stage.carry.clear();
                            progress = true;
                            continue;
                        }
                        Err(error) => return Err(ClientError::new(error.code, error.message)),
                    }
                }
                stage.carry.drain(..step.consumed);
                stage.finished = step.finished;
                stage.waiting = step.consumed == 0 && step.written == 0;
                progress |= step.consumed > 0 || step.written > 0 || step.finished;
                if step.written > 0 {
                    if i + 1 == self.stages.len() {
                        return Ok(Some(out[..step.written].to_vec()));
                    }
                    self.stages[i + 1]
                        .carry
                        .extend_from_slice(&out[..step.written]);
                    self.stages[i + 1].waiting = false;
                    // Drain that stage before producing another block upstream.
                    break;
                }
            }
            if !progress {
                return Ok(None);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn feed(
        &mut self,
        input: &[u8],
        emit: &mut dyn FnMut(&[u8]) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        self.push_input(input);
        while let Some(bytes) = self.next_chunk(false)? {
            emit(&bytes)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn finish(&mut self, emit: &mut dyn FnMut(&[u8]) -> Result<(), ClientError>) {
        while let Ok(Some(bytes)) = self.next_chunk(true) {
            if emit(&bytes).is_err() {
                break;
            }
        }
        self.stages.clear();
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[test]
    fn completion_releases_decoder_stages() {
        let mut decoder = ContentDecoder::for_header("gzip", 4096).unwrap().unwrap();
        let mut output = Vec::new();
        let mut emit = |bytes: &[u8]| {
            output.extend_from_slice(bytes);
            Ok(())
        };
        decoder
            .feed(
                &[
                    31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 75, 73, 77, 206, 79, 73, 45, 82, 40, 207,
                    47, 202, 46, 46, 72, 76, 78, 5, 0, 45, 146, 37, 255, 17, 0, 0, 0,
                ],
                &mut emit,
            )
            .unwrap();
        decoder.finish(&mut emit);
        assert_eq!(output, b"decoder workspace");
        assert!(
            decoder.stages.is_empty(),
            "finished codecs must release their workspace"
        );
    }
}

#[cfg(test)]
#[test]
fn high_expansion_decoder_yields_bounded_blocks_without_losing_output() {
    let compressed = [
        31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 237, 193, 49, 1, 0, 0, 0, 194, 160, 108, 235, 95, 202,
        16, 190, 64, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        124, 6, 201, 190, 246, 129, 0, 0, 16, 0,
    ];
    let mut decoder = ContentDecoder::for_header("gzip", usize::MAX)
        .unwrap()
        .unwrap();
    decoder.push_input(&compressed);
    let mut total = 0;
    while let Some(bytes) = decoder.next_chunk(false).unwrap() {
        assert!(bytes.len() <= 8192);
        assert!(bytes.iter().all(|b| *b == b'A'));
        total += bytes.len();
        assert!(decoder.stages.iter().all(|s| s.carry.len() <= 8192));
    }
    assert_eq!(total, 1024 * 1024);
    assert!(decoder.next_chunk(true).unwrap().is_none());
    assert!(decoder.finished());
}
