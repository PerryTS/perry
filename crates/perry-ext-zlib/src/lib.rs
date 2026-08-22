//! Native bindings for Node's `zlib` module — gzip, gunzip,
//! deflate, inflate. Sync + async variants.
//!
//! First binary-bytes wrapper port under #466 Phase 5 — reads input bytes from
//! strings/Buffers and returns registered runtime Buffers. Compressed payloads
//! aren't valid UTF-8, so the wrapper can't go through the standard
//! `read_string` / `alloc_string` path.

use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzEncoder, MultiGzDecoder, ZlibDecoder, ZlibEncoder,
};
use flate2::Compression;
use perry_ffi::{alloc_buffer, BufferHeader, ErrorKind};
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Read};

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
    let mut decoder = MultiGzDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn throw_deflate_decode_error(err: IoError) -> ! {
    if err.kind() == IoErrorKind::UnexpectedEof {
        perry_ffi::throw_with_code("unexpected end of file", "Z_BUF_ERROR", ErrorKind::Error);
    }
    perry_ffi::throw_with_code("incorrect header check", "Z_DATA_ERROR", ErrorKind::Error)
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

// Raw deflate (RFC 1951 — no zlib header, no adler32 trailer), which is what
// `deflateRawSync`/`inflateRawSync` speak. Distinct from the zlib-format pair
// above; the comment there already drew the line, but the entry points were
// never added on this side (#8005).
fn deflate_raw_bytes_with(data: &[u8], level: Compression) -> std::io::Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(data, level);
    let mut compressed = Vec::new();
    encoder.read_to_end(&mut compressed)?;
    Ok(compressed)
}

fn inflate_raw_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(data);
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

fn unzip_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    if data.starts_with(&[0x1f, 0x8b]) {
        gunzip_bytes(data)
    } else {
        inflate_bytes(data)
    }
}

fn crc32_bytes_with_seed(data: &[u8], seed: u32) -> u32 {
    let mut crc = seed ^ 0xffff_ffff;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 0 {
                crc >> 1
            } else {
                0xedb8_8320 ^ (crc >> 1)
            };
        }
    }
    crc ^ 0xffff_ffff
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
        Some(Err(err)) => throw_deflate_decode_error(err),
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
        Some(Err(err)) => throw_deflate_decode_error(err),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.deflateRawSync(data, opts)` — raw deflate, no zlib wrapper.
///
/// # Safety
///
/// NOTE THE ABI: codegen declares this pair as `(DOUBLE, DOUBLE)` and
/// `(DOUBLE)` (`runtime_decls/stdlib_ffi/third_party.rs`), unlike the
/// zlib-format one-shots beside it which take the data as `I64`. The parameter
/// types must match the DECLARATION, not this crate's local convention — the
/// bits are the same NaN-boxed value either way, so a mismatch would link
/// cleanly and misread the argument.
///
/// #4917: honor `options.level`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_raw_sync(data_value: f64, opts: f64) -> *mut BufferHeader {
    stream::js_zlib_validate_options(opts, 8);
    let data_bits = data_value.to_bits() as i64;
    stream::js_zlib_validate_buffer_arg(data_bits);
    let level = stream::compression_from_opts(opts);
    match stream::read_input_from_bits(data_bits).map(|d| deflate_raw_bytes_with(&d, level)) {
        Some(Ok(out)) => alloc_buffer(&out),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.inflateRawSync(data)` — raw inflate, no zlib wrapper.
///
/// # Safety
///
/// See `js_zlib_deflate_raw_sync` on the `DOUBLE` ABI.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_raw_sync(data_value: f64) -> *mut BufferHeader {
    let data_bits = data_value.to_bits() as i64;
    stream::js_zlib_validate_buffer_arg(data_bits);
    match stream::read_input_from_bits(data_bits).map(|d| inflate_raw_bytes(&d)) {
        Some(Ok(out)) => alloc_buffer(&out),
        Some(Err(err)) => throw_deflate_decode_error(err),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.unzipSync(data)` — auto-detect gzip or zlib-wrapped deflate input.
///
/// # Safety
///
/// `data_value` must be a valid NaN-boxed string, Buffer, or TypedArray value.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_unzip_sync(data_value: f64) -> *mut BufferHeader {
    let data_bits = data_value.to_bits() as i64;
    stream::js_zlib_validate_buffer_arg(data_bits);
    match stream::read_input_from_bits(data_bits).map(|data| unzip_bytes(&data)) {
        Some(Ok(out)) => alloc_buffer(&out),
        Some(Err(err)) => throw_deflate_decode_error(err),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.crc32(data, seed?)` using the reflected IEEE polynomial.
///
/// # Safety
///
/// `data_value` must be a valid NaN-boxed string, Buffer, or TypedArray value.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_crc32(data_value: f64, seed: f64) -> f64 {
    let data_bits = data_value.to_bits() as i64;
    stream::js_zlib_validate_buffer_arg(data_bits);
    match stream::read_input_from_bits(data_bits) {
        Some(data) => f64::from(crc32_bytes_with_seed(&data, seed as u32)),
        None => 0.0,
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
    stream::queue_one_shot_callback(data_value, callback_value, "Gzip", gzip_bytes);
}

/// `zlib.gunzip(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Gunzip", gunzip_bytes);
}

/// `zlib.deflate(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Deflate", deflate_bytes);
}

/// `zlib.inflate(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Inflate", inflate_bytes);
}

/// `zlib.deflateRaw(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_raw(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "DeflateRaw", |data| {
        deflate_raw_bytes_with(data, Compression::default())
    });
}

/// `zlib.inflateRaw(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_raw(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "InflateRaw", inflate_raw_bytes);
}

/// `zlib.unzip(data, callback) -> undefined`.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_unzip(data_value: f64, callback_value: f64) {
    stream::queue_one_shot_callback(data_value, callback_value, "Unzip", unzip_bytes);
}

/// Dispatch a captured `node:zlib` export through the external zlib archive.
///
/// Optimized builds strip the bundled codecs from `perry-stdlib` and link this
/// crate instead. Direct calls still target the exported `js_zlib_*` symbols,
/// while value calls such as `util.promisify(zlib.gzip)` enter the runtime's
/// native-module dispatcher. This is the external counterpart of
/// `perry_stdlib::zlib::js_zlib_native_dispatch`, keeping both call paths on
/// the same implementation.
///
/// # Safety
/// `method` and `args` must be valid for their corresponding lengths. Every
/// argument is a raw NaN-boxed JS value.
#[no_mangle]
pub unsafe extern "C" fn js_ext_zlib_native_dispatch(
    method: *const u8,
    method_len: usize,
    args: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(perry_ffi::JsValue::UNDEFINED.bits());
    if method.is_null() || method_len == 0 {
        return undefined;
    }
    let name = std::str::from_utf8(std::slice::from_raw_parts(method, method_len)).unwrap_or("");
    let arg = |index: usize| -> f64 {
        if index < args_len && !args.is_null() {
            *args.add(index)
        } else {
            undefined
        }
    };
    let pointer_value = |ptr: *mut BufferHeader| -> f64 {
        if ptr.is_null() {
            undefined
        } else {
            f64::from_bits(perry_ffi::JsValue::from_object_ptr(ptr).bits())
        }
    };
    let handle_value = |handle: i64| -> f64 {
        f64::from_bits(perry_ffi::JsValue::from_object_ptr(handle as usize as *mut u8).bits())
    };
    let callback = || arg(args_len.saturating_sub(1));

    match name {
        "gzipSync" => pointer_value(js_zlib_gzip_sync(arg(0).to_bits() as i64, arg(1))),
        "gunzipSync" => pointer_value(js_zlib_gunzip_sync(arg(0).to_bits() as i64)),
        "deflateSync" => pointer_value(js_zlib_deflate_sync(arg(0).to_bits() as i64, arg(1))),
        "inflateSync" => pointer_value(js_zlib_inflate_sync(arg(0).to_bits() as i64)),
        "deflateRawSync" => pointer_value(js_zlib_deflate_raw_sync(arg(0), arg(1))),
        "inflateRawSync" => pointer_value(js_zlib_inflate_raw_sync(arg(0))),
        "unzipSync" => pointer_value(js_zlib_unzip_sync(arg(0))),
        "brotliCompressSync" => {
            pointer_value(js_zlib_brotli_compress_sync(arg(0).to_bits() as i64))
        }
        "brotliDecompressSync" => {
            pointer_value(js_zlib_brotli_decompress_sync(arg(0).to_bits() as i64))
        }
        "zstdCompressSync" => pointer_value(js_zlib_zstd_compress_sync(arg(0), arg(1))),
        "zstdDecompressSync" => pointer_value(js_zlib_zstd_decompress_sync(arg(0), arg(1))),
        "crc32" => js_zlib_crc32(arg(0), if args_len >= 2 { arg(1) } else { 0.0 }),
        "gzip" => {
            js_zlib_gzip(arg(0), callback());
            undefined
        }
        "gunzip" => {
            js_zlib_gunzip(arg(0), callback());
            undefined
        }
        "deflate" => {
            js_zlib_deflate(arg(0), callback());
            undefined
        }
        "inflate" => {
            js_zlib_inflate(arg(0), callback());
            undefined
        }
        "deflateRaw" => {
            js_zlib_deflate_raw(arg(0), callback());
            undefined
        }
        "inflateRaw" => {
            js_zlib_inflate_raw(arg(0), callback());
            undefined
        }
        "unzip" => {
            js_zlib_unzip(arg(0), callback());
            undefined
        }
        "brotliCompress" => {
            js_zlib_brotli_compress(arg(0), callback());
            undefined
        }
        "brotliDecompress" => {
            js_zlib_brotli_decompress(arg(0), callback());
            undefined
        }
        "zstdCompress" => {
            js_zlib_zstd_compress(arg(0), callback());
            undefined
        }
        "zstdDecompress" => {
            js_zlib_zstd_decompress(arg(0), callback());
            undefined
        }
        "createGzip" => handle_value(js_zlib_create_gzip(arg(0))),
        "createGunzip" => handle_value(js_zlib_create_gunzip(arg(0))),
        "createDeflate" => handle_value(js_zlib_create_deflate(arg(0))),
        "createInflate" => handle_value(js_zlib_create_inflate(arg(0))),
        "createDeflateRaw" => handle_value(js_zlib_create_deflate_raw(arg(0))),
        "createInflateRaw" => handle_value(js_zlib_create_inflate_raw(arg(0))),
        "createUnzip" => handle_value(js_zlib_create_unzip(arg(0))),
        "createBrotliCompress" => handle_value(js_zlib_create_brotli_compress(arg(0))),
        "createBrotliDecompress" => handle_value(js_zlib_create_brotli_decompress(arg(0))),
        "createZstdCompress" => handle_value(js_zlib_create_zstd_compress(arg(0))),
        "createZstdDecompress" => handle_value(js_zlib_create_zstd_decompress(arg(0))),
        _ => undefined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{
        alloc_closure, alloc_string, read_buffer_bytes, register_closure_arity, JsString, JsValue,
        RawClosureHeader,
    };
    use std::cell::Cell;

    thread_local! {
        static DISPATCH_CALLBACK_FIRED: Cell<bool> = const { Cell::new(false) };
        static DISPATCH_CALLBACK_OK: Cell<bool> = const { Cell::new(false) };
    }

    extern "C" fn record_dispatch_callback(
        _closure: *const RawClosureHeader,
        err: f64,
        value: f64,
    ) -> f64 {
        DISPATCH_CALLBACK_FIRED.with(|fired| fired.set(true));
        let err_is_null = err.to_bits() == JsValue::NULL.bits();
        let output = JsValue::from_bits(value.to_bits()).as_pointer::<BufferHeader>();
        let output_is_gzip =
            read_buffer_bytes(output).is_some_and(|bytes| bytes.starts_with(&[0x1f, 0x8b]));
        DISPATCH_CALLBACK_OK.with(|ok| ok.set(err_is_null && output_is_gzip));
        f64::from_bits(JsValue::UNDEFINED.bits())
    }

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
    fn gunzip_reads_all_gzip_members() {
        let a = gzip_bytes(b"first ").unwrap();
        let b = gzip_bytes(b"second ").unwrap();
        let c = gzip_bytes(b"third").unwrap();
        let mut concatenated = Vec::new();
        concatenated.extend_from_slice(&a);
        concatenated.extend_from_slice(&b);
        concatenated.extend_from_slice(&c);
        assert_eq!(gunzip_bytes(&concatenated).unwrap(), b"first second third");
    }

    #[test]
    fn gunzip_rejects_invalid_data() {
        assert!(gunzip_bytes(b"not a gzip stream").is_err());
    }

    #[test]
    fn deflate_then_inflate_round_trips() {
        let input = b"deflate test deflate test deflate test deflate test";
        let compressed = deflate_bytes(input).unwrap();
        let decompressed = inflate_bytes(&compressed).unwrap();
        assert_eq!(decompressed, input);
    }

    #[test]
    fn inflate_rejects_raw_deflate_payload() {
        use flate2::read::DeflateEncoder;

        let mut raw = Vec::new();
        DeflateEncoder::new(&b"hello"[..], Compression::default())
            .read_to_end(&mut raw)
            .unwrap();
        assert!(inflate_bytes(&raw).is_err());
    }

    #[test]
    fn unzip_auto_detects_gzip_and_zlib_streams() {
        let input = b"hello from unzip";
        assert_eq!(unzip_bytes(&gzip_bytes(input).unwrap()).unwrap(), input);
        assert_eq!(unzip_bytes(&deflate_bytes(input).unwrap()).unwrap(), input);
    }

    #[test]
    fn crc32_matches_node_with_optional_seed() {
        assert_eq!(crc32_bytes_with_seed(b"hello", 0), 907_060_870);
        assert_eq!(crc32_bytes_with_seed(b"hello", 123), 3_088_217_944);
        assert_eq!(crc32_bytes_with_seed(b"", 0), 0);
    }

    #[test]
    fn external_native_dispatch_routes_async_gzip_callback() {
        DISPATCH_CALLBACK_FIRED.with(|fired| fired.set(false));
        DISPATCH_CALLBACK_OK.with(|ok| ok.set(false));

        register_closure_arity(record_dispatch_callback as *const u8, 2);
        let callback = alloc_closure(record_dispatch_callback as *const u8, 0);
        assert!(!callback.is_null());

        let input = alloc_buffer(b"captured zlib export");
        assert!(!input.is_null());
        let args = [
            f64::from_bits(JsValue::from_object_ptr(input).bits()),
            f64::from_bits(JsValue::from_object_ptr(callback).bits()),
        ];
        let method = b"gzip";
        let result = unsafe {
            js_ext_zlib_native_dispatch(method.as_ptr(), method.len(), args.as_ptr(), args.len())
        };

        assert_eq!(result.to_bits(), JsValue::UNDEFINED.bits());
        assert_eq!(js_ext_zlib_has_active_handles(), 1);
        assert_eq!(unsafe { js_ext_zlib_process_pending() }, 1);
        assert!(DISPATCH_CALLBACK_FIRED.with(Cell::get));
        assert!(DISPATCH_CALLBACK_OK.with(Cell::get));
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
