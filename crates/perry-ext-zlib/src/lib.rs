//! Native bindings for Node's `zlib` module — gzip, gunzip,
//! deflate, inflate. Sync + async variants.
//!
//! First binary-bytes wrapper port under #466 Phase 5 — reads input bytes from
//! strings/Buffers and returns registered runtime Buffers. Compressed payloads
//! aren't valid UTF-8, so the wrapper can't go through the standard
//! `read_string` / `alloc_string` path.

use flate2::read::{GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder};
use flate2::Compression;
use perry_ffi::{alloc_buffer, BufferHeader};
use std::io::Read;

// #1843 — Transform-stream objects (`createGzip`/`createDeflate`/… with
// `.write`/`.end`/`.on`/`.pipe`) and Brotli one-shots. Split into its own
// module to keep this file under the 2000-line size gate.
mod stream;
pub use stream::*;

fn gzip_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    gzip_bytes_with(data, Compression::default())
}

// #2935: honor the `{ level }` option. `level` selects the zlib compression
// level (0 = none .. 9 = best), which changes the compressed output size.
fn gzip_bytes_with(data: &[u8], level: Compression) -> std::io::Result<Vec<u8>> {
    let mut encoder = GzEncoder::new(data, level);
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

fn gunzip_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = GzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

// Node's `zlib.deflateSync`/`inflateSync` use the zlib format (RFC 1950 —
// 0x78 header + adler32), NOT raw deflate. Raw deflate is `deflateRawSync`/
// `inflateRawSync`. Using ZlibEncoder/ZlibDecoder here makes the one-shots
// Node-byte-compatible and consistent with `createDeflate`/`createInflate`
// (which also use the zlib format), so a stream's output round-trips through
// `inflateSync` (#1843).
fn deflate_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    deflate_bytes_with(data, Compression::default())
}

// #2935: honor the `{ level }` option (see `gzip_bytes_with`).
fn deflate_bytes_with(data: &[u8], level: Compression) -> std::io::Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(data, level);
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

fn inflate_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

// ── sync variants ─────────────────────────────────────────────

/// `zlib.gzipSync(data, options?)`.
///
/// # Safety
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument (a string or
/// Buffer/TypedArray); the pointer is recovered via `js_get_string_pointer_unified`.
/// `opts` is the raw NaN-boxed options value (or `undefined`); an out-of-range
/// `{ level }` throws `RangeError` before any compression runs (#2935).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip_sync(data_bits: i64, opts: f64) -> *mut BufferHeader {
    stream::js_zlib_validate_options(opts, 9); // gzip needs windowBits >= 9 (#3662)
    stream::js_zlib_validate_buffer_arg(data_bits); // options validate before the buffer
    let level = stream::compression_from_opts(opts);
    match stream::read_input_from_bits(data_bits).map(|d| gzip_bytes_with(&d, level)) {
        Some(Ok(out)) => alloc_buffer(&out),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.gunzipSync(data)`.
///
/// # Safety
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument (#2935).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip_sync(data_bits: i64) -> *mut BufferHeader {
    stream::js_zlib_validate_buffer_arg(data_bits); // #3662
    match stream::read_input_from_bits(data_bits).map(|d| gunzip_bytes(&d)) {
        Some(Ok(out)) => alloc_buffer(&out),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.deflateSync(data, options?)`.
///
/// # Safety
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument; `opts` is
/// the raw NaN-boxed options value. An out-of-range `{ level }` throws
/// `RangeError` before any compression runs (#2935).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_sync(data_bits: i64, opts: f64) -> *mut BufferHeader {
    stream::js_zlib_validate_options(opts, 8); // deflate accepts windowBits >= 8 (#3662)
    stream::js_zlib_validate_buffer_arg(data_bits);
    let level = stream::compression_from_opts(opts);
    match stream::read_input_from_bits(data_bits).map(|d| deflate_bytes_with(&d, level)) {
        Some(Ok(out)) => alloc_buffer(&out),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.inflateSync(data)`.
///
/// # Safety
///
/// `data_bits` is the raw NaN-box bit pattern of the data argument (#2935).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_sync(data_bits: i64) -> *mut BufferHeader {
    stream::js_zlib_validate_buffer_arg(data_bits); // #3662
    match stream::read_input_from_bits(data_bits).map(|d| inflate_bytes(&d)) {
        Some(Ok(out)) => alloc_buffer(&out),
        _ => std::ptr::null_mut(),
    }
}

// `zlib.createBrotliDecompress` and the other `create*` Transform-stream
// factories now live in `stream.rs` (returning real stream handles).

// ── callback variants ─────────────────────────────────────────

/// `zlib.gzip(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Gzip", |b| gzip_bytes(b));
}

/// `zlib.gunzip(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Gunzip", |b| gunzip_bytes(b));
}

/// `zlib.deflate(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Deflate", |b| deflate_bytes(b));
}

/// `zlib.inflate(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Inflate", |b| inflate_bytes(b));
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{alloc_string, JsString};

    #[test]
    fn gzip_then_gunzip_round_trips_text() {
        let input = b"hello, world! hello, world! hello, world!";
        let compressed = gzip_bytes(input).unwrap();
        // Compression should actually compress repeating text.
        assert!(compressed.len() < input.len());
        let decompressed = gunzip_bytes(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn deflate_then_inflate_round_trips() {
        let input = b"deflate test deflate test deflate test deflate test";
        let compressed = deflate_bytes(input).unwrap();
        let decompressed = inflate_bytes(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    // End-to-end TS smoke tests cover the FFI Buffer allocation path.
    // Unit tests stay scoped to pure-Rust gzip / gunzip correctness above.
    #[test]
    #[allow(dead_code)]
    fn _placeholder() {
        // Compile-test only — the imports above stay live so a
        // future contributor sees the full FFI surface even when
        // the exec-end coverage moves into integration tests.
        let _ = (alloc_string("x"), JsString::from_raw as *const ());
    }
}
