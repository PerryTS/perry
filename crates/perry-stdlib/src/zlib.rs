//! Zlib compression module
//!
//! Native implementation of Node.js zlib module.
//! Provides gzip, gunzip, deflate, and inflate functions.

use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, MultiGzDecoder, ZlibDecoder, ZlibEncoder,
};
use flate2::Compression;
use perry_runtime::{
    buffer::{
        buffer_alloc, buffer_data, buffer_data_mut, is_registered_buffer, js_buffer_alloc,
        js_buffer_is_buffer, mark_as_uint8array, BufferHeader,
    },
    closure::is_closure_ptr,
    js_closure_call0, js_closure_call1, js_closure_call2, js_get_string_pointer_unified,
    js_string_from_bytes, ClosureHeader, JSValue, StringHeader,
};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;

/// Throw a JS `Error` with the given message, longjmp'ing back to the
/// nearest enclosing `try`. Used by sync codec FFIs when flate2/brotli
/// reports invalid input so callers see a Node-shaped exception instead
/// of a sentinel null return.
fn throw_zlib_error(message: &str) -> ! {
    unsafe {
        let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let err = perry_runtime::error::js_error_new_with_message(msg);
        perry_runtime::exception::js_throw(perry_runtime::value::js_nanbox_pointer(err as i64))
    }
}

/// Allocate a Buffer (Uint8Array-marked) holding `data`. The codegen entry
/// for `*Sync` codecs declares `NR_PTR`, so the caller NaN-boxes this
/// pointer with `POINTER_TAG` — exactly what Perry expects for Buffer-typed
/// JS values. Critical for `compressed[i]` indexing and binary-safe round
/// trips (#1843 / node-suite zlib).
unsafe fn buffer_from_slice(data: &[u8]) -> *mut BufferHeader {
    let buf = buffer_alloc(data.len() as u32);
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    (*buf).length = data.len() as u32;
    if !data.is_empty() {
        std::ptr::copy_nonoverlapping(data.as_ptr(), buffer_data_mut(buf), data.len());
    }
    mark_as_uint8array(buf as usize);
    buf
}

fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let js_value = JSValue::from_bits(bits);
    if js_value.is_pointer() || js_value.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && bits >= 0x1000 && bits < 0x0001_0000_0000_0000 {
        bits as usize
    } else {
        0
    }
}

fn throw_invalid_data_arg(value: f64, arg_name: &str, accepted: &str) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type string or an instance of {}. Received {}",
        arg_name,
        accepted,
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn validate_callback_arg(value: f64) -> *const ClosureHeader {
    let bits = value.to_bits();
    if JSValue::from_bits(bits).is_pointer() {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if is_closure_ptr(ptr) {
            return ptr as *const ClosureHeader;
        }
    }
    let message = format!(
        "The \"callback\" argument must be of type function. Received {}",
        perry_runtime::fs::validate::describe_received(value)
    );
    perry_runtime::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

unsafe fn bytes_from_js_data(
    value: f64,
    allow_array_buffer: bool,
    arg_name: &str,
    accepted: &str,
) -> Vec<u8> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_any_string() {
        let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
        if ptr.is_null() {
            throw_invalid_data_arg(value, arg_name, accepted);
        }
        let len = (*ptr).byte_len as usize;
        let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        return std::slice::from_raw_parts(data_ptr, len).to_vec();
    }

    let raw = raw_addr_from_value(value);
    if raw >= 0x1000 {
        if perry_runtime::typedarray::lookup_typed_array_kind(raw).is_some() {
            let ta = raw as *const perry_runtime::typedarray::TypedArrayHeader;
            if let Some(bytes) = perry_runtime::typedarray::typed_array_bytes(ta) {
                return bytes.to_vec();
            }
        }
        if is_registered_buffer(raw) {
            if !allow_array_buffer
                && perry_runtime::buffer::is_any_array_buffer(raw)
                && !perry_runtime::buffer::is_data_view(raw)
            {
                throw_invalid_data_arg(value, arg_name, accepted);
            }
            let buf = raw as *const BufferHeader;
            let len = (*buf).length as usize;
            let data_ptr = buffer_data(buf);
            return std::slice::from_raw_parts(data_ptr, len).to_vec();
        }
    }

    throw_invalid_data_arg(value, arg_name, accepted);
}

unsafe fn codec_bytes(value: f64) -> Vec<u8> {
    bytes_from_js_data(
        value,
        true,
        "buffer",
        "Buffer, TypedArray, DataView, or ArrayBuffer",
    )
}

unsafe fn crc32_bytes(value: f64) -> Vec<u8> {
    bytes_from_js_data(value, false, "data", "Buffer, TypedArray, or DataView")
}

/// Gzip compress data synchronously
/// zlib.gzipSync(data, options?) -> Buffer
///
/// #2935: `options.level` selects the compression level (validated `-1..=9`;
/// out-of-range throws `RangeError [ERR_OUT_OF_RANGE]` before compression).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip_sync(data_bits: i64, opts: f64) -> *mut BufferHeader {
    // Gzip compression requires `windowBits >= 9`; option validation runs
    // before the buffer is touched so a bad option reports Node's error first.
    perry_runtime::js_zlib_validate_options(opts, 9);
    let level = Compression::new(perry_runtime::js_zlib_resolve_level(opts) as u32);
    let data = codec_bytes(f64::from_bits(data_bits as u64));

    let mut encoder = GzEncoder::new(&data[..], level);
    let mut compressed = Vec::new();

    match encoder.read_to_end(&mut compressed) {
        Ok(_) => buffer_from_slice(&compressed),
        Err(e) => throw_zlib_error(&format!("zlib: {}", e)),
    }
}

/// Gunzip decompress data synchronously
/// zlib.gunzipSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip_sync(data_bits: i64) -> *mut BufferHeader {
    let data = codec_bytes(f64::from_bits(data_bits as u64));

    // `MultiGzDecoder` walks every concatenated gzip member, matching Node's
    // semantics where `gunzipSync(concat(gzip(a), gzip(b)))` returns `a + b`
    // (RFC 1952 §2.2). Plain `GzDecoder` only reads the first member.
    let mut decoder = MultiGzDecoder::new(&data[..]);
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => buffer_from_slice(&decompressed),
        Err(e) => throw_zlib_error(&format!("incorrect header check: {}", e)),
    }
}

/// Deflate compress data synchronously
/// zlib.deflateSync(data, options?) -> Buffer
///
/// #2935: honor `options.level` (see `js_zlib_gzip_sync`).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_sync(data_bits: i64, opts: f64) -> *mut BufferHeader {
    // Raw/zlib deflate accepts `windowBits >= 8` (only gzip needs 9).
    perry_runtime::js_zlib_validate_options(opts, 8);
    let level = Compression::new(perry_runtime::js_zlib_resolve_level(opts) as u32);
    let data = codec_bytes(f64::from_bits(data_bits as u64));

    // Node's `deflateSync` produces the zlib format (RFC 1950), not raw
    // deflate — `deflateRawSync` is the raw form. Use ZlibEncoder so the
    // output is Node-byte-compatible and round-trips through `inflateSync`
    // (and matches `createDeflate`) (#1843).
    let mut encoder = ZlibEncoder::new(&data[..], level);
    let mut compressed = Vec::new();

    match encoder.read_to_end(&mut compressed) {
        Ok(_) => buffer_from_slice(&compressed),
        Err(e) => throw_zlib_error(&format!("deflate: {}", e)),
    }
}

/// Inflate decompress data synchronously
/// zlib.inflateSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_sync(data_bits: i64) -> *mut BufferHeader {
    let data = codec_bytes(f64::from_bits(data_bits as u64));

    let mut decoder = ZlibDecoder::new(&data[..]);
    let mut decompressed = Vec::new();

    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => buffer_from_slice(&decompressed),
        Err(e) => throw_zlib_error(&format!("incorrect header check: {}", e)),
    }
}

/// Raw deflate compress synchronously (no zlib header, no adler32).
/// zlib.deflateRawSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_raw_sync(data_value: f64) -> *mut BufferHeader {
    let data = codec_bytes(data_value);
    let mut encoder = DeflateEncoder::new(&data[..], Compression::default());
    let mut compressed = Vec::new();
    match encoder.read_to_end(&mut compressed) {
        Ok(_) => buffer_from_slice(&compressed),
        Err(e) => throw_zlib_error(&format!("deflate raw: {}", e)),
    }
}

/// Raw deflate decompress synchronously.
/// zlib.inflateRawSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_raw_sync(data_value: f64) -> *mut BufferHeader {
    let data = codec_bytes(data_value);
    let mut decoder = DeflateDecoder::new(&data[..]);
    let mut decompressed = Vec::new();
    match decoder.read_to_end(&mut decompressed) {
        Ok(_) => buffer_from_slice(&decompressed),
        Err(e) => throw_zlib_error(&format!("inflate raw: {}", e)),
    }
}

/// Auto-detect gzip vs zlib by sniffing the first two bytes. gzip members
/// always start with 0x1f 0x8b; everything else is treated as zlib-format
/// deflate.
/// zlib.unzipSync(data) -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_unzip_sync(data_value: f64) -> *mut BufferHeader {
    let data = codec_bytes(data_value);
    let mut out = Vec::new();
    let ok = if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        // Multi-member gzip support per RFC 1952 §2.2.
        MultiGzDecoder::new(&data[..]).read_to_end(&mut out).is_ok()
    } else {
        ZlibDecoder::new(&data[..]).read_to_end(&mut out).is_ok()
    };
    if ok {
        buffer_from_slice(&out)
    } else {
        throw_zlib_error("incorrect header check");
    }
}

/// CRC32 (IEEE 802.3) with optional running seed for chunked input.
/// `seed = 0` (or absent) produces the canonical one-shot CRC32.
/// zlib.crc32(data, seed?) -> number
#[no_mangle]
pub unsafe extern "C" fn js_zlib_crc32(data_value: f64, seed: f64) -> f64 {
    let data = crc32_bytes(data_value);
    // Reflected IEEE polynomial 0xEDB88320 — same as zlib's. Built once on
    // first call. Sync::Once is enough here: every thread sees the populated
    // table after the first init.
    static mut TABLE: [u32; 256] = [0; 256];
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB88320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            TABLE[i as usize] = c;
        }
    });
    let mut c = (seed as u32) ^ 0xFFFF_FFFF;
    for &b in &data {
        let idx = ((c ^ b as u32) & 0xFF) as usize;
        c = TABLE[idx] ^ (c >> 8);
    }
    (c ^ 0xFFFF_FFFF) as f64
}

#[derive(Clone, Copy, PartialEq)]
enum Codec {
    Gzip,
    Gunzip,
    Deflate,
    Inflate,
    DeflateRaw,
    InflateRaw,
    Unzip,
    BrotliCompress,
    BrotliDecompress,
    ZstdCompress,
    ZstdDecompress,
}

const ZSTD_DEFAULT_LEVEL: i32 = 3;

fn run_one_shot_codec(codec: Codec, input: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match codec {
        Codec::Gzip => {
            GzEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::Gunzip => {
            MultiGzDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::Deflate => {
            ZlibEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::Inflate => {
            ZlibDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::DeflateRaw => {
            DeflateEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::InflateRaw => {
            DeflateDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::Unzip => {
            if input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
                MultiGzDecoder::new(input).read_to_end(&mut out)?;
            } else {
                ZlibDecoder::new(input).read_to_end(&mut out)?;
            }
        }
        Codec::BrotliCompress => {
            out = brotli_compress_bytes(input);
        }
        Codec::BrotliDecompress => {
            out = brotli_decompress_bytes(input)?;
        }
        Codec::ZstdCompress => {
            out = zstd_compress_bytes(input)?;
        }
        Codec::ZstdDecompress => {
            out = zstd_decompress_bytes(input)?;
        }
    }
    Ok(out)
}

unsafe fn queue_zlib_callback(codec: Codec, data_value: f64, callback_value: f64) {
    let callback = validate_callback_arg(callback_value) as i64;
    let data = codec_bytes(data_value);
    let result = run_one_shot_codec(codec, &data).map_err(|e| e.to_string());
    crate::common::async_bridge::ensure_pump_registered();
    ensure_zlib_gc_scanner();
    ZLIB_PENDING_EVENTS
        .lock()
        .unwrap()
        .push(ZlibEvent::OneShotCallback(callback, result));
    perry_runtime::event_pump::js_notify_main_thread();
}

/// `zlib.gzip(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::Gzip, data_value, callback_value);
}

/// `zlib.gunzip(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::Gunzip, data_value, callback_value);
}

/// `zlib.deflate(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::Deflate, data_value, callback_value);
}

/// `zlib.inflate(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::Inflate, data_value, callback_value);
}

/// `zlib.deflateRaw(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_raw(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::DeflateRaw, data_value, callback_value);
}

/// `zlib.inflateRaw(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_raw(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::InflateRaw, data_value, callback_value);
}

/// `zlib.unzip(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_unzip(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::Unzip, data_value, callback_value);
}

// ============================================================================
// Brotli one-shot functions (#1843 cluster 2)
//
// The `brotli` crate is already a `compression`-feature dep. Use its
// reader-based codecs for one-shot compress/decompress, mirroring the
// flate2 `*Sync`/async wrappers above. Quality 11 / window 22 are Node's
// defaults for `brotliCompressSync`.
// ============================================================================

fn brotli_compress_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut reader = brotli::CompressorReader::new(data, 4096, 11, 22);
    let _ = reader.read_to_end(&mut out);
    out
}

fn brotli_decompress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut reader = brotli::Decompressor::new(data, 4096);
    reader.read_to_end(&mut out)?;
    Ok(out)
}

/// `zlib.brotliCompressSync(data)` -> Buffer
///
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress_sync(data_bits: i64) -> *mut BufferHeader {
    let data = codec_bytes(f64::from_bits(data_bits as u64));
    let out = brotli_compress_bytes(&data);
    buffer_from_slice(&out)
}

/// `zlib.brotliDecompressSync(data)` -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_sync(data_bits: i64) -> *mut BufferHeader {
    let data = codec_bytes(f64::from_bits(data_bits as u64));
    match brotli_decompress_bytes(&data) {
        Ok(out) => buffer_from_slice(&out),
        Err(e) => throw_zlib_error(&format!("brotli: {}", e)),
    }
}

/// `zlib.brotliCompress(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::BrotliCompress, data_value, callback_value);
}

/// `zlib.brotliDecompress(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::BrotliDecompress, data_value, callback_value);
}

// ============================================================================
// Zstd one-shot functions (#2510)
// ============================================================================

fn zstd_compress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    zstd::stream::encode_all(data, ZSTD_DEFAULT_LEVEL)
}

fn zstd_decompress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    zstd::stream::decode_all(data)
}

/// `zlib.zstdCompressSync(data)` -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress_sync(
    data_value: f64,
    _opts: f64,
) -> *mut BufferHeader {
    let data = codec_bytes(data_value);
    match zstd_compress_bytes(&data) {
        Ok(out) => buffer_from_slice(&out),
        Err(e) => throw_zlib_error(&format!("zstd: {}", e)),
    }
}

/// `zlib.zstdDecompressSync(data)` -> Buffer
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress_sync(
    data_value: f64,
    _opts: f64,
) -> *mut BufferHeader {
    let data = codec_bytes(data_value);
    match zstd_decompress_bytes(&data) {
        Ok(out) => buffer_from_slice(&out),
        Err(e) => throw_zlib_error(&format!("zstd: {}", e)),
    }
}

/// `zlib.zstdCompress(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::ZstdCompress, data_value, callback_value);
}

/// `zlib.zstdDecompress(data, callback)` -> undefined
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress(data_value: f64, callback_value: f64) {
    queue_zlib_callback(Codec::ZstdDecompress, data_value, callback_value);
}

// ============================================================================
// zlib Transform-stream objects (#1843 cluster 1)
//
// `zlib.createGzip()` / `createGunzip()` / `createDeflate()` /
// `createInflate()` / `createDeflateRaw()` / `createInflateRaw()` /
// `createUnzip()` / `createBrotliCompress()` / `createBrotliDecompress()`
// return small-int handles (base 0xE0000, under the 0x100000 small-handle
// dispatch threshold) that the codegen NaN-boxes with POINTER_TAG. Subsequent
// `s.write()` / `s.end()` / `s.on()` / `s.pipe()` calls lose their static type
// and route through `js_native_call_method` → HANDLE_METHOD_DISPATCH →
// `dispatch_zlib_stream` (crates/perry-stdlib/src/common/dispatch.rs).
//
// This mirrors the net.Socket handle pattern (crates/perry-stdlib/src/net/
// mod.rs) but compression is synchronous, so there's no tokio task: input is
// buffered across `.write()` calls, the codec runs once on `.end()`, and the
// resulting 'data'/'end' events are merely *deferred* onto ZLIB_PENDING_EVENTS
// (drained by js_zlib_process_pending on the next loop tick) so that listeners
// registered after `.write()` still fire and `.pipe()` can forward chunks.
// ============================================================================

struct ZlibStreamState {
    codec: Codec,
    /// Streaming codec, fed incrementally by `.write()`. `None` for
    /// `createUnzip` (uses `input` + `run_codec` on `.end()`) or once finalized.
    codec_state: Option<CodecState>,
    /// Only used by `createUnzip` (buffer-until-end auto-detect).
    input: Vec<u8>,
    ended: bool,
    bytes_written: usize,
    pending_bytes_written: usize,
    /// Destinations registered via `.pipe(dest)` — stored as NaN-boxed bits;
    /// 'data'/'end' are forwarded to each via dynamic method dispatch.
    pipes: Vec<u64>,
}

enum ZlibEvent {
    Data(i64, Vec<u8>),
    End(i64),
    Error(i64, String),
    /// `.flush(cb)` completion callback — invoked after its flushed 'data'.
    Callback(i64),
    /// One-shot `zlib.gzip(data, cb)` style completion callback.
    OneShotCallback(i64, Result<Vec<u8>, String>),
}

lazy_static::lazy_static! {
    static ref ZLIB_STREAMS: Mutex<HashMap<i64, ZlibStreamState>> = Mutex::new(HashMap::new());
    static ref ZLIB_LISTENERS: Mutex<HashMap<i64, HashMap<String, Vec<i64>>>> =
        Mutex::new(HashMap::new());
    static ref ZLIB_PENDING_EVENTS: Mutex<Vec<ZlibEvent>> = Mutex::new(Vec::new());
    static ref NEXT_ZLIB_ID: Mutex<i64> = Mutex::new(ZLIB_STREAM_HANDLE_ID_START);
}

// Band boundaries owned by `perry_runtime::value::addr_class`.
const ZLIB_STREAM_HANDLE_ID_START: i64 =
    perry_runtime::value::addr_class::ZLIB_HANDLE_BAND_START as i64;
const ZLIB_STREAM_HANDLE_ID_END: i64 =
    perry_runtime::value::addr_class::ZLIB_HANDLE_BAND_END as i64;

static ZLIB_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the zlib-stream GC root scanner once. Listener closures
/// (`s.on('data', cb)`) are only referenced from `ZLIB_LISTENERS`; without
/// rooting them a GC between `.on()` and the deferred dispatch would free the
/// closure body (the same hazard net.Socket guards against — issue #35).
fn ensure_zlib_gc_scanner() {
    ZLIB_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named("stdlib:zlib", scan_zlib_roots);
    });
}

fn scan_zlib_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut listeners) = ZLIB_LISTENERS.lock() {
        for per_stream in listeners.values_mut() {
            for cb_vec in per_stream.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }
    // `.flush(cb)` and one-shot callbacks queued but not yet drained live only here.
    if let Ok(mut pending) = ZLIB_PENDING_EVENTS.lock() {
        for ev in pending.iter_mut() {
            match ev {
                ZlibEvent::Callback(cb) | ZlibEvent::OneShotCallback(cb, _) => {
                    visitor.visit_i64_slot(cb);
                }
                _ => {}
            }
        }
    }
}

fn next_zlib_id() -> i64 {
    let mut g = NEXT_ZLIB_ID.lock().unwrap();
    let id = *g;
    if id >= ZLIB_STREAM_HANDLE_ID_END {
        panic!("zlib stream handle id range exhausted");
    }
    *g += 1;
    id
}

fn create_zlib_stream(codec: Codec) -> i64 {
    ensure_zlib_gc_scanner();
    let id = next_zlib_id();
    ZLIB_STREAMS.lock().unwrap().insert(
        id,
        ZlibStreamState {
            codec,
            codec_state: make_codec_state(codec),
            input: Vec::new(),
            ended: false,
            bytes_written: 0,
            pending_bytes_written: 0,
            pipes: Vec::new(),
        },
    );
    id
}

/// True iff `handle` indexes a live zlib stream. Gates the dispatch arm in
/// `common::dispatch` so a handle-id collision with another subsystem's
/// registry can't misroute (handle id-spaces are not unified — see the long
/// comment in `js_handle_method_dispatch`).
pub fn is_zlib_stream_handle(handle: i64) -> bool {
    ZLIB_STREAMS.lock().unwrap().contains_key(&handle)
}

// ── factories ──────────────────────────────────────────────────────────────

/// # Safety
/// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_gzip(opts: f64) -> i64 {
    // Node validates the options object in the stream constructor, before any
    // data flows. Gzip compression needs `windowBits >= 9` (#3662).
    perry_runtime::js_zlib_validate_options(opts, 9);
    create_zlib_stream(Codec::Gzip)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_gunzip(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::Gunzip)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_deflate(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::Deflate)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_inflate(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::Inflate)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_deflate_raw(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::DeflateRaw)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_inflate_raw(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::InflateRaw)
}
/// # Safety
/// FFI entry; `opts` is the NaN-boxed options object (validated, then ignored).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_unzip(opts: f64) -> i64 {
    perry_runtime::js_zlib_validate_options(opts, 8);
    create_zlib_stream(Codec::Unzip)
}
/// # Safety
/// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_brotli_compress(_opts: f64) -> i64 {
    create_zlib_stream(Codec::BrotliCompress)
}
/// `zlib.createBrotliDecompress(options?)` — returns a real Transform-stream
/// handle. (Previously a feature-check Buffer stub; axios's
/// `typeof createBrotliDecompress === 'function'` gate still passes and a `br`
/// response now actually decodes.)
///
/// # Safety
/// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_brotli_decompress(_opts: f64) -> i64 {
    create_zlib_stream(Codec::BrotliDecompress)
}
/// # Safety
/// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_zstd_compress(_opts: f64) -> i64 {
    create_zlib_stream(Codec::ZstdCompress)
}
/// # Safety
/// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_zstd_decompress(_opts: f64) -> i64 {
    create_zlib_stream(Codec::ZstdDecompress)
}

// ── one-shot codec used by the streams on .end() ─────────────────────────────

fn run_codec(codec: Codec, input: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    match codec {
        Codec::Gzip => {
            GzEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::Gunzip => {
            GzDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::Deflate => {
            ZlibEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::Inflate => {
            ZlibDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::DeflateRaw => {
            DeflateEncoder::new(input, Compression::default()).read_to_end(&mut out)?;
        }
        Codec::InflateRaw => {
            DeflateDecoder::new(input).read_to_end(&mut out)?;
        }
        Codec::Unzip => {
            // Node's `createUnzip` auto-detects gzip vs zlib by header.
            if input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
                GzDecoder::new(input).read_to_end(&mut out)?;
            } else {
                ZlibDecoder::new(input).read_to_end(&mut out)?;
            }
        }
        Codec::BrotliCompress => {
            out = brotli_compress_bytes(input);
        }
        Codec::BrotliDecompress => {
            out = brotli_decompress_bytes(input)?;
        }
        Codec::ZstdCompress => {
            out = zstd_compress_bytes(input)?;
        }
        Codec::ZstdDecompress => {
            out = zstd_decompress_bytes(input)?;
        }
    }
    Ok(out)
}

// ── streaming codec state ────────────────────────────────────────────────────
//
// Stateful write-codec: fed incrementally by `.write()`, flushed by `.flush()`
// (Z_SYNC_FLUSH / BROTLI_OPERATION_FLUSH), finalized by `.end()`. `None` for
// `createUnzip` (gzip/zlib auto-detect stays buffer-until-end via `run_codec`).

enum CodecState {
    GzEnc(flate2::write::GzEncoder<Vec<u8>>),
    GzDec(flate2::write::GzDecoder<Vec<u8>>),
    ZlibEnc(flate2::write::ZlibEncoder<Vec<u8>>),
    ZlibDec(flate2::write::ZlibDecoder<Vec<u8>>),
    DeflateEnc(flate2::write::DeflateEncoder<Vec<u8>>),
    DeflateDec(flate2::write::DeflateDecoder<Vec<u8>>),
    BrotliEnc(brotli::CompressorWriter<Vec<u8>>),
    BrotliDec(brotli::DecompressorWriter<Vec<u8>>),
}

impl CodecState {
    fn write_chunk(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            CodecState::GzEnc(w) => w.write_all(data),
            CodecState::GzDec(w) => w.write_all(data),
            CodecState::ZlibEnc(w) => w.write_all(data),
            CodecState::ZlibDec(w) => w.write_all(data),
            CodecState::DeflateEnc(w) => w.write_all(data),
            CodecState::DeflateDec(w) => w.write_all(data),
            CodecState::BrotliEnc(w) => w.write_all(data),
            CodecState::BrotliDec(w) => w.write_all(data),
        }
    }

    fn flush_codec(&mut self) -> std::io::Result<()> {
        match self {
            CodecState::GzEnc(w) => w.flush(),
            CodecState::GzDec(w) => w.flush(),
            CodecState::ZlibEnc(w) => w.flush(),
            CodecState::ZlibDec(w) => w.flush(),
            CodecState::DeflateEnc(w) => w.flush(),
            CodecState::DeflateDec(w) => w.flush(),
            CodecState::BrotliEnc(w) => w.flush(),
            CodecState::BrotliDec(w) => w.flush(),
        }
    }

    fn drain(&mut self) -> Vec<u8> {
        match self {
            CodecState::GzEnc(w) => std::mem::take(w.get_mut()),
            CodecState::GzDec(w) => std::mem::take(w.get_mut()),
            CodecState::ZlibEnc(w) => std::mem::take(w.get_mut()),
            CodecState::ZlibDec(w) => std::mem::take(w.get_mut()),
            CodecState::DeflateEnc(w) => std::mem::take(w.get_mut()),
            CodecState::DeflateDec(w) => std::mem::take(w.get_mut()),
            CodecState::BrotliEnc(w) => std::mem::take(w.get_mut()),
            CodecState::BrotliDec(w) => std::mem::take(w.get_mut()),
        }
    }

    fn finish(self) -> std::io::Result<Vec<u8>> {
        match self {
            CodecState::GzEnc(w) => w.finish(),
            CodecState::GzDec(w) => w.finish(),
            CodecState::ZlibEnc(w) => w.finish(),
            CodecState::ZlibDec(w) => w.finish(),
            CodecState::DeflateEnc(w) => w.finish(),
            CodecState::DeflateDec(w) => w.finish(),
            CodecState::BrotliEnc(w) => Ok(w.into_inner()),
            CodecState::BrotliDec(w) => Ok(w.into_inner().unwrap_or_else(|v| v)),
        }
    }
}

fn make_codec_state(codec: Codec) -> Option<CodecState> {
    use flate2::write;
    Some(match codec {
        Codec::Gzip => CodecState::GzEnc(write::GzEncoder::new(Vec::new(), Compression::default())),
        Codec::Gunzip => CodecState::GzDec(write::GzDecoder::new(Vec::new())),
        Codec::Deflate => {
            CodecState::ZlibEnc(write::ZlibEncoder::new(Vec::new(), Compression::default()))
        }
        Codec::Inflate => CodecState::ZlibDec(write::ZlibDecoder::new(Vec::new())),
        Codec::DeflateRaw => CodecState::DeflateEnc(write::DeflateEncoder::new(
            Vec::new(),
            Compression::default(),
        )),
        Codec::InflateRaw => CodecState::DeflateDec(write::DeflateDecoder::new(Vec::new())),
        Codec::BrotliCompress => {
            CodecState::BrotliEnc(brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22))
        }
        Codec::BrotliDecompress => {
            CodecState::BrotliDec(brotli::DecompressorWriter::new(Vec::new(), 4096))
        }
        // Zstd streams stay buffer-until-end for this compatibility cut,
        // matching the existing `createUnzip` path.
        Codec::Unzip | Codec::ZstdCompress | Codec::ZstdDecompress => return None,
    })
}

// ── instance ops (called from dispatch_zlib_stream) ──────────────────────────

/// Convert a `.write()`/`.end()` chunk (Buffer, string, number) to bytes.
unsafe fn chunk_to_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JSValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_pointer() {
        let raw = (value.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64;
        if js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                let len = (*buf).length as usize;
                let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
            }
        }
    }
    // String (STRING_TAG / SSO short string / raw pointer) or number/bool.
    let sptr = js_get_string_pointer_unified(value) as *const StringHeader;
    if !sptr.is_null() {
        let len = (*sptr).byte_len as usize;
        if len <= (1 << 30) {
            let data = (sptr as *const u8).add(std::mem::size_of::<StringHeader>());
            return Some(std::slice::from_raw_parts(data, len).to_vec());
        }
    }
    None
}

/// `stream.write(chunk)` — feed the streaming codec and queue any output that
/// becomes available immediately (incremental 'data'). `createUnzip` buffers.
pub unsafe fn zlib_stream_write(handle: i64, chunk: f64) {
    let bytes = match chunk_to_bytes(chunk) {
        Some(b) => b,
        None => return,
    };
    let event = {
        let mut g = ZLIB_STREAMS.lock().unwrap();
        match g.get_mut(&handle) {
            Some(s) if !s.ended => {
                s.pending_bytes_written = s.pending_bytes_written.saturating_add(bytes.len());
                match s.codec_state.as_mut() {
                    Some(cs) => match cs.write_chunk(&bytes) {
                        Ok(()) => {
                            let out = cs.drain();
                            (!out.is_empty()).then(|| ZlibEvent::Data(handle, out))
                        }
                        Err(e) => Some(ZlibEvent::Error(handle, e.to_string())),
                    },
                    None => {
                        s.input.extend_from_slice(&bytes);
                        None
                    }
                }
            }
            _ => return,
        }
    };
    if let Some(ev) = event {
        ZLIB_PENDING_EVENTS.lock().unwrap().push(ev);
        perry_runtime::event_pump::js_notify_main_thread();
    }
}

/// `stream.end([chunk])` — optional final chunk, then finalize + queue events.
pub unsafe fn zlib_stream_end(handle: i64, chunk: f64) {
    zlib_stream_write(handle, chunk);
    finish_zlib_stream(handle);
}

/// `stream.flush([kind], cb?)` — emit a Z_SYNC_FLUSH block, then queue the cb.
pub fn zlib_stream_flush(handle: i64, cb: i64) {
    let data = {
        let mut g = ZLIB_STREAMS.lock().unwrap();
        match g.get_mut(&handle) {
            Some(s) if !s.ended => match s.codec_state.as_mut() {
                Some(cs) => {
                    let _ = cs.flush_codec();
                    cs.drain()
                }
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    };
    {
        let mut pending = ZLIB_PENDING_EVENTS.lock().unwrap();
        if !data.is_empty() {
            pending.push(ZlibEvent::Data(handle, data));
        }
        if cb != 0 {
            pending.push(ZlibEvent::Callback(cb));
        }
    }
    perry_runtime::event_pump::js_notify_main_thread();
}

/// `stream.params(level, strategy, cb?)` — Perry does not currently retune
/// compression levels, but Node exposes this as a function and invokes the
/// callback asynchronously when parameters are unchanged.
pub fn zlib_stream_params(_handle: i64, cb: i64) {
    if cb != 0 {
        ZLIB_PENDING_EVENTS
            .lock()
            .unwrap()
            .push(ZlibEvent::Callback(cb));
        perry_runtime::event_pump::js_notify_main_thread();
    }
}

/// `stream.reset()` — reset buffered codec state and byte accounting.
pub fn zlib_stream_reset(handle: i64) {
    let mut g = ZLIB_STREAMS.lock().unwrap();
    if let Some(s) = g.get_mut(&handle) {
        s.codec_state = make_codec_state(s.codec);
        s.input.clear();
        s.ended = false;
        s.bytes_written = 0;
        s.pending_bytes_written = 0;
    }
}

pub fn zlib_stream_bytes_written(handle: i64) -> f64 {
    ZLIB_STREAMS
        .lock()
        .unwrap()
        .get(&handle)
        .map(|s| s.bytes_written as f64)
        .unwrap_or(0.0)
}

fn publish_zlib_bytes_written(handle: i64) {
    if let Some(s) = ZLIB_STREAMS.lock().unwrap().get_mut(&handle) {
        s.bytes_written = s.pending_bytes_written;
    }
}

fn finish_zlib_stream(handle: i64) {
    let (codec_state, codec, input) = {
        let mut g = ZLIB_STREAMS.lock().unwrap();
        match g.get_mut(&handle) {
            Some(s) if !s.ended => {
                s.ended = true;
                (s.codec_state.take(), s.codec, std::mem::take(&mut s.input))
            }
            _ => return,
        }
    };
    let result = match codec_state {
        Some(cs) => cs.finish().map_err(|e| e.to_string()),
        None => run_codec(codec, &input).map_err(|e| e.to_string()), // Unzip
    };
    {
        let mut pending = ZLIB_PENDING_EVENTS.lock().unwrap();
        match result {
            Ok(out) => {
                if !out.is_empty() {
                    pending.push(ZlibEvent::Data(handle, out));
                }
                pending.push(ZlibEvent::End(handle));
            }
            Err(msg) => pending.push(ZlibEvent::Error(handle, msg)),
        }
    }
    perry_runtime::event_pump::js_notify_main_thread();
}

/// `stream.on(event, cb)` / `stream.once(event, cb)` — register a listener.
/// `event` is extracted SSO-safely via `js_get_string_pointer_unified` (short
/// names like "data"/"end" are SSO-inlined, so a raw unbox would miss them).
pub unsafe fn zlib_stream_on(handle: i64, event_value: f64, cb: i64) {
    ensure_zlib_gc_scanner();
    let event_ptr = js_get_string_pointer_unified(event_value) as *const StringHeader;
    if event_ptr.is_null() {
        return;
    }
    let len = (*event_ptr).byte_len as usize;
    let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let event = match std::str::from_utf8(std::slice::from_raw_parts(data, len)) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    ZLIB_LISTENERS
        .lock()
        .unwrap()
        .entry(handle)
        .or_default()
        .entry(event)
        .or_default()
        .push(cb);
}

/// `stream.pipe(dest)` — forward 'data' (→ dest.write) and 'end' (→ dest.end)
/// to `dest`. Stored as NaN-boxed bits; forwarding happens during the deferred
/// drain. Returns nothing here — the dispatch arm returns `dest` for chaining.
pub unsafe fn zlib_stream_pipe(handle: i64, dest: f64) {
    if let Some(s) = ZLIB_STREAMS.lock().unwrap().get_mut(&handle) {
        s.pipes.push(dest.to_bits());
    }
}

// ── pump (drained on the main thread from js_stdlib_process_pending) ─────────

extern "C" {
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    ZLIB_LISTENERS
        .lock()
        .unwrap()
        .get(&id)
        .and_then(|m| m.get(event).cloned())
        .unwrap_or_default()
}

fn pipes_for(id: i64) -> Vec<u64> {
    ZLIB_STREAMS
        .lock()
        .unwrap()
        .get(&id)
        .map(|s| s.pipes.clone())
        .unwrap_or_default()
}

unsafe fn make_buffer(bytes: &[u8]) -> Option<f64> {
    let buf = js_buffer_alloc(bytes.len() as i32, 0);
    if buf.is_null() {
        return None;
    }
    let data = (buf as *mut u8).add(std::mem::size_of::<BufferHeader>());
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
    (*buf).length = bytes.len() as u32;
    Some(f64::from_bits(JSValue::pointer(buf as *const u8).bits()))
}

/// Forward a `.pipe(dest)` chunk: `dest.write(Buffer.from(bytes))`. Builds the
/// method-name string and the chunk Buffer back-to-back (name first) so there's
/// no allocation between the Buffer's creation and the dispatch that roots it —
/// the chunk comes from an owned `Vec<u8>`, not a GC-movable source.
unsafe fn forward_write(dest_bits: u64, bytes: &[u8]) {
    let name = js_string_from_bytes(b"write".as_ptr(), 5);
    if name.is_null() {
        return;
    }
    let buf = match make_buffer(bytes) {
        Some(b) => b,
        None => return,
    };
    let args = [buf];
    js_native_call_method_str_key(f64::from_bits(dest_bits), name as i64, args.as_ptr(), 1);
}

/// Forward `.pipe(dest)` end: `dest.end()`.
unsafe fn forward_end(dest_bits: u64) {
    let name = js_string_from_bytes(b"end".as_ptr(), 3);
    if name.is_null() {
        return;
    }
    js_native_call_method_str_key(f64::from_bits(dest_bits), name as i64, std::ptr::null(), 0);
}

unsafe fn build_zlib_error(msg: &str) -> f64 {
    // `{ message: msg }` so `s.on('error', e => e.message)` works.
    use perry_runtime::JSValue as V;
    let name = b"message";
    let mut shape_id: u32 = 0x5A4C_0000; // "ZL"
    for &b in name {
        shape_id = shape_id.wrapping_mul(31).wrapping_add(b as u32);
    }
    shape_id = shape_id.wrapping_add(1);
    let s_msg = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let obj =
        perry_runtime::js_object_alloc_with_shape(shape_id, 1, name.as_ptr(), name.len() as u32);
    if obj.is_null() {
        return f64::from_bits(V::string_ptr(s_msg).bits());
    }
    perry_runtime::js_object_set_field(obj, 0, V::string_ptr(s_msg));
    f64::from_bits((obj as u64 & 0x0000_FFFF_FFFF_FFFF) | 0x7FFD_0000_0000_0000)
}

/// Drain queued zlib stream events on the main thread. Wired into
/// `js_stdlib_process_pending`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_process_pending() -> i32 {
    let events: Vec<ZlibEvent> = {
        let mut g = ZLIB_PENDING_EVENTS.lock().unwrap();
        std::mem::take(&mut *g)
    };
    let count = events.len() as i32;
    for ev in events {
        match ev {
            ZlibEvent::Data(id, bytes) => {
                publish_zlib_bytes_written(id);
                let cbs = listeners_for(id, "data");
                if !cbs.is_empty() {
                    if let Some(buf_f64) = make_buffer(&bytes) {
                        for cb in cbs {
                            if cb != 0 {
                                js_closure_call1(cb as *const ClosureHeader, buf_f64);
                            }
                        }
                    }
                }
                // Fresh Buffer per pipe dest (the chunk lives in the owned
                // `bytes`, so this is safe even after listener callbacks GC'd).
                for dest in pipes_for(id) {
                    forward_write(dest, &bytes);
                }
            }
            ZlibEvent::End(id) => {
                publish_zlib_bytes_written(id);
                for cb in listeners_for(id, "end") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                for cb in listeners_for(id, "finish") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                for dest in pipes_for(id) {
                    forward_end(dest);
                }
                for cb in listeners_for(id, "close") {
                    if cb != 0 {
                        js_closure_call0(cb as *const ClosureHeader);
                    }
                }
                ZLIB_LISTENERS.lock().unwrap().remove(&id);
                ZLIB_STREAMS.lock().unwrap().remove(&id);
            }
            ZlibEvent::Callback(cb) => {
                if cb != 0 {
                    js_closure_call0(cb as *const ClosureHeader);
                }
            }
            ZlibEvent::OneShotCallback(cb, result) => {
                if cb != 0 {
                    match result {
                        Ok(bytes) => {
                            if let Some(buf_f64) = make_buffer(&bytes) {
                                js_closure_call2(
                                    cb as *const ClosureHeader,
                                    f64::from_bits(JSValue::null().bits()),
                                    buf_f64,
                                );
                            } else {
                                let err_f64 = build_zlib_error("Buffer allocation failed");
                                js_closure_call2(
                                    cb as *const ClosureHeader,
                                    err_f64,
                                    f64::from_bits(JSValue::undefined().bits()),
                                );
                            }
                        }
                        Err(msg) => {
                            let err_f64 = build_zlib_error(&msg);
                            js_closure_call2(
                                cb as *const ClosureHeader,
                                err_f64,
                                f64::from_bits(JSValue::undefined().bits()),
                            );
                        }
                    }
                }
            }
            ZlibEvent::Error(id, msg) => {
                let err_f64 = build_zlib_error(&msg);
                for cb in listeners_for(id, "error") {
                    if cb != 0 {
                        js_closure_call1(cb as *const ClosureHeader, err_f64);
                    }
                }
                ZLIB_LISTENERS.lock().unwrap().remove(&id);
                ZLIB_STREAMS.lock().unwrap().remove(&id);
            }
        }
    }
    count
}

/// Keep the event loop alive while zlib stream events are queued. Wired into
/// `js_stdlib_has_active_handles`.
pub fn js_zlib_has_active_handles() -> i32 {
    if !ZLIB_PENDING_EVENTS.lock().unwrap().is_empty() {
        1
    } else {
        0
    }
}

/// Method-name dispatcher for `node:zlib` — invoked by the runtime when a
/// captured zlib method is called via `js_native_call_method` (the path
/// `util.promisify(zlib.gzip)` and `const f = zlib.gzipSync; f(...)` take).
/// The codegen NATIVE_MODULE_TABLE handles direct call sites; this is the
/// runtime-side mirror so indirect callers reach the same FFIs.
///
/// `method`/`method_len` is the UTF-8 method name; `args`/`args_len` is the
/// raw NaN-boxed argument array. Returns the NaN-boxed result. Unknown
/// methods return `undefined`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_native_dispatch(
    method: *const u8,
    method_len: usize,
    args: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(JSValue::undefined().bits());
    if method.is_null() || method_len == 0 {
        return undefined;
    }
    let name = std::str::from_utf8(std::slice::from_raw_parts(method, method_len)).unwrap_or("");
    let arg = |i: usize| -> f64 {
        if i < args_len && !args.is_null() {
            *args.add(i)
        } else {
            undefined
        }
    };
    // Helper: pointer return → POINTER_TAG NaN-box (matches NR_PTR).
    let ptr_to_f64 = |p: *const u8| -> f64 {
        if p.is_null() {
            undefined
        } else {
            f64::from_bits(0x7FFD_0000_0000_0000u64 | ((p as u64) & 0x0000_FFFF_FFFF_FFFF))
        }
    };
    match name {
        // Sync codecs — all take 1 string/buffer-like JS value, return Buffer pointer.
        "gzipSync" => ptr_to_f64(js_zlib_gzip_sync(arg(0).to_bits() as i64, arg(1)) as *const u8),
        "gunzipSync" => ptr_to_f64(js_zlib_gunzip_sync(arg(0).to_bits() as i64) as *const u8),
        "deflateSync" => {
            ptr_to_f64(js_zlib_deflate_sync(arg(0).to_bits() as i64, arg(1)) as *const u8)
        }
        "inflateSync" => ptr_to_f64(js_zlib_inflate_sync(arg(0).to_bits() as i64) as *const u8),
        "deflateRawSync" => ptr_to_f64(js_zlib_deflate_raw_sync(arg(0)) as *const u8),
        "inflateRawSync" => ptr_to_f64(js_zlib_inflate_raw_sync(arg(0)) as *const u8),
        "unzipSync" => ptr_to_f64(js_zlib_unzip_sync(arg(0)) as *const u8),
        "brotliCompressSync" => {
            ptr_to_f64(js_zlib_brotli_compress_sync(arg(0).to_bits() as i64) as *const u8)
        }
        "brotliDecompressSync" => {
            ptr_to_f64(js_zlib_brotli_decompress_sync(arg(0).to_bits() as i64) as *const u8)
        }
        "zstdCompressSync" => ptr_to_f64(js_zlib_zstd_compress_sync(arg(0), arg(1)) as *const u8),
        "zstdDecompressSync" => {
            ptr_to_f64(js_zlib_zstd_decompress_sync(arg(0), arg(1)) as *const u8)
        }
        "crc32" => {
            let seed = if args_len >= 2 { arg(1) } else { 0.0 };
            js_zlib_crc32(arg(0), seed)
        }
        // Callback-form one-shot codecs — validate the trailing callback and return undefined.
        "gzip" => {
            js_zlib_gzip(arg(0), arg(1));
            undefined
        }
        "gunzip" => {
            js_zlib_gunzip(arg(0), arg(1));
            undefined
        }
        "deflate" => {
            js_zlib_deflate(arg(0), arg(1));
            undefined
        }
        "inflate" => {
            js_zlib_inflate(arg(0), arg(1));
            undefined
        }
        "deflateRaw" => {
            js_zlib_deflate_raw(arg(0), arg(1));
            undefined
        }
        "inflateRaw" => {
            js_zlib_inflate_raw(arg(0), arg(1));
            undefined
        }
        "unzip" => {
            js_zlib_unzip(arg(0), arg(1));
            undefined
        }
        "brotliCompress" => {
            js_zlib_brotli_compress(arg(0), arg(1));
            undefined
        }
        "brotliDecompress" => {
            js_zlib_brotli_decompress(arg(0), arg(1));
            undefined
        }
        "zstdCompress" => {
            js_zlib_zstd_compress(arg(0), arg(1));
            undefined
        }
        "zstdDecompress" => {
            js_zlib_zstd_decompress(arg(0), arg(1));
            undefined
        }
        "createZstdCompress" => ptr_to_f64(js_zlib_create_zstd_compress(arg(0)) as *const u8),
        "createZstdDecompress" => ptr_to_f64(js_zlib_create_zstd_decompress(arg(0)) as *const u8),
        _ => undefined,
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[test]
    fn zlib_stream_ids_live_in_dedicated_small_handle_band() {
        let id = next_zlib_id();
        assert!(
            (ZLIB_STREAM_HANDLE_ID_START..ZLIB_STREAM_HANDLE_ID_END).contains(&id),
            "zlib stream id {id:#x} must stay in the zlib small-handle band"
        );
    }

    /// Drive the streaming codec like the FFI ops do: write + drain, flush +
    /// drain between chunks, then finish — reassembling the full stream.
    fn stream_compress(codec: Codec, chunks: &[&[u8]]) -> Vec<u8> {
        let mut cs = make_codec_state(codec).expect("streaming codec");
        let mut out = Vec::new();
        for c in chunks {
            cs.write_chunk(c).unwrap();
            out.extend(cs.drain());
            cs.flush_codec().unwrap();
            out.extend(cs.drain());
        }
        out.extend(cs.finish().unwrap());
        out
    }

    #[test]
    fn gzip_stream_roundtrips() {
        let c = stream_compress(Codec::Gzip, &[b"hello ", b"streaming ", b"world"]);
        assert_eq!(&c[..2], &[0x1f, 0x8b]);
        assert_eq!(
            run_codec(Codec::Gunzip, &c).unwrap(),
            b"hello streaming world"
        );
    }

    #[test]
    fn deflate_stream_is_zlib_format_and_roundtrips() {
        let c = stream_compress(Codec::Deflate, &[b"AAAA", b"BBBB"]);
        assert_eq!(c[0], 0x78);
        assert_eq!(run_codec(Codec::Inflate, &c).unwrap(), b"AAAABBBB");
    }

    #[test]
    fn brotli_stream_roundtrips() {
        let c = stream_compress(Codec::BrotliCompress, &[b"brotli ", b"stream ", b"test"]);
        assert_eq!(
            run_codec(Codec::BrotliDecompress, &c).unwrap(),
            b"brotli stream test"
        );
    }

    #[test]
    fn zstd_one_shot_roundtrips() {
        let c = run_one_shot_codec(Codec::ZstdCompress, b"zstd one-shot test").unwrap();
        assert!(!c.is_empty());
        assert_eq!(
            run_one_shot_codec(Codec::ZstdDecompress, &c).unwrap(),
            b"zstd one-shot test"
        );
    }

    #[test]
    fn zstd_buffer_until_end_stream_roundtrips() {
        assert!(make_codec_state(Codec::ZstdCompress).is_none());
        assert!(make_codec_state(Codec::ZstdDecompress).is_none());

        let c = run_codec(Codec::ZstdCompress, b"zstd stream test").unwrap();
        assert!(!c.is_empty());
        assert_eq!(
            run_codec(Codec::ZstdDecompress, &c).unwrap(),
            b"zstd stream test"
        );
    }
}
