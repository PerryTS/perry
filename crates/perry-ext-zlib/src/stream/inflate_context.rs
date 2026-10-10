//! CPU-dispatched inflate state in the payload's allocation ledger.
//!
//! The C stream and its allocator ledger stay at stable addresses. zlib-ng
//! owns its aligned window/tables through the existing BufferOwner hook;
//! moving this Rust handle never moves the C stream or retains input bytes.

use super::allocation::{out_of_memory, Placed};
use libz_ng_sys as z;
use perry_ffi::native_payload::buffer::BufferOwner;
use std::ffi::{c_void, CStr};
use std::io::{self, ErrorKind};

extern "C" {
    // BufferOwner::new verifies this existing payload-buffer ABI. Its
    // cookie is the ledger held alive by Placed's BufferOwner clone.
    fn js_perry_payload_buffer_hook_alloc(opaque: *mut c_void, size: usize) -> *mut c_void;
    fn js_perry_payload_buffer_hook_free(opaque: *mut c_void, pointer: *mut c_void);
}

pub(super) struct Context(Placed<z::z_stream>);

pub(super) struct Progress {
    pub consumed: usize,
    pub written: usize,
    pub ended: bool,
}

unsafe extern "C" fn allocate(opaque: *mut c_void, items: u32, size: u32) -> *mut c_void {
    let Some(bytes) = (items as usize).checked_mul(size as usize) else {
        return std::ptr::null_mut();
    };
    js_perry_payload_buffer_hook_alloc(opaque, bytes)
}

unsafe extern "C" fn release(opaque: *mut c_void, pointer: *mut c_void) {
    js_perry_payload_buffer_hook_free(opaque, pointer);
}

impl Context {
    pub(super) fn new(owner: &BufferOwner, zlib_header: bool) -> io::Result<Self> {
        Self::with_allocator(owner, allocate, zlib_header)
    }

    fn with_allocator(
        owner: &BufferOwner,
        alloc: z::alloc_func,
        zlib_header: bool,
    ) -> io::Result<Self> {
        // z_stream's function fields are non-null Rust function pointers:
        // construct them explicitly rather than zeroing the C structure.
        let state = z::z_stream {
            next_in: std::ptr::null_mut(),
            avail_in: 0,
            total_in: 0,
            next_out: std::ptr::null_mut(),
            avail_out: 0,
            total_out: 0,
            msg: std::ptr::null_mut(),
            state: std::ptr::null_mut(),
            zalloc: alloc,
            zfree: release,
            opaque: owner.hook().opaque,
            data_type: 0,
            adler: 0,
            reserved: 0,
        };
        let mut context = Self(Placed::try_new(owner, state).ok_or_else(out_of_memory)?);
        let status =
            unsafe { z::zng_inflateInit2(&mut *context.0, if zlib_header { 15 } else { -15 }) };
        if status != z::Z_OK {
            return Err(context.error(status));
        }
        Ok(context)
    }

    pub(super) fn step(&mut self, input: &[u8], output: &mut [u8]) -> io::Result<Progress> {
        let input_len = input.len().min(u32::MAX as usize) as u32;
        let output_len = output.len().min(u32::MAX as usize) as u32;
        let stream = &mut *self.0;
        stream.next_in = input.as_ptr().cast_mut();
        stream.avail_in = input_len;
        stream.next_out = output.as_mut_ptr();
        stream.avail_out = output_len;
        // The input/output borrows last through this call; allocator hooks
        // allocate native bytes, and no JS allocation or callback runs here.
        let status = unsafe { z::inflate(stream, z::Z_NO_FLUSH) };
        let consumed = (input_len - stream.avail_in) as usize;
        let written = (output_len - stream.avail_out) as usize;
        // Neither the context nor C stream retains a borrowed buffer pointer.
        stream.next_in = std::ptr::null_mut();
        stream.next_out = std::ptr::null_mut();
        stream.avail_in = 0;
        stream.avail_out = 0;
        match status {
            z::Z_OK | z::Z_STREAM_END | z::Z_BUF_ERROR => Ok(Progress {
                consumed,
                written,
                ended: status == z::Z_STREAM_END,
            }),
            _ => Err(self.error(status)),
        }
    }

    fn error(&self, status: i32) -> io::Error {
        if status == z::Z_MEM_ERROR {
            return out_of_memory();
        }
        let message = if self.0.msg.is_null() {
            "invalid compressed data".into()
        } else {
            // zlib-ng supplies a NUL-terminated static error message.
            unsafe { CStr::from_ptr(self.0.msg) }
                .to_string_lossy()
                .into_owned()
        };
        io::Error::new(ErrorKind::InvalidData, message)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // Runs before Placed releases the stream, hook and owner. A failed
        // init leaves a null state, which inflateEnd accepts as an error.
        unsafe { z::inflateEnd(&mut *self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_inflate_context_preserves_chunk_boundaries_and_releases_exact_allocations() {
        let data: Vec<_> = (0..65_539).map(|i| (i * 37 % 251) as u8).collect();
        for zlib_header in [false, true] {
            let input = if zlib_header {
                crate::deflate_bytes(&data).unwrap()
            } else {
                crate::deflate_raw_bytes_with(&data, flate2::Compression::default()).unwrap()
            };
            for input_chunk in [1, 7, 511, 4096] {
                for output_chunk in [1, 17, 4096] {
                    let owner = BufferOwner::new();
                    let mut contexts = Vec::new();
                    contexts.push(Context::new(&owner, zlib_header).unwrap());
                    let mut context = contexts.pop().unwrap();
                    assert!(owner.bytes() > std::mem::size_of::<z::z_stream>());
                    let mut at = 0;
                    let mut output = Vec::new();
                    loop {
                        let end = (at + input_chunk).min(input.len());
                        let mut scratch = vec![0xa5; output_chunk + 16];
                        let progress = context
                            .step(&input[at..end], &mut scratch[..output_chunk])
                            .unwrap();
                        assert!(scratch[output_chunk..].iter().all(|&byte| byte == 0xa5));
                        assert!(context.0.next_in.is_null() && context.0.next_out.is_null());
                        at += progress.consumed;
                        output.extend_from_slice(&scratch[..progress.written]);
                        if progress.ended {
                            break;
                        }
                        assert!(progress.consumed > 0 || progress.written > 0);
                    }
                    assert_eq!(at, input.len());
                    assert_eq!(output, data);
                    drop(context);
                    assert_eq!(owner.bytes(), 0);
                }
            }
        }
    }

    #[test]
    fn refused_inflate_allocation_releases_the_placed_stream() {
        unsafe extern "C" fn refuse(_: *mut c_void, _: u32, _: u32) -> *mut c_void {
            std::ptr::null_mut()
        }
        let owner = BufferOwner::new();
        let result = Context::with_allocator(&owner, refuse, true);
        assert!(matches!(result, Err(e) if e.kind() == ErrorKind::OutOfMemory));
        assert_eq!(owner.bytes(), 0);
    }
}
