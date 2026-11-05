//! Bounded decompression into a caller-reused result buffer. This convenience API
//! accepts a complete encoded body; wire body streaming is exposed by the codecs.
use crate::{Error, Result};
/// Convenience whole-body decode. Cache `StreamingDecoder` and use `reset` for
/// allocation-free repeated responses instead of constructing one per call.
pub fn decode(encoding: &str, input: &[u8], output: &mut Vec<u8>, limit: usize) -> Result<()> {
    output.clear();
    let mut decoder = StreamingDecoder::new(encoding.trim(), limit)?;
    let mut pos = 0;
    loop {
        let mut bytes = [0; 8192];
        let step = decoder.process(&input[pos..], &mut bytes, true)?;
        pos += step.consumed;
        output.extend_from_slice(&bytes[..step.written]);
        if step.finished {
            return Ok(());
        }
        // All input was given with `end`, so wanting more means truncation.
        if step.needs_input {
            return Err(corrupt());
        }
    }
}

/// A single incremental decode step. Retain input after `consumed`, and consume
/// output before calling again. A single coding stages nothing internally; a
/// chain stages between its codings in buffers allocated at construction.
///
/// A step returns for exactly one of three reasons, and says which:
///
/// | `finished` | `needs_input` | stopped because | next |
/// |---|---|---|---|
/// | `true` | `false` | the body is complete | nothing; `reset` for another body |
/// | `false` | `true` | every input byte the decoder can use is used | more input, or `end = true` |
/// | `false` | `false` | `output` is full (`written == output.len()`) | drain, then call again |
///
/// So a host sizes its scratch buffer from the third case - it filled the
/// buffer - and reads from the transport only in the second. The unconsumed
/// tail of the input in the second case is a partial header or trailer the
/// decoder cannot use yet; keep it and append to it.
#[derive(Debug)]
pub struct DecodeStep {
    pub consumed: usize,
    pub written: usize,
    pub finished: bool,
    /// The decoder stopped for input, not for output space. See the table.
    pub needs_input: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Header,
    Body,
    Trailer,
    Done,
}
type BytePool = crate::recycling::Pool<u8>;
type IntPool = crate::recycling::Pool<u32>;
type CodePool = crate::recycling::Pool<brotli::HuffmanCode>;
type BrotliState = brotli::BrotliState<BytePool, IntPool, CodePool>;
// Keep the WASM decoder inline: 480 bytes avoids another per-response allocation.
#[allow(clippy::large_enum_variant)]
enum Engine {
    Identity,
    Deflate {
        decoder: flate2::Decompress,
        gzip: bool,
        detect: bool,
        stage: Stage,
        crc: flate2::Crc,
        members: usize,
    },
    Brotli {
        state: Box<BrotliState>,
        bytes: BytePool,
        ints: IntPool,
        codes: CodePool,
    },
    #[cfg(not(any(target_arch = "wasm32", feature = "pure-rust-zstd")))]
    Zstd {
        decoder: zstd::stream::raw::Decoder<'static>,
        boundary: bool,
    },
    #[cfg(any(target_arch = "wasm32", feature = "pure-rust-zstd"))]
    Zstd {
        decoder: turnloop_zstd_decoder::decoding::FrameDecoder,
        reset: bool,
        boundary: bool,
    },
}
/// Content codings one decoder accepts in a chain. A list is attacker-chosen and
/// every coding adds state and a staging buffer; undici stops at five as well.
pub const MAX_CODINGS: usize = 5;
/// Bytes staged between two codings of a chain, above the 8 KiB a gzip header
/// may take, so the inner coding can always see a whole header.
const STAGE_BYTES: usize = 16 * 1024;
/// Incremental decoder for a `Content-Encoding` value: one coding, or a list
/// of them applied in order (see [`StreamingDecoder::from_codings`]).
pub struct StreamingDecoder {
    /// The innermost coding - listed first, decoded last - writing the
    /// caller's output. The only coding when the list has one.
    codec: Codec,
    /// The other codings, outermost (listed last) first, each with the bytes it
    /// has produced and the next coding has not yet taken. Empty, and so never
    /// allocated, for a single coding.
    outer: Vec<Staged>,
    limit: usize,
    failed: bool,
}
/// One coding's decoder state.
struct Codec {
    engine: Engine,
    total: usize,
    done: bool,
}
/// An outer coding of a chain and the output it has staged for the next one.
struct Staged {
    codec: Codec,
    buffer: Box<[u8]>,
    start: usize,
    end: usize,
}
fn unsupported() -> Error {
    Error::new("UND_ERR_NOT_SUPPORTED", "unsupported content encoding")
}
fn corrupt() -> Error {
    Error::new("UND_ERR_SOCKET", "invalid or incomplete compressed body")
}
impl Codec {
    fn new(token: &[u8]) -> Result<Self> {
        // Content codings are case-insensitive (RFC 9110 section 8.4.1).
        let name = std::str::from_utf8(token)
            .map_err(|_| unsupported())?
            .to_ascii_lowercase();
        let engine = match name.as_str() {
            "" | "identity" => Engine::Identity,
            "gzip" | "x-gzip" => Engine::Deflate {
                decoder: flate2::Decompress::new(false),
                gzip: true,
                detect: false,
                stage: Stage::Header,
                crc: flate2::Crc::new(),
                members: 0,
            },
            "deflate" => Engine::Deflate {
                decoder: flate2::Decompress::new(false),
                gzip: false,
                detect: true,
                stage: Stage::Body,
                crc: flate2::Crc::new(),
                members: 0,
            },
            "br" => {
                let bytes = BytePool::default();
                let ints = IntPool::default();
                let codes = CodePool::default();
                let state = Box::new(BrotliState::new(bytes.clone(), ints.clone(), codes.clone()));
                Engine::Brotli {
                    state,
                    bytes,
                    ints,
                    codes,
                }
            }
            #[cfg(not(any(target_arch = "wasm32", feature = "pure-rust-zstd")))]
            "zstd" => {
                let mut decoder = zstd::stream::raw::Decoder::new().map_err(|_| corrupt())?;
                decoder
                    .set_parameter(zstd::stream::raw::DParameter::WindowLogMax(27))
                    .map_err(|_| corrupt())?;
                Engine::Zstd {
                    decoder,
                    boundary: false,
                }
            }
            #[cfg(any(target_arch = "wasm32", feature = "pure-rust-zstd"))]
            "zstd" => Engine::Zstd {
                decoder: turnloop_zstd_decoder::decoding::FrameDecoder::new(),
                reset: true,
                boundary: false,
            },
            _ => return Err(unsupported()),
        };
        Ok(Self {
            engine,
            total: 0,
            done: false,
        })
    }
    fn reset(&mut self) -> Result<()> {
        match &mut self.engine {
            Engine::Identity => {}
            Engine::Deflate {
                decoder,
                gzip,
                detect,
                stage,
                crc,
                members,
            } => {
                decoder.reset(false);
                *detect = !*gzip;
                *stage = if *gzip { Stage::Header } else { Stage::Body };
                crc.reset();
                *members = 0;
            }
            Engine::Brotli {
                state,
                bytes,
                ints,
                codes,
            } => {
                **state = BrotliState::new(bytes.clone(), ints.clone(), codes.clone());
            }
            #[cfg(not(any(target_arch = "wasm32", feature = "pure-rust-zstd")))]
            Engine::Zstd { decoder, boundary } => {
                use zstd::stream::raw::Operation;
                decoder.reinit().map_err(|_| corrupt())?;
                *boundary = false;
            }
            #[cfg(any(target_arch = "wasm32", feature = "pure-rust-zstd"))]
            Engine::Zstd {
                reset, boundary, ..
            } => {
                *reset = true;
                *boundary = false;
            }
        }
        self.total = 0;
        self.done = false;
        Ok(())
    }
    fn run(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        end: bool,
        limit: usize,
    ) -> Result<DecodeStep> {
        // Run single engine steps until one of the three stop reasons holds, so
        // the step that comes back names which one it was.
        let mut consumed = 0;
        let mut written = 0;
        loop {
            let step = self.step_once(&input[consumed..], &mut output[written..], end, limit)?;
            consumed += step.consumed;
            written += step.written;
            let stalled = step.consumed == 0 && step.written == 0;
            if step.finished || written == output.len() || stalled {
                return Ok(DecodeStep {
                    consumed,
                    written,
                    finished: step.finished,
                    needs_input: !step.finished && written < output.len(),
                });
            }
        }
    }
    /// One call into the engine. It may stop short of all three reasons: at a
    /// gzip member boundary, after a header, or wherever the engine returns.
    fn step_once(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        end: bool,
        limit: usize,
    ) -> Result<DecodeStep> {
        if self.done {
            if !input.is_empty() {
                return Err(corrupt());
            }
            return Ok(DecodeStep {
                consumed: 0,
                written: 0,
                finished: true,
                needs_input: false,
            });
        }
        if output.is_empty() {
            return Ok(DecodeStep {
                consumed: 0,
                written: 0,
                finished: false,
                needs_input: false,
            });
        }
        let mut consumed = 0;
        let mut written = 0;
        let mut finished = false;
        match &mut self.engine {
            Engine::Identity => {
                consumed = input.len().min(output.len());
                output[..consumed].copy_from_slice(&input[..consumed]);
                written = consumed;
                finished = end && consumed == input.len();
            }
            Engine::Deflate {
                decoder,
                gzip,
                detect,
                stage,
                crc,
                members,
            } => {
                if *stage == Stage::Header {
                    if input.is_empty() && end && *members > 0 {
                        finished = true;
                    } else if let Some(n) = gzip_header(input)? {
                        consumed = n;
                        decoder.reset(false);
                        crc.reset();
                        *stage = Stage::Body;
                    } else if end {
                        return Err(corrupt());
                    }
                } else if *stage == Stage::Trailer {
                    if input.len() >= 8 {
                        if u32::from_le_bytes(input[..4].try_into().unwrap()) != crc.sum()
                            || u32::from_le_bytes(input[4..8].try_into().unwrap()) != crc.amount()
                        {
                            return Err(corrupt());
                        }
                        consumed = 8;
                        *members += 1;
                        *stage = Stage::Header;
                        finished = end && input.len() == 8;
                    } else if end {
                        return Err(corrupt());
                    }
                } else if *stage == Stage::Done {
                    if !input.is_empty() {
                        return Err(corrupt());
                    }
                    finished = end;
                } else {
                    if *detect {
                        if input.len() < 2 {
                            if end {
                                return Err(corrupt());
                            }
                            return Ok(DecodeStep {
                                consumed: 0,
                                written: 0,
                                finished: false,
                                needs_input: false,
                            });
                        }
                        let zlib = input[0] & 15 == 8
                            && u16::from_be_bytes([input[0], input[1]]).is_multiple_of(31);
                        decoder.reset(zlib);
                        *detect = false;
                    }
                    let before_in = decoder.total_in();
                    let before_out = decoder.total_out();
                    let status = decoder
                        .decompress(input, output, flate2::FlushDecompress::None)
                        .map_err(|_| corrupt())?;
                    consumed = (decoder.total_in() - before_in) as usize;
                    written = (decoder.total_out() - before_out) as usize;
                    if *gzip {
                        crc.update(&output[..written]);
                    }
                    if status == flate2::Status::StreamEnd {
                        *stage = if *gzip { Stage::Trailer } else { Stage::Done };
                        finished = !*gzip && end && consumed == input.len();
                    } else if end && consumed == input.len() && written < output.len() {
                        return Err(corrupt());
                    }
                }
            }
            Engine::Brotli { state, .. } => {
                let mut available_in = input.len();
                let mut available_out = output.len();
                let mut total = self.total;
                let result = brotli::BrotliDecompressStream(
                    &mut available_in,
                    &mut consumed,
                    input,
                    &mut available_out,
                    &mut written,
                    output,
                    &mut total,
                    state,
                );
                match result {
                    brotli::BrotliResult::ResultSuccess => {
                        if consumed != input.len() {
                            return Err(corrupt());
                        }
                        finished = true;
                    }
                    brotli::BrotliResult::ResultFailure => return Err(corrupt()),
                    brotli::BrotliResult::NeedsMoreInput if end => return Err(corrupt()),
                    _ => {}
                }
            }
            #[cfg(not(any(target_arch = "wasm32", feature = "pure-rust-zstd")))]
            Engine::Zstd { decoder, boundary } => {
                use zstd::stream::raw::Operation;
                if input.is_empty() && end && *boundary {
                    self.done = true;
                    return Ok(DecodeStep {
                        consumed: 0,
                        written: 0,
                        finished: true,
                        needs_input: false,
                    });
                }
                let result = decoder
                    .run_on_buffers(input, output)
                    .map_err(|_| corrupt())?;
                consumed = result.bytes_read;
                written = result.bytes_written;
                // A call that moves nothing reports the size of the *next*
                // frame's header, which says nothing about the frame that just
                // ended: only progress may move the boundary.
                if consumed > 0 || written > 0 {
                    *boundary = result.remaining == 0;
                }
                finished = *boundary && end && consumed == input.len();
                if !*boundary && end && consumed == input.len() && written < output.len() {
                    return Err(corrupt());
                }
            }
            #[cfg(any(target_arch = "wasm32", feature = "pure-rust-zstd"))]
            Engine::Zstd {
                decoder,
                reset,
                boundary,
            } => {
                if input.is_empty() && end && *boundary {
                    self.done = true;
                    return Ok(DecodeStep {
                        consumed: 0,
                        written: 0,
                        finished: true,
                        needs_input: false,
                    });
                }
                let mut source = input;
                if *reset {
                    if source.len() < 18 && !end {
                        return Ok(DecodeStep {
                            consumed: 0,
                            written: 0,
                            finished: false,
                            needs_input: false,
                        });
                    }
                    decoder.reset(&mut source).map_err(|_| corrupt())?;
                    consumed = input.len() - source.len();
                    *reset = false;
                    *boundary = false;
                }
                let result = decoder
                    .decode_from_to(source, output)
                    .map_err(|_| corrupt())?;
                consumed += result.0;
                written = result.1;
                if decoder.is_finished() && decoder.can_collect() == 0 {
                    if let Some(expected) = decoder.get_checksum_from_data()
                        && decoder.get_calculated_checksum() != Some(expected)
                    {
                        return Err(corrupt());
                    }
                    *boundary = true;
                    *reset = true;
                    finished = end && consumed == input.len();
                } else if end && consumed == 0 && written == 0 {
                    return Err(corrupt());
                }
            }
        }
        if written > limit.saturating_sub(self.total) {
            return Err(Error::new(
                "UND_ERR_RES_EXCEEDED_MAX_SIZE",
                "decoded body exceeds limit",
            ));
        }
        self.total += written;
        self.done = finished;
        Ok(DecodeStep {
            consumed,
            written,
            finished,
            needs_input: false,
        })
    }
}
impl Staged {
    /// Move the unread bytes to the front, so the free space is contiguous.
    fn compact(&mut self) {
        if self.start > 0 {
            self.buffer.copy_within(self.start..self.end, 0);
            self.end -= self.start;
            self.start = 0;
        }
    }
}
impl StreamingDecoder {
    /// A decoder for one `Content-Encoding` header value, which may be a list
    /// (`"gzip, br"`). The same as [`from_codings`](Self::from_codings) with
    /// that one value.
    pub fn new(encoding: &str, limit: usize) -> Result<Self> {
        Self::from_codings([encoding], limit)
    }
    /// A decoder for a `Content-Encoding` field: pass every header line's
    /// value, in order. Repeated lines are merged as for any list-valued field
    /// (RFC 9110 section 5.3), and each value may itself be a comma-separated
    /// list.
    ///
    /// Codings are listed in the order they were applied, so they are decoded
    /// in reverse: in `gzip, br` the last-listed `br` is outermost and decoded
    /// first. Names are case-insensitive, `x-gzip` is `gzip`, and `identity`
    /// and empty list elements are skipped; no coding at all decodes as
    /// identity. An unknown coding, or more than [`MAX_CODINGS`], fails with
    /// `UND_ERR_NOT_SUPPORTED`.
    ///
    /// One coding decodes straight into the caller's output as before. Each
    /// further coding stages its output in a fixed 16 KiB buffer allocated
    /// here and kept across [`reset`](Self::reset); `limit` bounds every
    /// coding's output, not only the last, so an outer coding cannot expand
    /// without bound into an inner one that produces nothing.
    pub fn from_codings<I>(values: I, limit: usize) -> Result<Self>
    where
        I: IntoIterator,
        I::Item: AsRef<[u8]>,
    {
        let mut codecs = Vec::new();
        for value in values {
            for token in value.as_ref().split(|b| *b == b',') {
                let token = token.trim_ascii();
                if token.is_empty() || token.eq_ignore_ascii_case(b"identity") {
                    continue;
                }
                if codecs.len() == MAX_CODINGS {
                    return Err(Error::new(
                        "UND_ERR_NOT_SUPPORTED",
                        "too many content codings",
                    ));
                }
                codecs.push(Codec::new(token)?);
            }
        }
        let mut codecs = codecs.into_iter();
        let codec = match codecs.next() {
            Some(codec) => codec,
            None => Codec::new(b"identity")?,
        };
        let outer = codecs
            .rev()
            .map(|codec| Staged {
                codec,
                buffer: vec![0; STAGE_BYTES].into_boxed_slice(),
                start: 0,
                end: 0,
            })
            .collect();
        Ok(Self {
            codec,
            outer,
            limit,
            failed: false,
        })
    }
    /// Reuse algorithm and scratch storage for another body of the same encoding.
    /// Warm up with the largest expected body shape before measuring allocations.
    pub fn reset(&mut self, limit: usize) -> Result<()> {
        self.codec.reset()?;
        for stage in &mut self.outer {
            stage.codec.reset()?;
            stage.start = 0;
            stage.end = 0;
        }
        self.limit = limit;
        self.failed = false;
        Ok(())
    }
    /// Decode as far as `input` and `output` allow: the call returns only when
    /// the body is finished, the decoder needs more input, or `output` is full,
    /// and [`DecodeStep`] says which. One call per transport read and one per
    /// full output buffer is enough; there is no need to loop until a step
    /// makes no progress.
    ///
    /// `end` means no more encoded bytes will arrive, not that output is unbounded.
    pub fn process(&mut self, input: &[u8], output: &mut [u8], end: bool) -> Result<DecodeStep> {
        if self.failed {
            return Err(corrupt());
        }
        let result = self.process_inner(input, output, end);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn process_inner(&mut self, input: &[u8], output: &mut [u8], end: bool) -> Result<DecodeStep> {
        let limit = self.limit;
        let Some((last, earlier)) = self.outer.split_last_mut() else {
            return self.codec.run(input, output, end, limit);
        };
        if self.codec.done {
            // Finished: the same answer, or error, a single coding gives.
            return self.codec.run(input, output, end, limit);
        }
        let mut consumed = 0;
        let mut written = 0;
        loop {
            let mut progress = false;
            // Outermost first: each stage decodes what the one before it staged
            // (the first, the caller's input) into its own buffer. A stage's
            // input is complete once the stage feeding it is done.
            for i in 0..=earlier.len() {
                let (before, rest) = earlier.split_at_mut(i);
                let stage = match rest.first_mut() {
                    Some(stage) => stage,
                    None => &mut *last,
                };
                stage.compact();
                if stage.end == stage.buffer.len() {
                    continue;
                }
                let step = match before.last_mut() {
                    None => {
                        let step = stage.codec.run(
                            &input[consumed..],
                            &mut stage.buffer[stage.end..],
                            end,
                            limit,
                        )?;
                        consumed += step.consumed;
                        step
                    }
                    Some(feed) => {
                        let step = stage.codec.run(
                            &feed.buffer[feed.start..feed.end],
                            &mut stage.buffer[stage.end..],
                            feed.codec.done,
                            limit,
                        )?;
                        feed.start += step.consumed;
                        step
                    }
                };
                stage.end += step.written;
                progress |= step.consumed > 0 || step.written > 0;
            }
            // The innermost coding decodes the last stage into the caller's output.
            let step = self.codec.run(
                &last.buffer[last.start..last.end],
                &mut output[written..],
                last.codec.done,
                limit,
            )?;
            last.start += step.consumed;
            written += step.written;
            progress |= step.consumed > 0 || step.written > 0;
            if step.finished || written == output.len() || !progress {
                return Ok(DecodeStep {
                    consumed,
                    written,
                    finished: step.finished,
                    needs_input: !step.finished && written < output.len(),
                });
            }
        }
    }
}
fn gzip_header(input: &[u8]) -> Result<Option<usize>> {
    if input.len() < 10 {
        return Ok(None);
    }
    if input[..3] != [31, 139, 8] || input[3] & 0xe0 != 0 {
        return Err(corrupt());
    }
    let flags = input[3];
    let mut pos = 10;
    if flags & 4 != 0 {
        if input.len() < 12 {
            return Ok(None);
        }
        pos = 12 + u16::from_le_bytes(input[10..12].try_into().unwrap()) as usize;
    }
    for flag in [8, 16] {
        if flags & flag != 0 {
            if pos > input.len() {
                break;
            }
            if let Some(n) = input[pos..].iter().position(|b| *b == 0) {
                pos += n + 1;
            } else {
                if input.len() > 8192 {
                    return Err(corrupt());
                }
                return Ok(None);
            }
        }
    }
    if pos > 8192 {
        return Err(corrupt());
    }
    if input.len() < pos {
        return Ok(None);
    }
    if flags & 2 != 0 {
        if input.len() < pos + 2 {
            return Ok(None);
        }
        let mut crc = flate2::Crc::new();
        crc.update(&input[..pos]);
        if crc.sum() as u16 != u16::from_le_bytes(input[pos..pos + 2].try_into().unwrap()) {
            return Err(corrupt());
        }
        pos += 2;
    }
    Ok(Some(pos))
}
