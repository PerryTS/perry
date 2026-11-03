//! Dictionary-bearing zlib encoder. Ordinary encodes retain their old backend.
//! The stream and all C working allocations use the payload's stable ledger.
use super::allocation::{out_of_memory, Placed};
use super::inflate_context::{allocate, release};
use super::zsys as z;
use perry_ffi::native_payload::buffer::BufferOwner;
use perry_ffi::native_stream::{StepIn, StepStatus, StreamOp};
use std::io::{self, ErrorKind};

pub(super) struct Context(Placed<z::z_stream>, Option<(i32, i32)>);
impl Context {
    pub(super) fn new(owner: &BufferOwner, level: u32, dictionary: &[u8]) -> io::Result<Self> {
        let len = u32::try_from(dictionary.len())
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "dictionary too large"))?;
        let state = z::z_stream {
            next_in: std::ptr::null_mut(),
            avail_in: 0,
            total_in: 0,
            next_out: std::ptr::null_mut(),
            avail_out: 0,
            total_out: 0,
            msg: std::ptr::null_mut(),
            state: std::ptr::null_mut(),
            zalloc: allocate,
            zfree: release,
            opaque: owner.hook().opaque,
            data_type: 0,
            adler: 0,
            reserved: 0,
        };
        let mut context = Self(
            Placed::try_new(owner, state).ok_or_else(out_of_memory)?,
            None,
        );
        let status = unsafe { z::zng_deflateInit(&mut *context.0, level as i32) };
        context.check(status)?;
        let status = unsafe { z::deflateSetDictionary(&mut *context.0, dictionary.as_ptr(), len) };
        context.check(status)?;
        Ok(context)
    }
    fn check(&self, status: i32) -> io::Result<()> {
        if status == z::Z_OK {
            return Ok(());
        }
        if status == z::Z_MEM_ERROR {
            return Err(out_of_memory());
        }
        Err(io::Error::new(
            ErrorKind::InvalidData,
            "dictionary compression failed",
        ))
    }
    fn process(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        flush: i32,
    ) -> io::Result<(usize, usize, bool)> {
        let stream = &mut *self.0;
        let input_len = input.len().min(u32::MAX as usize) as u32;
        let output_len = output.len().min(u32::MAX as usize) as u32;
        stream.next_in = std::ptr::null_mut();
        stream.avail_in = 0;
        stream.next_out = output.as_mut_ptr();
        stream.avail_out = output_len;
        // deflateParams can emit a block while changing strategy. Give it the
        // caller's output space, but never let it consume the next JS write.
        let parameters = if let Some((level, strategy)) = self.1 {
            let status = unsafe { z::deflateParams(stream, level, strategy) };
            if status == z::Z_OK {
                self.1 = None;
            }
            status
        } else {
            z::Z_OK
        };
        let status = if parameters == z::Z_OK {
            stream.next_in = input.as_ptr().cast_mut();
            stream.avail_in = input_len;
            unsafe { z::deflate(stream, flush) }
        } else {
            parameters
        };
        let consumed = if parameters == z::Z_OK {
            (input_len - stream.avail_in) as usize
        } else {
            0
        };
        let written = (output_len - stream.avail_out) as usize;
        // No borrowed pointer survives a native step, including errors.
        stream.next_in = std::ptr::null_mut();
        stream.avail_in = 0;
        stream.next_out = std::ptr::null_mut();
        stream.avail_out = 0;
        if !matches!(status, z::Z_OK | z::Z_BUF_ERROR | z::Z_STREAM_END) {
            self.check(status)?;
        }
        Ok((consumed, written, status == z::Z_STREAM_END))
    }
    pub(super) fn params(&mut self, level: u32, strategy: i32) {
        self.1 = Some((level as i32, strategy));
    }
    pub(super) fn step(
        &mut self,
        op: &StepIn,
        input: &[u8],
        output: &mut [u8],
    ) -> io::Result<(usize, usize, StepStatus)> {
        let flush = if op.op == StreamOp::FINAL {
            z::Z_FINISH
        } else if op.op == StreamOp::FLUSH {
            op.flush_kind as i32
        } else {
            z::Z_NO_FLUSH
        };
        let (consumed, written, ended) = self.process(input, output, flush)?;
        let status = if ended {
            StepStatus::ENDED
        } else if consumed < input.len()
            || written == output.len()
            || flush == z::Z_FINISH
            || self.1.is_some()
        {
            StepStatus::MORE
        } else {
            StepStatus::NEED_INPUT
        };
        Ok((consumed, written, status))
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { z::deflateEnd(&mut *self.0) };
    }
}
pub(crate) fn encode_bytes(
    input: &[u8],
    level: flate2::Compression,
    dictionary: &[u8],
) -> io::Result<Vec<u8>> {
    let owner = BufferOwner::new();
    let mut context = Context::new(&owner, level.level(), dictionary)?;
    let mut offset = 0;
    let mut result = Vec::new();
    let mut output = [0; 16384];
    loop {
        let (consumed, written, ended) =
            context.process(&input[offset..], &mut output, z::Z_FINISH)?;
        offset += consumed;
        result.extend_from_slice(&output[..written]);
        if ended {
            return Ok(result);
        }
        if consumed == 0 && written == 0 {
            return Err(io::Error::new(
                ErrorKind::WriteZero,
                "dictionary compression made no progress",
            ));
        }
    }
}
