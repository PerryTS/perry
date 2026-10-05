//! Codec input and readable output belong to the stream, not a global queue.
//! `WouldBlock` means another write is needed; EOF means the caller ended it.
use super::*;
use std::io::{self, BufRead, ErrorKind};

#[derive(Default)]
struct Input {
    bytes: VecDeque<Vec<u8>>,
    offset: usize,
    ended: bool,
    consumed: usize,
}
impl BufRead for Input {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self.bytes.front() {
            Some(bytes) => Ok(&bytes[self.offset..]),
            None if self.ended => Ok(&[]),
            None => Err(ErrorKind::WouldBlock.into()),
        }
    }
    fn consume(&mut self, n: usize) {
        self.offset += n;
        self.consumed += n;
        if self.bytes.front().is_some_and(|b| self.offset == b.len()) {
            self.bytes.pop_front(); // release each consumed compressed write
            self.offset = 0;
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
    engine: flate2::Decompress,
    finished: bool,
}
impl Inflate {
    fn new(input: Input, zlib_header: bool) -> Self {
        Self {
            input,
            engine: flate2::Decompress::new(zlib_header),
            finished: false,
        }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.finished {
            return Ok(0);
        }
        loop {
            let before_in = self.engine.total_in();
            let before_out = self.engine.total_out();
            let input = match self.input.fill_buf() {
                Ok(input) => input,
                Err(e) if e.kind() == ErrorKind::WouldBlock => &[],
                Err(e) => return Err(e),
            };
            let status = self
                .engine
                .decompress(input, out, flate2::FlushDecompress::None)
                .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?;
            let consumed = (self.engine.total_in() - before_in) as usize;
            let written = (self.engine.total_out() - before_out) as usize;
            self.input.consume(consumed);
            self.finished = status == flate2::Status::StreamEnd;
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

enum GzipStage {
    Header(flate2::bufread::GzDecoder<Input>),
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
            stage: GzipStage::Header(flate2::bufread::GzDecoder::new(input)),
            crc: flate2::Crc::new(),
        }
    }
    fn input(&mut self) -> &mut Input {
        match &mut self.stage {
            GzipStage::Header(d) => d.get_mut(),
            GzipStage::Body(d) => &mut d.input,
            GzipStage::Trailer(input, _, _) | GzipStage::Next(input) | GzipStage::Done(input) => {
                input
            }
        }
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            match &mut self.stage {
                GzipStage::Header(header) => {
                    // flate2 parses the optional fields/header CRC without
                    // decoding a body when the output slice is empty.
                    let consumed = header.read(&mut [])?;
                    if consumed != 0 {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            "gzip header decoder wrote bytes to an empty buffer",
                        ));
                    }
                    let old = std::mem::replace(&mut self.stage, GzipStage::Done(Input::default()));
                    if let GzipStage::Header(header) = old {
                        self.stage = GzipStage::Body(Inflate::new(header.into_inner(), false));
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
                        self.stage = GzipStage::Header(flate2::bufread::GzDecoder::new(
                            std::mem::take(input),
                        ));
                    }
                }
                GzipStage::Done(_) => return Ok(0),
            }
        }
    }
}

type BrotliState = brotli::BrotliState<
    brotli::enc::StandardAlloc,
    brotli::enc::StandardAlloc,
    brotli::enc::StandardAlloc,
>;
struct Brotli {
    input: Input,
    state: Box<BrotliState>,
    total_out: usize,
    finished: bool,
}
impl Brotli {
    fn new(input: Input) -> Self {
        Self {
            input,
            state: Box::new(BrotliState::new(
                Default::default(),
                Default::default(),
                Default::default(),
            )),
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
    engine: zstd::stream::raw::Decoder<'static>,
    boundary: bool,
}
impl Zstd {
    fn new(input: Input) -> io::Result<Self> {
        Ok(Self {
            input,
            engine: zstd::stream::raw::Decoder::new()?,
            boundary: false,
        })
    }
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        use zstd::stream::raw::Operation;
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
            let step = self.engine.run_on_buffers(bytes, out)?;
            self.input.consume(step.bytes_read);
            self.boundary = step.remaining == 0;
            if step.bytes_written > 0 {
                return Ok(step.bytes_written);
            }
            if self.boundary {
                continue;
            }
            if step.bytes_read == 0 {
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
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Self::Sniff(input) = self {
            let prefix: Vec<u8> = input
                .bytes
                .iter()
                .flat_map(|b| b.iter())
                .take(2)
                .copied()
                .collect();
            if prefix.len() < 2 && !input.ended {
                return Err(ErrorKind::WouldBlock.into());
            }
            let input = std::mem::take(input);
            *self = if prefix == [0x1f, 0x8b] {
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

pub(super) enum Work {
    Write(Vec<u8>, usize, i64),
    Flush(i64),
    End,
}

pub(super) struct Driver {
    decoder: Option<Decoder>,
    pub(super) work: VecDeque<Work>,
    pub(super) output: VecDeque<Vec<u8>>,
    pub(super) output_bytes: usize,
    pub(super) input_bytes: usize,
    pub(super) chunk_size: usize,
    pub(super) readable_hwm: usize,
    pub(super) writable_hwm: usize,
    pub(super) flowing: bool,
    pub(super) paused: bool,
    pub(super) scheduled: bool,
    pub(super) done: bool,
    pub(super) need_drain: bool,
    pub(super) pipe_waiters: usize,
    active_write: Option<i64>,
    encoded: Vec<u8>,
    encoded_offset: usize,
    finishing: bool,
}
impl Driver {
    pub(super) fn new(
        codec: Codec,
        chunk_size: usize,
        readable_hwm: usize,
        writable_hwm: usize,
    ) -> Self {
        Self {
            decoder: Decoder::new(codec),
            work: VecDeque::new(),
            output: VecDeque::new(),
            output_bytes: 0,
            input_bytes: 0,
            chunk_size,
            readable_hwm,
            writable_hwm,
            flowing: false,
            paused: false,
            scheduled: false,
            done: false,
            need_drain: false,
            pipe_waiters: 0,
            active_write: None,
            encoded: Vec::new(),
            encoded_offset: 0,
            finishing: false,
        }
    }
    pub(super) fn is_decoder(&self) -> bool {
        self.decoder.is_some()
    }
    pub(super) fn scan(&mut self, visitor: &mut GcRootVisitor<'_>) {
        if let Some(cb) = &mut self.active_write {
            visitor.visit_i64_slot(cb);
        }
        for work in &mut self.work {
            match work {
                Work::Write(_, _, cb) | Work::Flush(cb) => {
                    visitor.visit_i64_slot(cb);
                }
                Work::End => {}
            }
        }
    }
    fn emit_encoded(&mut self) {
        let end = (self.encoded_offset + self.chunk_size).min(self.encoded.len());
        let chunk = self.encoded[self.encoded_offset..end].to_vec();
        self.output_bytes += chunk.len();
        self.output.push_back(chunk);
        self.encoded_offset = end;
        if end == self.encoded.len() {
            self.encoded = Vec::new();
            self.encoded_offset = 0;
        }
    }
    fn queue_output(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        self.encoded = bytes;
        self.encoded_offset = 0;
        self.emit_encoded();
    }
    pub(super) fn cancel_callbacks(&mut self) -> Vec<i64> {
        let mut callbacks = Vec::new();
        if let Some(cb) = self.active_write.take() {
            if cb != 0 {
                callbacks.push(cb);
            }
        }
        for work in &self.work {
            if let Work::Write(_, _, cb) | Work::Flush(cb) = work {
                if *cb != 0 {
                    callbacks.push(*cb);
                }
            }
        }
        callbacks
    }
    fn complete_write(&mut self, handle: i64, events: &mut Vec<ZlibEvent>) {
        if let Some(cb) = self.active_write.take() {
            if cb != 0 {
                events.push(ZlibEvent::Callback(cb));
            }
            if self.input_bytes == 0 && self.need_drain {
                self.need_drain = false;
                events.push(ZlibEvent::Drain(handle));
            }
        }
    }
    /// Do at most one output allocation. Never run the codec with a full
    /// readable queue, including when a consumer pauses from a data callback.
    pub(super) fn produce(
        &mut self,
        s: &mut Option<CodecState>,
        handle: i64,
        bytes_written: &mut usize,
    ) -> Result<Vec<ZlibEvent>, String> {
        let mut events = Vec::new();
        if self.output_bytes >= self.readable_hwm.max(1) {
            return Ok(events);
        }
        if !self.encoded.is_empty() {
            self.emit_encoded();
            return Ok(events);
        }
        if self.done {
            return Ok(events);
        }
        if self.finishing && self.decoder.is_none() {
            self.done = true;
            events.push(ZlibEvent::Finish(handle));
            return Ok(events);
        }
        loop {
            if self.decoder.is_none() {
                self.complete_write(handle, &mut events);
            }
            if let Some(decoder) = &mut self.decoder {
                let mut out = vec![0; self.chunk_size];
                let before = decoder.input().consumed;
                let result = decoder.read(&mut out);
                let consumed = decoder.input().consumed - before;
                self.input_bytes = self.input_bytes.saturating_sub(consumed);
                *bytes_written += consumed;
                if matches!(result, Ok(0)) {
                    let input = decoder.input();
                    let unused: usize = input
                        .bytes
                        .iter()
                        .map(Vec::len)
                        .sum::<usize>()
                        .saturating_sub(input.offset);
                    input.bytes.clear();
                    input.offset = 0;
                    self.input_bytes = self.input_bytes.saturating_sub(unused);
                }
                match result {
                    Ok(n) if n > 0 => {
                        out.truncate(n);
                        // Small compressed writes can produce tiny outputs.
                        // Their pending storage must not retain chunkSize bytes
                        // each while accounting only for the delivered length.
                        out.shrink_to_fit();
                        self.output_bytes += n;
                        self.output.push_back(out);
                        return Ok(events);
                    }
                    Ok(_) if self.finishing => {
                        self.decoder = None; // release codec workspace at completion
                        self.done = true;
                        self.complete_write(handle, &mut events);
                        events.push(ZlibEvent::Finish(handle));
                        return Ok(events);
                    }
                    Ok(_) => {} // complete frame; writes/End still settle in order
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => {
                        return Err(if e.kind() == ErrorKind::UnexpectedEof {
                            "unexpected end of file".to_string()
                        } else {
                            e.to_string()
                        })
                    }
                }
                self.complete_write(handle, &mut events);
            }
            match self.work.pop_front() {
                Some(Work::Write(bytes, offset, cb)) => {
                    if let Some(decoder) = &mut self.decoder {
                        if !bytes.is_empty() {
                            decoder.input().bytes.push_back(bytes);
                        }
                        self.active_write = Some(cb);
                    } else {
                        // Feeding bounded input also prevents an encoder's Vec
                        // from growing to the size of a single huge write.
                        let end = (offset + self.chunk_size).min(bytes.len());
                        let cs = s.as_mut().ok_or("codec closed")?;
                        cs.write_chunk(&bytes[offset..end])
                            .map_err(|e| e.to_string())?;
                        self.input_bytes -= end - offset;
                        *bytes_written += end - offset;
                        let out = cs.drain();
                        if end < bytes.len() {
                            self.work.push_front(Work::Write(bytes, end, cb));
                        } else {
                            self.active_write = Some(cb);
                        }
                        if !out.is_empty() {
                            self.queue_output(out);
                            return Ok(events);
                        }
                    }
                }
                Some(Work::Flush(cb)) => {
                    if let Some(cs) = s.as_mut() {
                        cs.flush_codec().map_err(|e| e.to_string())?;
                        self.queue_output(cs.drain());
                    }
                    if cb != 0 {
                        events.push(ZlibEvent::Callback(cb));
                    }
                    return Ok(events);
                }
                Some(Work::End) => {
                    self.finishing = true;
                    if let Some(decoder) = &mut self.decoder {
                        decoder.input().ended = true;
                    } else {
                        self.queue_output(
                            s.take()
                                .ok_or("codec closed")?
                                .finish()
                                .map_err(|e| e.to_string())?,
                        );
                        return Ok(events);
                    }
                }
                None => return Ok(events),
            }
        }
    }
    pub(super) fn can_progress(&self) -> bool {
        (self.flowing && self.pipe_waiters == 0 && !self.output.is_empty())
            || (!self.done
                && self.output_bytes < self.readable_hwm.max(1)
                && (self.active_write.is_some()
                    || self.finishing
                    || !self.work.is_empty()
                    || !self.encoded.is_empty()))
            || (self.done && self.output.is_empty() && self.flowing)
    }
}
