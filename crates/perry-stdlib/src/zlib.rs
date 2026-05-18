//! Zlib compression module
//!
//! Native implementation of Node.js zlib module.
//! Provides gzip, gunzip, deflate, and inflate functions.

use flate2::read::{DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder};
use flate2::Compression;
use perry_runtime::{
    buffer::{buffer_alloc, mark_as_uint8array, BufferHeader},
    js_string_from_bytes, JSValue, StringHeader,
};
use std::io::Read;

use crate::common::async_bridge::{queue_promise_resolution, spawn};

/// Helper to extract bytes from StringHeader pointer
unsafe fn bytes_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(bytes.to_vec())
}

/// Gzip compress data synchronously
/// zlib.gzipSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip_sync(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut encoder = GzEncoder::new(&data[..], Compression::default());
    let mut compressed = Vec::new();

    match encoder.read_to_end(&mut compressed) {
        Ok(_) => js_string_from_bytes(compressed.as_ptr(), compressed.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gunzip decompress data synchronously
/// zlib.gunzipSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip_sync(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut decoder = GzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => js_string_from_bytes(decompressed.as_ptr(), decompressed.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Deflate compress data synchronously
/// zlib.deflateSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_sync(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut encoder = DeflateEncoder::new(&data[..], Compression::default());
    let mut compressed = Vec::new();

    match encoder.read_to_end(&mut compressed) {
        Ok(_) => js_string_from_bytes(compressed.as_ptr(), compressed.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Inflate decompress data synchronously
/// zlib.inflateSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_sync(data_ptr: *const StringHeader) -> *mut StringHeader {
    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => return std::ptr::null_mut(),
    };

    let mut decoder = DeflateDecoder::new(&data[..]);
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => js_string_from_bytes(decompressed.as_ptr(), decompressed.len() as u32),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Gzip compress data asynchronously
/// zlib.gzip(data) -> Promise<Buffer>
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip(
    data_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => {
            let err_msg = "Invalid input data";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let mut encoder = GzEncoder::new(&data[..], Compression::default());
            let mut compressed = Vec::new();
            encoder.read_to_end(&mut compressed).map(|_| compressed)
        })
        .await;

        match result {
            Ok(Ok(compressed)) => {
                let result_str = js_string_from_bytes(compressed.as_ptr(), compressed.len() as u32);
                let result_bits = JSValue::pointer(result_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Ok(Err(e)) => {
                let err_msg = format!("Gzip error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
            Err(e) => {
                let err_msg = format!("Task error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// `zlib.createBrotliDecompress(options?)` — minimal feature-check
/// shim.
///
/// Axios's compile-time module init calls
/// `typeof zlib.createBrotliDecompress === 'function'` and only
/// invokes this when a server actually replies with
/// `content-encoding: br`. To get axios past the gate we return a
/// registered Buffer-shaped handle (32 bytes, marked as Uint8Array
/// so `instanceof Uint8Array` is true). The real Brotli pipe
/// (`write` / `end` / `on('data')` / `on('end')`) is a TODO follow-
/// up — when a Brotli response actually comes back the stream
/// handlers will hit the unimplemented dispatch path and surface a
/// clear error to the caller rather than silently corrupting the
/// payload.
///
/// `_opts` is the (optional) BrotliDecompressOptions object; we
/// ignore it for the stub since none of the configurable knobs
/// affect feature-check semantics. Returns a `*mut BufferHeader`
/// which the calling site NaN-boxes with POINTER_TAG via
/// `nanbox_pointer_inline`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_brotli_decompress(_opts: f64) -> *mut BufferHeader {
    // 32 bytes is arbitrary — large enough to look like a real
    // stream handle, small enough to be cheap.
    let buf = buffer_alloc(32);
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    (*buf).length = 0;
    mark_as_uint8array(buf as usize);
    buf
}

/// Gunzip decompress data asynchronously
/// zlib.gunzip(data) -> Promise<Buffer>
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip(
    data_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let data = match bytes_from_header(data_ptr) {
        Some(d) => d,
        None => {
            let err_msg = "Invalid input data";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let mut decoder = GzDecoder::new(&data[..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map(|_| decompressed)
        })
        .await;

        match result {
            Ok(Ok(decompressed)) => {
                let result_str =
                    js_string_from_bytes(decompressed.as_ptr(), decompressed.len() as u32);
                let result_bits = JSValue::pointer(result_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Ok(Err(e)) => {
                let err_msg = format!("Gunzip error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
            Err(e) => {
                let err_msg = format!("Task error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}
