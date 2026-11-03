//! The zlib C ABI the dictionary-bearing contexts drive.
//!
//! Native builds use zlib-ng (`libz-ng-sys`, a C library). wasm32 has no C
//! toolchain contract for it, so it uses `libz-rs-sys`, a Rust implementation
//! of the same ABI, behind the same names: `inflate_context.rs` and
//! `deflate_context.rs` import this module as `z` and run unchanged, including
//! their custom `zalloc`/`zfree` that charge the payload ledger. Exactly one
//! zlib is linked per target.

#[cfg(not(target_family = "wasm"))]
pub use libz_ng_sys::*;

#[cfg(target_family = "wasm")]
pub use wasm::*;

#[cfg(target_family = "wasm")]
#[allow(non_snake_case)]
mod wasm {
    use libz_rs_sys as rs;
    use std::ffi::{c_char, c_int, c_ulong, c_void};

    pub use rs::{
        alloc_func, free_func, Z_BUF_ERROR, Z_DATA_ERROR, Z_FINISH, Z_MEM_ERROR, Z_NEED_DICT,
        Z_NO_FLUSH, Z_OK, Z_STREAM_END,
    };

    /// `libz_ng_sys::z_stream`'s shape (non-null allocator pointers, mutable
    /// `next_in`), which the contexts build field by field. It is layout
    /// identical to `libz_rs_sys::z_stream`, whose allocator pointers are
    /// `Option`s (null-pointer niche) and whose `next_in` is `*const`.
    #[repr(C)]
    pub struct z_stream {
        pub next_in: *mut u8,
        pub avail_in: u32,
        pub total_in: c_ulong,
        pub next_out: *mut u8,
        pub avail_out: u32,
        pub total_out: c_ulong,
        pub msg: *mut c_char,
        pub state: *mut c_void,
        pub zalloc: alloc_func,
        pub zfree: free_func,
        pub opaque: *mut c_void,
        pub data_type: c_int,
        pub adler: c_ulong,
        pub reserved: c_ulong,
    }
    const _: () = {
        assert!(size_of::<z_stream>() == size_of::<rs::z_stream>());
        assert!(align_of::<z_stream>() == align_of::<rs::z_stream>());
    };

    fn raw(stream: *mut z_stream) -> *mut rs::z_stream {
        stream.cast()
    }

    pub unsafe fn zng_deflateInit(stream: *mut z_stream, level: c_int) -> c_int {
        rs::deflateInit_(
            raw(stream),
            level,
            rs::zlibVersion(),
            size_of::<rs::z_stream>() as c_int,
        )
    }
    pub unsafe fn zng_inflateInit2(stream: *mut z_stream, window_bits: c_int) -> c_int {
        rs::inflateInit2_(
            raw(stream),
            window_bits,
            rs::zlibVersion(),
            size_of::<rs::z_stream>() as c_int,
        )
    }
    pub unsafe fn deflate(stream: *mut z_stream, flush: c_int) -> c_int {
        rs::deflate(raw(stream), flush)
    }
    pub unsafe fn deflateParams(stream: *mut z_stream, level: c_int, strategy: c_int) -> c_int {
        rs::deflateParams(raw(stream), level, strategy)
    }
    pub unsafe fn deflateSetDictionary(stream: *mut z_stream, dict: *const u8, len: u32) -> c_int {
        rs::deflateSetDictionary(raw(stream), dict, len)
    }
    pub unsafe fn deflateEnd(stream: *mut z_stream) -> c_int {
        rs::deflateEnd(raw(stream))
    }
    pub unsafe fn inflate(stream: *mut z_stream, flush: c_int) -> c_int {
        rs::inflate(raw(stream), flush)
    }
    pub unsafe fn inflateSetDictionary(stream: *mut z_stream, dict: *const u8, len: u32) -> c_int {
        rs::inflateSetDictionary(raw(stream), dict, len)
    }
    pub unsafe fn inflateEnd(stream: *mut z_stream) -> c_int {
        rs::inflateEnd(raw(stream))
    }
}
