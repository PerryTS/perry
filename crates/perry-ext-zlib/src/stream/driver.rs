//! Codec input and readable output belong to the stream, not a global queue.
//! `WouldBlock` means another write is needed; EOF means the caller ended it.
use super::*;
use std::io::{self, BufRead, ErrorKind};

/// A step-local borrow, cleared before the step returns. Only the two sniff
/// bytes may be retained. There is no native input queue or callback address.
struct Input {
    ptr: *const u8,
    len: usize,
    offset: usize,
    prefix: Vec<u8>,
    prefix_offset: usize,
    ended: bool,
    consumed: usize,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            ptr: std::ptr::null(),
            len: 0,
            offset: 0,
            prefix: Vec::new(),
            prefix_offset: 0,
            ended: false,
            consumed: 0,
        }
    }
}
impl Input {
    fn borrow(&mut self, bytes: &[u8], ended: bool) {
        assert!(self.ptr.is_null());
        self.ptr = bytes.as_ptr();
        self.len = bytes.len();
        self.offset = 0;
        self.consumed = 0;
        self.ended = ended;
    }
    fn clear_borrow(&mut self) -> usize {
        let n = self.consumed;
        self.ptr = std::ptr::null();
        self.len = 0;
        self.offset = 0;
        self.consumed = 0;
        n
    }
}
impl BufRead for Input {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.prefix_offset < self.prefix.len() {
            return Ok(&self.prefix[self.prefix_offset..]);
        }
        if self.offset < self.len {
            // SAFETY: only used inside step; no allocation on the JS heap,
            // safepoint or callback runs until clear_borrow resets this pointer.
            return Ok(unsafe {
                std::slice::from_raw_parts(self.ptr.add(self.offset), self.len - self.offset)
            });
        }
        if self.ended {
            Ok(&[])
        } else {
            Err(ErrorKind::WouldBlock.into())
        }
    }
    fn consume(&mut self, n: usize) {
        if self.prefix_offset < self.prefix.len() {
            self.prefix_offset += n;
            if self.prefix_offset == self.prefix.len() {
                self.prefix.clear();
                self.prefix_offset = 0;
            }
        } else {
            self.offset += n;
            self.consumed += n;
        }
    }
}
impl Read for Input {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let input = self.fill_buf()?;
        let n = input.len().min(out.len());
        out[..n].copy_from_slice(&input[..n]);
        self.consume(n);
        Ok(n)
    }
}

// Drive the raw engine even with no new input: a backreference can still
// have output after its final encoded byte was consumed. Treating that as
// WouldBlock would settle a write before all its output had been drained.
struct Inflate {
    input: Input,
    engine: Box<miniz_oxide::inflate::stream::InflateState>,
    finished: bool,
    zlib_header: bool,
}
impl Inflate {
    fn new(input: Input, zlib_header: bool) -> Self {
        Self {
            input,
            engine: miniz_oxide::inflate::stream::InflateState::new_boxed(if zlib_header {
                miniz_oxide::DataFormat::Zlib
            } else {
                miniz_oxide::DataFormat::Raw
            }),
            finished: false,
            zlib_header,
        }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished {
            return Ok(0);
        }
        loop {
            let input = match self.input.fill_buf() {
                Ok(input) => input,
                Err(e) if e.kind() == ErrorKind::WouldBlock => &[],
                Err(e) => return Err(e),
            };
            let result = miniz_oxide::inflate::stream::inflate(
                &mut self.engine,
                input,
                out,
                miniz_oxide::MZFlush::None,
            );
            let consumed = result.bytes_consumed;
            let written = result.bytes_written;
            let status = match result.status {
                Ok(s) => Some(s),
                Err(miniz_oxide::MZError::Buf) => None,
                Err(_) => {
                    let message = if self.engine.last_status()
                        == miniz_oxide::inflate::TINFLStatus::Adler32Mismatch
                    {
                        "incorrect data check"
                    } else if !self.zlib_header {
                        "invalid block type"
                    } else {
                        "incorrect header check"
                    };
                    return Err(io::Error::new(ErrorKind::InvalidData, message));
                }
            };
            self.input.consume(consumed);
            self.finished = status == Some(miniz_oxide::MZStatus::StreamEnd);
            if written > 0 || self.finished {
                return Ok(written);
            }
            if consumed == 0 {
                return Err(if self.input.ended {
                    ErrorKind::UnexpectedEof
                } else {
                    ErrorKind::WouldBlock
                }
                .into());
            }
        }
    }
}

/// Parse optional gzip headers without retaining filename/comment/extra data.
/// The header CRC is incremental; every header field can split across writes.
struct Header {
    fixed: [u8; 10],
    pos: usize,
    stage: u8,
    flags: u8,
    extra: usize,
    crc: flate2::Crc,
    check: [u8; 2],
}
impl Default for Header {
    fn default() -> Self {
        Self {
            fixed: [0; 10],
            pos: 0,
            stage: 0,
            flags: 0,
            extra: 0,
            crc: flate2::Crc::new(),
            check: [0; 2],
        }
    }
}
impl Header {
    fn read(&mut self, input: &mut Input) -> io::Result<()> {
        loop {
            match self.stage {
                0 => {
                    while self.pos < 10 {
                        let b = input.fill_buf()?;
                        if b.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let byte = b[0];
                        input.consume(1);
                        self.fixed[self.pos] = byte;
                        self.pos += 1;
                        self.crc.update(&[byte]);
                    }
                    if self.fixed[..3] != [31, 139, 8] || self.fixed[3] & 0xe0 != 0 {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "incorrect header check",
                        ));
                    }
                    self.flags = self.fixed[3];
                    self.pos = 0;
                    self.stage = 1;
                }
                1 => {
                    if self.flags & 4 == 0 {
                        self.stage = 3;
                        continue;
                    }
                    while self.pos < 2 {
                        let b = input.fill_buf()?;
                        if b.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let byte = b[0];
                        input.consume(1);
                        self.check[self.pos] = byte;
                        self.pos += 1;
                        self.crc.update(&[byte]);
                    }
                    self.extra = u16::from_le_bytes(self.check) as usize;
                    self.pos = 0;
                    self.stage = 2;
                }
                2 => {
                    while self.extra > 0 {
                        let b = input.fill_buf()?;
                        if b.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let n = b.len().min(self.extra);
                        self.crc.update(&b[..n]);
                        input.consume(n);
                        self.extra -= n;
                    }
                    self.stage = 3;
                }
                3 | 4 => {
                    let flag = if self.stage == 3 { 8 } else { 16 };
                    if self.flags & flag == 0 {
                        self.stage += 1;
                        continue;
                    }
                    loop {
                        let b = input.fill_buf()?;
                        if b.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let byte = b[0];
                        input.consume(1);
                        self.crc.update(&[byte]);
                        if byte == 0 {
                            break;
                        }
                    }
                    self.stage += 1;
                }
                5 => {
                    if self.flags & 2 == 0 {
                        return Ok(());
                    }
                    while self.pos < 2 {
                        let b = input.fill_buf()?;
                        if b.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let byte = b[0];
                        input.consume(1);
                        self.check[self.pos] = byte;
                        self.pos += 1;
                    }
                    if u16::from_le_bytes(self.check) != self.crc.sum() as u16 {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "header crc mismatch",
                        ));
                    }
                    self.stage = 6;
                }
                _ => return Ok(()),
            }
        }
    }
}

enum GzipStage {
    Header(Header, Input),
    Body(Inflate),
    Trailer(Input, [u8; 8], usize),
    Next(Input),
    Done(Input),
}
struct Gzip {
    stage: GzipStage,
    crc: flate2::Crc,
}
impl Gzip {
    fn new(input: Input) -> Self {
        Self {
            stage: GzipStage::Header(Header::default(), input),
            crc: flate2::Crc::new(),
        }
    }
    fn input(&mut self) -> &mut Input {
        match &mut self.stage {
            GzipStage::Header(_, input) => input,
            GzipStage::Body(d) => &mut d.input,
            GzipStage::Trailer(input, _, _) | GzipStage::Next(input) | GzipStage::Done(input) => {
                input
            }
        }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            match &mut self.stage {
                GzipStage::Header(header, input) => {
                    header.read(input)?;
                    let old = std::mem::replace(&mut self.stage, GzipStage::Done(Input::default()));
                    if let GzipStage::Header(_, input) = old {
                        self.stage = GzipStage::Body(Inflate::new(input, false));
                    }
                }
                GzipStage::Body(body) => {
                    let n = body.read(out)?;
                    if n > 0 {
                        self.crc.update(&out[..n]);
                        return Ok(n);
                    }
                    let old = std::mem::replace(&mut self.stage, GzipStage::Done(Input::default()));
                    if let GzipStage::Body(body) = old {
                        self.stage = GzipStage::Trailer(body.input, [0; 8], 0);
                    }
                }
                GzipStage::Trailer(input, trailer, pos) => {
                    while *pos < 8 {
                        let bytes = input.fill_buf()?;
                        if bytes.is_empty() {
                            return Err(ErrorKind::UnexpectedEof.into());
                        }
                        let n = bytes.len().min(8 - *pos);
                        trailer[*pos..*pos + n].copy_from_slice(&bytes[..n]);
                        input.consume(n);
                        *pos += n;
                    }
                    if u32::from_le_bytes(trailer[..4].try_into().unwrap()) != self.crc.sum() {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "incorrect data check",
                        ));
                    }
                    if u32::from_le_bytes(trailer[4..].try_into().unwrap()) != self.crc.amount() {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "incorrect length check",
                        ));
                    }
                    let input = std::mem::take(input);
                    self.crc.reset();
                    self.stage = GzipStage::Next(input);
                }
                GzipStage::Next(input) => {
                    let bytes = input.fill_buf()?;
                    if bytes.is_empty() || bytes[0] == 0 {
                        // Node permits zero padding after the last member.
                        self.stage = GzipStage::Done(std::mem::take(input));
                    } else {
                        self.stage = GzipStage::Header(Header::default(), std::mem::take(input));
                    }
                }
                GzipStage::Done(_) => return Ok(0),
            }
        }
    }
}

type BrotliState = brotli::BrotliState<
    allocation::CountingAlloc,
    allocation::CountingAlloc,
    allocation::CountingAlloc,
>;
struct Brotli {
    input: Input,
    state: Box<BrotliState>,
    total_out: usize,
    allocated: allocation::CountingAlloc,
    finished: bool,
}
impl Brotli {
    fn new(input: Input) -> Self {
        let allocated = allocation::CountingAlloc::default();
        Self {
            input,
            state: Box::new(BrotliState::new(
                allocated.clone(),
                allocated.clone(),
                allocated.clone(),
            )),
            allocated,
            total_out: 0,
            finished: false,
        }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished {
            return Ok(0);
        }
        loop {
            let bytes = match self.input.fill_buf() {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == ErrorKind::WouldBlock => &[],
                Err(e) => return Err(e),
            };
            let mut available_in = bytes.len();
            let mut input_offset = 0;
            let mut available_out = out.len();
            let mut written = 0;
            let status = brotli::BrotliDecompressStream(
                &mut available_in,
                &mut input_offset,
                bytes,
                &mut available_out,
                &mut written,
                out,
                &mut self.total_out,
                &mut self.state,
            );
            self.input.consume(input_offset);
            self.finished = matches!(status, brotli::BrotliResult::ResultSuccess);
            if matches!(status, brotli::BrotliResult::ResultFailure) {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "Decompression failed",
                ));
            }
            if written > 0 || self.finished {
                return Ok(written);
            }
            if input_offset == 0 {
                return Err(if self.input.ended {
                    ErrorKind::UnexpectedEof
                } else {
                    ErrorKind::WouldBlock
                }
                .into());
            }
        }
    }
}
struct Zstd {
    input: Input,
    engine: zstd::zstd_safe::DCtx<'static>,
    boundary: bool,
    last_error: usize,
}
impl Zstd {
    fn new(input: Input) -> io::Result<Self> {
        Ok(Self {
            input,
            engine: {
                let mut ctx = zstd::zstd_safe::DCtx::create();
                ctx.init().map_err(zstd_error)?;
                ctx
            },
            boundary: false,
            last_error: 0,
        })
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            let bytes = match self.input.fill_buf() {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == ErrorKind::WouldBlock => &[],
                Err(e) => return Err(e),
            };
            if bytes.is_empty() && self.boundary {
                return if self.input.ended {
                    Ok(0)
                } else {
                    Err(ErrorKind::WouldBlock.into())
                };
            }
            let mut input = zstd::zstd_safe::InBuffer { src: bytes, pos: 0 };
            let mut output = zstd::zstd_safe::OutBuffer::around(out);
            let remaining = match self.engine.decompress_stream(&mut output, &mut input) {
                Ok(n) => n,
                Err(code) => {
                    self.last_error = code;
                    return Err(zstd_error(code));
                }
            };
            let consumed = input.pos;
            let written = output.pos();
            self.input.consume(consumed);
            self.boundary = remaining == 0;
            if written > 0 {
                return Ok(written);
            }
            if self.boundary {
                continue;
            }
            if consumed == 0 {
                return Err(if self.input.ended {
                    ErrorKind::UnexpectedEof
                } else {
                    ErrorKind::WouldBlock
                }
                .into());
            }
        }
    }
}

enum Decoder {
    Gzip(Gzip),
    Zlib(Inflate),
    Raw(Inflate),
    Brotli(Brotli),
    Zstd(Zstd),
    Sniff(Input),
}
impl Decoder {
    fn new(codec: Codec) -> Option<Self> {
        let input = Input::default();
        Some(match codec {
            Codec::Gunzip => Self::Gzip(Gzip::new(input)),
            Codec::Inflate => Self::Zlib(Inflate::new(input, true)),
            Codec::InflateRaw => Self::Raw(Inflate::new(input, false)),
            Codec::BrotliDecompress => Self::Brotli(Brotli::new(input)),
            Codec::ZstdDecompress => Self::Zstd(Zstd::new(input).ok()?),
            Codec::Unzip => Self::Sniff(input),
            _ => return None,
        })
    }
    fn input(&mut self) -> &mut Input {
        match self {
            Self::Gzip(d) => d.input(),
            Self::Zlib(d) => &mut d.input,
            Self::Raw(d) => &mut d.input,
            Self::Brotli(d) => &mut d.input,
            Self::Zstd(d) => &mut d.input,
            Self::Sniff(input) => input,
        }
    }
    fn native_bytes(&self) -> usize {
        let prefix = match self {
            Self::Gzip(g) => match &g.stage {
                GzipStage::Header(_, i)
                | GzipStage::Trailer(i, _, _)
                | GzipStage::Next(i)
                | GzipStage::Done(i) => i.prefix.capacity(),
                GzipStage::Body(i) => i.input.prefix.capacity(),
            },
            Self::Zlib(i) | Self::Raw(i) => i.input.prefix.capacity(),
            Self::Brotli(i) => i.input.prefix.capacity(),
            Self::Zstd(i) => i.input.prefix.capacity(),
            Self::Sniff(i) => i.prefix.capacity(),
        };
        prefix
            + match self {
                Self::Gzip(g) => {
                    if matches!(g.stage, GzipStage::Body(_)) {
                        allocation::inflate_bytes()
                    } else {
                        0
                    }
                }
                Self::Zlib(_) | Self::Raw(_) => allocation::inflate_bytes(),
                Self::Brotli(b) => {
                    std::mem::size_of::<BrotliState>()
                        + b.allocated.0.get()
                        + 3 * std::mem::size_of::<usize>()
                }
                Self::Zstd(z) => z.engine.sizeof(),
                Self::Sniff(_) => 0,
            }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Self::Sniff(input) = self {
            while input.prefix.len() < 2 && input.offset < input.len {
                input.prefix.push(unsafe { *input.ptr.add(input.offset) });
                input.offset += 1;
                input.consumed += 1;
            }
            if input.prefix.len() < 2 && !input.ended {
                return Err(ErrorKind::WouldBlock.into());
            }
            let gzip = input.prefix == [0x1f, 0x8b];
            let input = std::mem::take(input);
            *self = if gzip {
                Self::Gzip(Gzip::new(input))
            } else {
                Self::Zlib(Inflate::new(input, true))
            };
        }
        match self {
            Self::Gzip(d) => d.read(out),
            Self::Zlib(d) => d.read(out),
            Self::Raw(d) => d.read(out),
            Self::Brotli(d) => d.read(out),
            Self::Zstd(d) => d.read(out),
            Self::Sniff(_) => unreachable!(),
        }
    }
}

fn zstd_error(code: usize) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        zstd::zstd_safe::get_error_name(code),
    )
}

enum Encoder {
    Flate(super::zlib_encoder::Encoder),
    Brotli(
        Box<brotli::enc::encode::BrotliEncoderStateStruct<allocation::CountingAlloc>>,
        allocation::CountingAlloc,
    ),
    Zstd(zstd::zstd_safe::CCtx<'static>),
}
impl Encoder {
    fn new(codec: Codec, level: Compression) -> io::Result<Self> {
        Ok(match codec {
            Codec::Gzip | Codec::Deflate | Codec::DeflateRaw => {
                Self::Flate(super::zlib_encoder::Encoder::new(codec, level)?)
            }
            Codec::BrotliCompress => {
                let alloc = allocation::CountingAlloc::default();
                let mut state = Box::new(brotli::enc::encode::BrotliEncoderStateStruct::new(
                    alloc.clone(),
                ));
                state.params.quality = 11;
                state.params.lgwin = 22;
                Self::Brotli(state, alloc)
            }
            Codec::ZstdCompress => {
                let mut ctx = zstd::zstd_safe::CCtx::create();
                ctx.init(ZSTD_DEFAULT_LEVEL).map_err(zstd_error)?;
                Self::Zstd(ctx)
            }
            _ => {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    "decoder used as encoder",
                ))
            }
        })
    }
    fn native_bytes(&self) -> usize {
        match self {
            Self::Flate(f) => f.native_bytes(),
            Self::Brotli(state, alloc) => {
                std::mem::size_of_val(&**state) + alloc.0.get() + 3 * std::mem::size_of::<usize>()
            }
            Self::Zstd(ctx) => ctx.sizeof(),
        }
    }
    fn step(
        &mut self,
        op: &perry_ffi::native_stream::StepIn,
        bytes: &[u8],
        output: &mut [u8],
    ) -> io::Result<(usize, usize, perry_ffi::native_stream::StepStatus)> {
        use perry_ffi::native_stream::{StepStatus, StreamOp};
        let final_op = op.op == StreamOp::FINAL;
        match self {
            Self::Flate(f) => f.step(op, bytes, output),
            Self::Brotli(state, _) => {
                use brotli::enc::encode::BrotliEncoderOperation;
                let operation = if final_op {
                    BrotliEncoderOperation::BROTLI_OPERATION_FINISH
                } else if op.op == StreamOp::FLUSH {
                    BrotliEncoderOperation::BROTLI_OPERATION_FLUSH
                } else {
                    BrotliEncoderOperation::BROTLI_OPERATION_PROCESS
                };
                let mut available_in = bytes.len();
                let mut input_offset = 0;
                let mut available_out = output.len();
                let mut output_offset = 0;
                let ok = state.compress_stream(
                    operation,
                    &mut available_in,
                    bytes,
                    &mut input_offset,
                    &mut available_out,
                    output,
                    &mut output_offset,
                    &mut None,
                    &mut |_, _, _, _| {},
                );
                if !ok {
                    return Err(io::Error::new(ErrorKind::InvalidData, "Compression failed"));
                }
                let status = if final_op && state.is_finished() {
                    StepStatus::ENDED
                } else if available_in > 0
                    || state.has_more_output()
                    || (final_op && !state.is_finished())
                {
                    StepStatus::MORE
                } else {
                    StepStatus::NEED_INPUT
                };
                Ok((input_offset, output_offset, status))
            }
            Self::Zstd(ctx) => {
                use zstd::zstd_safe::{zstd_sys::ZSTD_EndDirective, InBuffer, OutBuffer};
                let directive = if final_op {
                    ZSTD_EndDirective::ZSTD_e_end
                } else if op.op == StreamOp::FLUSH {
                    ZSTD_EndDirective::ZSTD_e_flush
                } else {
                    ZSTD_EndDirective::ZSTD_e_continue
                };
                let mut input = InBuffer { src: bytes, pos: 0 };
                let mut out = OutBuffer::around(output);
                let remaining = ctx
                    .compress_stream2(&mut out, &mut input, directive)
                    .map_err(zstd_error)?;
                let status = if final_op && remaining == 0 {
                    StepStatus::ENDED
                } else if input.pos < bytes.len()
                    || out.pos() == out.capacity()
                    || (op.op != StreamOp::WRITE && remaining > 0)
                {
                    StepStatus::MORE
                } else {
                    StepStatus::NEED_INPUT
                };
                Ok((input.pos, out.pos(), status))
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static PAYLOAD_COUNTS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}
#[cfg(test)]
impl Drop for Payload {
    fn drop(&mut self) {
        PAYLOAD_COUNTS.with(|n| {
            let (created, dropped) = n.get();
            n.set((created, dropped + 1));
        });
    }
}
/// Only codec state and scratch: the runtime owns all records and queues.
pub(super) struct Payload {
    codec: Codec,
    level: Compression,
    decoder: Option<Decoder>,
    encoder: Option<Encoder>,
    scratch: Vec<u8>,
    output_offset: usize,
    bytes_written: usize,
    error: Option<String>,
}
impl Payload {
    pub(super) fn new(codec: Codec, level: Compression, chunk_size: usize) -> io::Result<Self> {
        #[cfg(test)]
        let chunk_size = if sabotage("unbounded_output") {
            chunk_size * 16384
        } else {
            chunk_size
        };
        let decoder = Decoder::new(codec);
        let encoder = if decoder.is_none() {
            Some(Encoder::new(codec, level)?)
        } else {
            None
        };
        #[cfg(test)]
        PAYLOAD_COUNTS.with(|n| {
            let (created, dropped) = n.get();
            n.set((created + 1, dropped));
        });
        Ok(Self {
            codec,
            level,
            decoder,
            encoder,
            scratch: vec![0; chunk_size],
            output_offset: 0,
            bytes_written: 0,
            error: None,
        })
    }
    pub(super) fn external_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.scratch.capacity()
            + self.decoder.as_ref().map_or(0, Decoder::native_bytes)
            + self.encoder.as_ref().map_or(0, Encoder::native_bytes)
            + self.error.as_ref().map_or(0, String::capacity)
    }
    pub(super) fn step(
        &mut self,
        op: &perry_ffi::native_stream::StepIn,
        out: &mut perry_ffi::native_stream::StepOut,
    ) {
        use perry_ffi::native_stream::{StepStatus, StreamOp};
        let bytes = if op.len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(op.input, op.len) }
        };
        let start = self.output_offset;
        let result = if let Some(decoder) = &mut self.decoder {
            decoder.input().borrow(bytes, op.op == StreamOp::FINAL);
            let result = decoder.read(&mut self.scratch[start..]);
            let consumed = decoder.input().clear_borrow();
            match result {
                Ok(n) => Ok((
                    consumed,
                    n,
                    if n > 0 {
                        StepStatus::MORE
                    } else if op.op == StreamOp::FINAL {
                        StepStatus::ENDED
                    } else {
                        StepStatus::NEED_INPUT
                    },
                )),
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    Ok((consumed, 0, StepStatus::NEED_INPUT))
                }
                Err(e) => Err(e),
            }
        } else {
            self.encoder
                .as_mut()
                .unwrap()
                .step(op, bytes, &mut self.scratch[start..])
        };
        match result {
            Ok((consumed, written, status)) => {
                out.consumed = consumed;
                out.out = unsafe { self.scratch.as_ptr().add(start) };
                self.output_offset = (start + written) % self.scratch.len();
                out.out_len = written;
                out.status = status;
                self.bytes_written += consumed;
            }
            Err(e) => {
                out.code = if e.kind() == ErrorKind::UnexpectedEof {
                    5
                } else {
                    3
                };
                out.status = StepStatus::ERROR;
                self.error = Some(e.to_string());
            }
        }
        #[cfg(test)]
        if sabotage("corrupt_output") && out.out_len > 0 {
            self.scratch[start] ^= 1;
        }
        out.external_bytes = self.external_bytes();
    }
    pub(super) fn bytes_written(&self) -> usize {
        self.bytes_written
    }
    pub(super) fn error_details(&self, code: u32) -> (String, String, i32) {
        let message = self
            .error
            .clone()
            .unwrap_or_else(|| "Decompression failed".into());
        if let Some(Decoder::Brotli(b)) = &self.decoder {
            if (b.state.error_code as i32) < 0 {
                let name = format!("{:?}", b.state.error_code);
                return (
                    message,
                    format!("ERR__{}", name.strip_prefix("BROTLI_DECODER_").unwrap()),
                    b.state.error_code as i32,
                );
            }
        }
        if let Some(Decoder::Zstd(z)) = &self.decoder {
            if z.last_error != 0 {
                let code = unsafe { zstd::zstd_safe::zstd_sys::ZSTD_getErrorCode(z.last_error) };
                return (message, format!("{code:?}"), code as i32);
            }
        }
        (
            message,
            if code == 5 {
                "Z_BUF_ERROR"
            } else {
                "Z_DATA_ERROR"
            }
            .into(),
            -(code as i32),
        )
    }
    pub(super) fn params(&mut self, level: Compression, strategy: i32) {
        self.level = level;
        if let Some(Encoder::Flate(f)) = &mut self.encoder {
            f.params(level, strategy);
        }
    }
    pub(super) fn reset(&mut self) -> io::Result<()> {
        *self = Self::new(self.codec, self.level, self.scratch.len())?;
        Ok(())
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        if let Self::Brotli(state, _) = self {
            brotli::enc::encode::BrotliEncoderDestroyInstance(state);
        }
    }
}
