//! The zlib-compatible encoder, with every native allocation counted. The
//! C API is used only here to supply its allocator; stream
//! input pointers are cleared before returning to the runtime.
use super::*;
use libz_sys::{gz_header, z_stream};
use std::{
    alloc::{alloc_zeroed, dealloc, Layout},
    cell::Cell,
};

const ALIGN: usize = 64;
unsafe extern "C" fn allocate(opaque: *mut c_void, items: u32, size: u32) -> *mut c_void {
    let Some(bytes) = (items as usize)
        .checked_mul(size as usize)
        .and_then(|n| n.checked_add(ALIGN))
    else {
        return std::ptr::null_mut();
    };
    let Ok(layout) = Layout::from_size_align(bytes, ALIGN) else {
        return std::ptr::null_mut();
    };
    let base = alloc_zeroed(layout);
    if base.is_null() {
        return base.cast();
    }
    base.cast::<usize>().write(bytes);
    let count = &*(opaque as *const Cell<usize>);
    count.set(count.get() + bytes);
    base.add(ALIGN).cast()
}
unsafe extern "C" fn free(opaque: *mut c_void, ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    let base = ptr.cast::<u8>().sub(ALIGN);
    let bytes = base.cast::<usize>().read();
    let count = &*(opaque as *const Cell<usize>);
    count.set(count.get() - bytes);
    dealloc(base, Layout::from_size_align_unchecked(bytes, ALIGN));
}

pub(super) struct Encoder {
    stream: Box<z_stream>,
    allocated: Box<Cell<usize>>,
    header: Option<Box<gz_header>>,
}
impl Encoder {
    pub(super) fn new(codec: Codec, level: Compression) -> std::io::Result<Self> {
        let allocated = Box::new(Cell::new(0));
        let mut stream = Box::new(z_stream {
            next_in: std::ptr::null_mut(),
            avail_in: 0,
            total_in: 0,
            next_out: std::ptr::null_mut(),
            avail_out: 0,
            total_out: 0,
            msg: std::ptr::null_mut(),
            state: std::ptr::null_mut(),
            zalloc: allocate,
            zfree: free,
            opaque: (&*allocated as *const Cell<usize>).cast_mut().cast(),
            data_type: 0,
            adler: 0,
            reserved: 0,
        });
        let window_bits = match codec {
            Codec::Gzip => 31,
            Codec::DeflateRaw => -15,
            _ => 15,
        };
        let status = unsafe {
            libz_sys::deflateInit2_(
                &mut *stream,
                level.level() as i32,
                libz_sys::Z_DEFLATED,
                window_bits,
                8,
                libz_sys::Z_DEFAULT_STRATEGY,
                libz_sys::zlibVersion(),
                std::mem::size_of::<z_stream>() as i32,
            )
        };
        if status != libz_sys::Z_OK {
            return Err(std::io::Error::other(format!("deflate init: {status}")));
        }
        let mut result = Self {
            stream,
            allocated,
            header: None,
        };
        if matches!(codec, Codec::Gzip) {
            let mut header: Box<gz_header> = Box::new(unsafe { std::mem::zeroed() });
            header.os = if cfg!(target_os = "macos") {
                19
            } else if cfg!(target_os = "windows") {
                10
            } else {
                3
            };
            unsafe {
                libz_sys::deflateSetHeader(&mut *result.stream, &mut *header);
            }
            result.header = Some(header);
        }
        Ok(result)
    }
    pub(super) fn native_bytes(&self) -> usize {
        self.allocated.get()
            + std::mem::size_of::<z_stream>()
            + std::mem::size_of::<Cell<usize>>()
            + self
                .header
                .as_ref()
                .map_or(0, |h| std::mem::size_of_val(&**h))
    }
    pub(super) fn step(
        &mut self,
        op: &ns::StepIn,
        input: &[u8],
        output: &mut [u8],
    ) -> std::io::Result<(usize, usize, ns::StepStatus)> {
        let flush = if op.op == ns::StreamOp::FINAL {
            libz_sys::Z_FINISH
        } else if op.op == ns::StreamOp::FLUSH {
            match op.flush_kind {
                0 => libz_sys::Z_NO_FLUSH,
                1 => libz_sys::Z_PARTIAL_FLUSH,
                2 => libz_sys::Z_SYNC_FLUSH,
                3 => libz_sys::Z_FULL_FLUSH,
                4 => libz_sys::Z_FINISH,
                _ => libz_sys::Z_BLOCK,
            }
        } else {
            libz_sys::Z_NO_FLUSH
        };
        let length = input.len().min(u32::MAX as usize);
        self.stream.next_in = input.as_ptr().cast_mut();
        self.stream.avail_in = length as u32;
        self.stream.next_out = output.as_mut_ptr();
        self.stream.avail_out = output.len() as u32;
        let result = unsafe { libz_sys::deflate(&mut *self.stream, flush) };
        let consumed = length - self.stream.avail_in as usize;
        let written = output.len() - self.stream.avail_out as usize;
        self.stream.next_in = std::ptr::null_mut();
        self.stream.avail_in = 0;
        self.stream.next_out = std::ptr::null_mut();
        self.stream.avail_out = 0;
        let status = match result {
            libz_sys::Z_STREAM_END if op.op == ns::StreamOp::FINAL => ns::StepStatus::ENDED,
            libz_sys::Z_STREAM_END => ns::StepStatus::NEED_INPUT,
            libz_sys::Z_OK | libz_sys::Z_BUF_ERROR => {
                if consumed < input.len() || written == output.len() || op.op == ns::StreamOp::FINAL
                {
                    ns::StepStatus::MORE
                } else {
                    ns::StepStatus::NEED_INPUT
                }
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("deflate: {result:?}"),
                ))
            }
        };
        Ok((consumed, written, status))
    }
    pub(super) fn params(&mut self, level: Compression, strategy: i32) {
        // SyncFlush ran before this callback, leaving no pending output. The
        // C API still requires nonempty output space for a parameter change.
        let mut scratch = [0; 128];
        self.stream.next_out = scratch.as_mut_ptr();
        self.stream.avail_out = scratch.len() as u32;
        let status =
            unsafe { libz_sys::deflateParams(&mut *self.stream, level.level() as i32, strategy) };
        debug_assert!(status == libz_sys::Z_OK || status == libz_sys::Z_BUF_ERROR);
        debug_assert_eq!(self.stream.avail_out, scratch.len() as u32);
        self.stream.next_out = std::ptr::null_mut();
        self.stream.avail_out = 0;
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = libz_sys::deflateEnd(&mut *self.stream);
        }
        debug_assert_eq!(self.allocated.get(), 0);
    }
}
