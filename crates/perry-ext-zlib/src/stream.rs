//! Node `zlib` Transform-stream objects + Brotli one-shots (#1843).
//!
//! `zlib.createGzip()` / `createGunzip()` / `createDeflate()` /
//! `createInflate()` / `createDeflateRaw()` / `createInflateRaw()` /
//! `createUnzip()` / `createBrotliCompress()` / `createBrotliDecompress()`
//! return small-int handles (base 0x60000, under the 0x100000 small-handle
//! dispatch threshold) that the codegen NaN-boxes with POINTER_TAG.
//! Subsequent `s.write()` / `s.end()` / `s.on()` / `s.pipe()` calls lose
//! their static type and route through perry-runtime's
//! `js_native_call_method` → HANDLE_METHOD_DISPATCH → perry-stdlib's
//! external-zlib-pump arm → `js_ext_zlib_dispatch_method` here.
//!
//! Codec work is deferred onto the agent pump. Each stream owns its input and
//! bounded readable queue; paused consumers leave the codec suspended there.

use perry_ffi::{
    alloc_buffer, alloc_string, notify_main_thread, register_agent_event_pump, BufferHeader,
    ErrorKind, GcRootVisitor, JsClosure, JsValue, RawClosureHeader, StringHeader,
    TransientRootScope, TransientRootedAddr,
};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::io::{Read, Write};

mod one_shot_callback;
pub(crate) use one_shot_callback::queue_one_shot_callback;

#[cfg(test)]
use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzEncoder, MultiGzDecoder, ZlibDecoder, ZlibEncoder,
};
use flate2::Compression;

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TRUE_BITS: u64 = 0x7FFC_0000_0000_0004;

// perry-runtime `#[no_mangle]` symbols, resolved at final link (perry-runtime
// is always linked). Mirrors perry-ext-net's extern usage.
extern "C" {
    fn js_zlib_stream_error(message: *const u8, len: usize, truncated: i32) -> f64;
    fn js_zlib_is_callback(value: f64) -> i32;
    fn js_zlib_stream_iterator(stream: f64, options: f64) -> f64;
    fn js_zlib_stream_option(opts: f64, which: i32) -> usize;
    fn js_zlib_pipe_drain_callback(stream: f64) -> i64;
    fn js_buffer_is_buffer(ptr: i64) -> i32;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    // #2935: resolve + validate a `{ level }` option to a flate2 level
    // (`0..=9`); throws `RangeError [ERR_OUT_OF_RANGE]` for out-of-range
    // values. Lives in perry-runtime (it owns the by-name object reader + the
    // throwing path). `js_zlib_resolve_level(undefined)` returns the default.
    pub(crate) fn js_zlib_resolve_level(opts: f64) -> i32;
    // #3285: validate `.params(level, strategy)` args, returning the clamped
    // flate2 level (`0..=9`). Throws `TypeError [ERR_INVALID_ARG_TYPE]` for a
    // non-numeric arg and `RangeError [ERR_OUT_OF_RANGE]` for an out-of-range
    // level/strategy, matching Node — the throwing path lives in perry-runtime.
    pub(crate) fn js_zlib_validate_params(level: f64, strategy: f64) -> i32;
    // #3662: validate the full options object (windowBits/level/memLevel/
    // strategy/chunkSize/flush) the way Node's Zlib constructor does, throwing
    // the spec `TypeError`/`RangeError` before any compression runs.
    // `min_window_bits` is 9 for gzip compression, 8 for every other codec.
    pub(crate) fn js_zlib_validate_options(opts: f64, min_window_bits: i32);
    // #3662: reject a non-string/non-Buffer/TypedArray/DataView/ArrayBuffer
    // `buffer` argument with `TypeError [ERR_INVALID_ARG_TYPE]` before reading
    // any bytes. The in-tree codecs validate inline; this shared helper gives
    // the ext crate the same rejection without the runtime's value typing.
    pub(crate) fn js_zlib_validate_buffer_arg(data_bits: i64);
    // Async one-shot zlib helpers require a callable callback and throw
    // synchronously before queuing codec work.
    pub(crate) fn js_zlib_validate_callback(callback: f64) -> i64;
    fn js_async_hooks_provider_init(type_ptr: *const u8, type_len: usize) -> u64;
    fn js_async_hooks_provider_run_catching(
        async_id: u64,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
    fn js_async_hooks_provider_run_catching_deferred_destroy(
        async_id: u64,
        check_turns: u32,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
    fn js_async_hooks_provider_run_catching_deferred_destroy_on_error(
        async_id: u64,
        check_turns: u32,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
}

extern "C" fn process_pending_aux() -> i32 {
    unsafe { js_ext_zlib_process_pending() }
}

fn ensure_aux_pump_registered() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        // The queues are per agent (see `statics`), so a worker drains its own.
        register_agent_event_pump(process_pending_aux, js_ext_zlib_has_active_handles);
    });
}

// ── Brotli one-shots (#1843 cluster 2) ───────────────────────────────────────

fn brotli_compress_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut r = brotli::CompressorReader::new(data, 4096, 11, 22);
    let _ = r.read_to_end(&mut out);
    out
}

fn brotli_decompress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    brotli::Decompressor::new(data, 4096).read_to_end(&mut out)?;
    Ok(out)
}

fn throw_brotli_decode_error() -> ! {
    perry_ffi::throw_with_code(
        "Decompression failed",
        "ERR__ERROR_FORMAT_PADDING_2",
        ErrorKind::Error,
    )
}

/// Read the bytes of a one-shot input argument. Node's `gzipSync` / `gunzipSync`
/// / `brotli*Sync` accept BOTH strings and Buffers/Uint8Arrays; the codegen
/// unboxes either to a raw pointer typed `*const StringHeader`. A real Buffer is
/// a `BufferHeader` (length at offset 0), so reading it as a `StringHeader`
/// (byte_len at offset 4) corrupts the length. Probe the buffer registry first
/// (#1843 — `gunzipSync(Buffer.concat(chunks))` / `gunzipSync(fs.readFileSync)`).
pub(crate) unsafe fn read_input_bytes(ptr: *const StringHeader) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    if js_buffer_is_buffer(ptr as i64) != 0 {
        let buf = ptr as *const BufferHeader;
        return Some(perry_ffi::read_buffer_bytes(buf).unwrap_or(&[]).to_vec());
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    Some(std::slice::from_raw_parts(data, len).to_vec())
}

/// Read the bytes of a one-shot input passed as raw NaN-box bits (#2935).
///
/// `gzipSync`/`deflateSync` now receive the data argument as `i64` NaN-box
/// bits (NA_JSV) rather than a pre-unboxed pointer, so the codec can accept a
/// string, Buffer, or TypedArray uniformly. `js_get_string_pointer_unified`
/// recovers the underlying `StringHeader`/`BufferHeader` pointer (masking the
/// POINTER/STRING tag), which `read_input_bytes` then reads buffer-aware.
///
/// # Safety
/// `data_bits` must be a valid NaN-box bit pattern from the runtime.
pub(crate) unsafe fn read_input_from_bits(data_bits: i64) -> Option<Vec<u8>> {
    let ptr = js_get_string_pointer_unified(f64::from_bits(data_bits as u64));
    if ptr == 0 {
        return None;
    }
    read_input_bytes(ptr as *const StringHeader)
}

/// Resolve a `node:zlib` option object to a `flate2::Compression` level.
///
/// Delegates the read + range validation to perry-runtime's
/// `js_zlib_resolve_level` (#2935): an out-of-range `level` throws a
/// Node-compatible `RangeError` (via longjmp) before this returns, and an
/// absent/`undefined` `level` yields the zlib default level (`6`).
pub(crate) unsafe fn compression_from_opts(opts: f64) -> Compression {
    Compression::new(js_zlib_resolve_level(opts) as u32)
}

/// `zlib.brotliCompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_bits` must be the raw NaN-box bit pattern of the data argument.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress_sync(data_bits: i64) -> *mut BufferHeader {
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits) {
        Some(d) => alloc_buffer(&brotli_compress_bytes(&d)),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.brotliDecompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_bits` must be the raw NaN-box bit pattern of the data argument.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_sync(data_bits: i64) -> *mut BufferHeader {
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits).map(|d| brotli_decompress_bytes(&d)) {
        Some(Ok(out)) => alloc_buffer(&out),
        Some(Err(_)) => throw_brotli_decode_error(),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.brotliCompress(data, options?, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress(
    data_value: f64,
    options: f64,
    callback_value: f64,
) {
    queue_one_shot_callback(
        data_value,
        options,
        callback_value,
        "BrotliCompress",
        |b, _level| Ok(brotli_compress_bytes(b)),
    );
}

/// `zlib.brotliDecompress(data, options?, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress(
    data_value: f64,
    options: f64,
    callback_value: f64,
) {
    queue_one_shot_callback(
        data_value,
        options,
        callback_value,
        "BrotliDecompress",
        |b, _level| brotli_decompress_bytes(b),
    );
}

fn throw_zstd_error(err: &std::io::Error) -> ! {
    perry_ffi::throw_with_code(&format!("zstd: {}", err), "Z_DATA_ERROR", ErrorKind::Error)
}

/// `zlib.zstdCompressSync(data)` -> Buffer. `_opts` is accepted (codegen
/// passes the options slot through) but zstd params are not wired up —
/// matches perry-stdlib's copy.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress_sync(
    data_value: f64,
    _opts: f64,
) -> *mut BufferHeader {
    let data_bits = data_value.to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits)
        .map(|d| zstd::stream::encode_all(d.as_slice(), ZSTD_DEFAULT_LEVEL))
    {
        Some(Ok(out)) => alloc_buffer(&out),
        Some(Err(e)) => throw_zstd_error(&e),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.zstdDecompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress_sync(
    data_value: f64,
    _opts: f64,
) -> *mut BufferHeader {
    let data_bits = data_value.to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits).map(|d| zstd::stream::decode_all(d.as_slice())) {
        Some(Ok(out)) => alloc_buffer(&out),
        Some(Err(e)) => throw_zstd_error(&e),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.zstdCompress(data, options?, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress(data_value: f64, options: f64, callback_value: f64) {
    queue_one_shot_callback(
        data_value,
        options,
        callback_value,
        "ZstdCompress",
        |b, _level| zstd::stream::encode_all(b, ZSTD_DEFAULT_LEVEL),
    );
}

/// `zlib.zstdDecompress(data, options?, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress(
    data_value: f64,
    options: f64,
    callback_value: f64,
) {
    queue_one_shot_callback(
        data_value,
        options,
        callback_value,
        "ZstdDecompress",
        |b, _level| zstd::stream::decode_all(b),
    );
}

// ── stream codec ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
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

/// Node's `zlib` zstd default (matches perry-stdlib's copy). zstd levels run
/// 1..=22 and don't share the deflate 0..=9 scale, so the `{ level }` option
/// resolved by `js_zlib_resolve_level` is not applied to zstd codecs.
const ZSTD_DEFAULT_LEVEL: i32 = 3;

#[cfg(test)]
fn run_codec(codec: Codec, input: &[u8]) -> std::io::Result<Vec<u8>> {
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
            // Node's `createUnzip` auto-detects gzip vs zlib by header.
            if input.len() >= 2 && input[0] == 0x1f && input[1] == 0x8b {
                MultiGzDecoder::new(input).read_to_end(&mut out)?;
            } else {
                ZlibDecoder::new(input).read_to_end(&mut out)?;
            }
        }
        Codec::BrotliCompress => out = brotli_compress_bytes(input),
        Codec::BrotliDecompress => out = brotli_decompress_bytes(input)?,
        Codec::ZstdCompress => out = zstd::stream::encode_all(input, ZSTD_DEFAULT_LEVEL)?,
        Codec::ZstdDecompress => out = zstd::stream::decode_all(input)?,
    }
    Ok(out)
}

// ── streaming codec state ────────────────────────────────────────────────────
//
// Stateful write-codec backing a stream handle: fed incrementally by `.write()`,
// flushed by `.flush()`, finalized by `.end()`. flate2's write-encoders compress
// on write and emit a Z_SYNC_FLUSH block on `flush()`; brotli's CompressorWriter
// does the same via BROTLI_OPERATION_FLUSH and runs BROTLI_OPERATION_FINISH on
// `into_inner()`. `None` for `createUnzip` (gzip/zlib auto-detect isn't a
// streaming write-codec, so it stays buffer-until-end via `run_codec`).

enum CodecState {
    GzEnc(flate2::write::GzEncoder<Vec<u8>>),
    GzDec(flate2::write::GzDecoder<Vec<u8>>),
    ZlibEnc(flate2::write::ZlibEncoder<Vec<u8>>),
    ZlibDec(flate2::write::ZlibDecoder<Vec<u8>>),
    DeflateEnc(flate2::write::DeflateEncoder<Vec<u8>>),
    DeflateDec(flate2::write::DeflateDecoder<Vec<u8>>),
    BrotliEnc(brotli::CompressorWriter<Vec<u8>>),
    BrotliDec(brotli::DecompressorWriter<Vec<u8>>),
    ZstdEnc(zstd::stream::write::Encoder<'static, Vec<u8>>),
    ZstdDec(zstd::stream::write::Decoder<'static, Vec<u8>>),
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
            CodecState::ZstdEnc(w) => w.write_all(data),
            CodecState::ZstdDec(w) => w.write_all(data),
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
            CodecState::ZstdEnc(w) => w.flush(),
            CodecState::ZstdDec(w) => w.flush(),
        }
    }

    /// Take the output produced since the last drain (the inner `Vec<u8>`).
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
            CodecState::ZstdEnc(w) => std::mem::take(w.get_mut()),
            CodecState::ZstdDec(w) => std::mem::take(w.get_mut()),
        }
    }

    /// Finalize the stream, returning the remaining output (since the last drain).
    fn finish(self) -> std::io::Result<Vec<u8>> {
        match self {
            CodecState::GzEnc(w) => w.finish(),
            CodecState::GzDec(w) => w.finish(),
            CodecState::ZlibEnc(w) => w.finish(),
            CodecState::ZlibDec(w) => w.finish(),
            CodecState::DeflateEnc(w) => w.finish(),
            CodecState::DeflateDec(w) => w.finish(),
            CodecState::BrotliEnc(w) => Ok(w.into_inner()),
            // DecompressorWriter::into_inner returns Result<W, W> (Err on an
            // unterminated stream); take the decoded bytes either way.
            CodecState::BrotliDec(w) => Ok(w.into_inner().unwrap_or_else(|v| v)),
            // Encoder::finish writes the zstd frame epilogue then hands back
            // the inner Vec; Decoder::into_inner is tolerant of an
            // unterminated frame (same stance as BrotliDec above).
            CodecState::ZstdEnc(w) => w.finish(),
            CodecState::ZstdDec(mut w) => {
                w.flush()?;
                Ok(w.into_inner())
            }
        }
    }
}

#[allow(dead_code)] // test scaffolding: default-level wrapper used only by the cfg(test) streaming tests
fn make_codec_state(codec: Codec) -> Option<CodecState> {
    make_codec_state_with_level(codec, Compression::default())
}

/// Build the streaming codec for `codec` at compression `level`. Only the
/// deflate-family encoders (gzip/zlib/raw-deflate) honor `level`; decoders and
/// brotli ignore it. Used by both `create_stream` (initial `{ level }`) and
/// `stream_params` (#3285, mid-stream retune before any data is written).
fn make_codec_state_with_level(codec: Codec, level: Compression) -> Option<CodecState> {
    use flate2::write;
    Some(match codec {
        Codec::Gzip => CodecState::GzEnc(write::GzEncoder::new(Vec::new(), level)),
        Codec::Gunzip => CodecState::GzDec(write::GzDecoder::new(Vec::new())),
        Codec::Deflate => CodecState::ZlibEnc(write::ZlibEncoder::new(Vec::new(), level)),
        Codec::Inflate => CodecState::ZlibDec(write::ZlibDecoder::new(Vec::new())),
        Codec::DeflateRaw => CodecState::DeflateEnc(write::DeflateEncoder::new(Vec::new(), level)),
        Codec::InflateRaw => CodecState::DeflateDec(write::DeflateDecoder::new(Vec::new())),
        Codec::BrotliCompress => {
            CodecState::BrotliEnc(brotli::CompressorWriter::new(Vec::new(), 4096, 11, 22))
        }
        Codec::BrotliDecompress => {
            CodecState::BrotliDec(brotli::DecompressorWriter::new(Vec::new(), 4096))
        }
        // zstd context allocation is fallible; `None` falls back to the same
        // buffer-until-end `run_codec` path `createUnzip` uses, so a failed
        // allocation degrades to one-shot semantics instead of erroring.
        Codec::ZstdCompress => CodecState::ZstdEnc(
            zstd::stream::write::Encoder::new(Vec::new(), ZSTD_DEFAULT_LEVEL).ok()?,
        ),
        Codec::ZstdDecompress => {
            CodecState::ZstdDec(zstd::stream::write::Decoder::new(Vec::new()).ok()?)
        }
        // Unzip auto-detects the header — kept buffer-until-end (run_codec).
        Codec::Unzip => return None,
    })
}

// ── registry ─────────────────────────────────────────────────────────────────

struct ZlibStreamState {
    async_id: u64,
    codec: Codec,
    level: Compression,
    /// Streaming codec, fed incrementally. `None` for `createUnzip` (uses
    /// `input` + `run_codec` on `.end()`) or once finalized.
    codec_state: Option<CodecState>,
    ended: bool,
    /// Set once any chunk has been fed. `.params()` can only rebuild the
    /// encoder at a new level (flate2 has no mid-stream `deflateParams`) before
    /// this flips; after data is written it validates + flushes only (#3285).
    wrote_data: bool,
    bytes_written: usize,
    /// `.pipe(dest)` destinations as NaN-boxed bits; 'data'/'end' forward here.
    pipes: Vec<u64>,
    driver: Driver,
    destroyed: bool,
    readable_ended: bool,
    finished: bool,
    error: Option<f64>,
    end_callbacks: Vec<i64>,
}

enum ZlibEvent {
    Pump(i64),
    Drain(i64),
    Close(i64),
    Data(i64, Vec<u8>),
    Finish(i64),
    End(i64),
    DestroyError(i64),
    /// `.flush(cb)` completion callback — invoked (0 args) after its flushed
    /// 'data' is delivered.
    Callback(i64),
    WriteError(i64, i64),
    /// `zlib.gzip(data, cb)` style one-shot completion callback.
    OneShotCallback(i64, Result<Vec<u8>, String>, u64),
}

pub(crate) struct Statics {
    streams: HashMap<i64, ZlibStreamState>,
    listeners: HashMap<i64, HashMap<String, Vec<(i64, bool)>>>,
    pending: VecDeque<ZlibEvent>,
    next_id: i64,
}

mod driver;
use driver::{Driver, Work};
mod agent_state;
use agent_state::ensure_gc_scanner_registered;
pub(crate) use agent_state::statics;

pub(super) fn scan_zlib_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut s) = statics().lock() {
        for per_stream in s.listeners.values_mut() {
            for cb_vec in per_stream.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(&mut cb.0);
                }
            }
        }
        for stream in s.streams.values_mut() {
            stream.driver.scan(visitor);
            for cb in &mut stream.end_callbacks {
                visitor.visit_i64_slot(cb);
            }
            if let Some(error) = &mut stream.error {
                visitor.visit_nanbox_f64_slot(error);
            }
            for dest in &mut stream.pipes {
                visitor.visit_nanbox_u64_slot(dest);
            }
        }
        // Queued callbacks are referenced only here — root them too, same
        // hazard as listeners.
        for ev in s.pending.iter_mut() {
            match ev {
                ZlibEvent::Callback(cb)
                | ZlibEvent::OneShotCallback(cb, _, _)
                | ZlibEvent::WriteError(_, cb) => {
                    visitor.visit_i64_slot(cb);
                }
                _ => {}
            }
        }
    }
}

fn create_stream(
    codec: Codec,
    level: Compression,
    chunk_size: usize,
    readable_hwm: usize,
    writable_hwm: usize,
) -> i64 {
    // External zlib owns its event queue, so register it directly with the
    // runtime when the first stream is created. In particular, do not rely on
    // perry-stdlib's async pump registration: zlib streams are synchronous and
    // forcing the Tokio runtime just to deliver their deferred events is both
    // unnecessary and unsafe in stripped well-known-wrapper builds.
    ensure_aux_pump_registered();
    ensure_gc_scanner_registered();
    let async_id = unsafe { js_async_hooks_provider_init(b"ZLIB".as_ptr(), b"ZLIB".len()) };
    let driver = Driver::new(codec, chunk_size, readable_hwm, writable_hwm);
    let codec_state = if driver.is_decoder() {
        None
    } else {
        make_codec_state_with_level(codec, level)
    };
    let mut s = statics().lock().unwrap();
    let id = s.next_id;
    s.next_id += 1;
    s.streams.insert(
        id,
        ZlibStreamState {
            async_id,
            codec,
            level,
            codec_state,
            ended: false,
            wrote_data: false,
            bytes_written: 0,
            pipes: Vec::new(),
            driver,
            destroyed: false,
            readable_ended: false,
            finished: false,
            error: None,
            end_callbacks: Vec::new(),
        },
    );
    id
}

// ── factories ────────────────────────────────────────────────────────────────

macro_rules! factory {
    // `$min_wb` is the lower `windowBits` bound for option validation (#3662):
    // 9 for gzip compression, 8 for every other deflate-family codec, and 0 to
    // skip zlib option validation entirely (brotli has its own option shape).
    ($name:ident, $codec:expr, $min_wb:expr) => {
        /// # Safety
        /// FFI entry; `opts` is the NaN-boxed options object. It is validated
        /// the way Node's constructor does (#3662), then its `{ level }` (if
        /// present) sets the initial compression level for deflate-family
        /// encoders.
        #[no_mangle]
        pub unsafe extern "C" fn $name(opts: f64) -> i64 {
            let roots = TransientRootScope::enter();
            let opts = roots.root_nanbox(opts);
            if $min_wb != 0 {
                js_zlib_validate_options(opts.get(), $min_wb);
            }
            let level = Compression::new(js_zlib_resolve_level(opts.get()) as u32);
            let chunk_size = js_zlib_stream_option(opts.get(), 0);
            let readable_hwm = js_zlib_stream_option(opts.get(), 1);
            let writable_hwm = js_zlib_stream_option(opts.get(), 2);
            create_stream($codec, level, chunk_size, readable_hwm, writable_hwm)
        }
    };
}
factory!(js_zlib_create_gzip, Codec::Gzip, 9);
factory!(js_zlib_create_gunzip, Codec::Gunzip, 8);
factory!(js_zlib_create_deflate, Codec::Deflate, 8);
factory!(js_zlib_create_inflate, Codec::Inflate, 8);
factory!(js_zlib_create_deflate_raw, Codec::DeflateRaw, 8);
factory!(js_zlib_create_inflate_raw, Codec::InflateRaw, 8);
factory!(js_zlib_create_unzip, Codec::Unzip, 8);
factory!(js_zlib_create_brotli_compress, Codec::BrotliCompress, 0);
factory!(js_zlib_create_brotli_decompress, Codec::BrotliDecompress, 0);
factory!(js_zlib_create_zstd_compress, Codec::ZstdCompress, 0);
factory!(js_zlib_create_zstd_decompress, Codec::ZstdDecompress, 0);

// ── chunk / buffer helpers ─────────────────────────────────────────────────────

/// Convert a `.write()`/`.end()` chunk (Buffer, string, number) to bytes.
unsafe fn chunk_to_bytes(value: f64) -> Option<Vec<u8>> {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_undefined() || v.is_null() {
        return None;
    }
    if v.is_pointer() {
        let raw = (value.to_bits() & POINTER_MASK) as i64;
        if js_buffer_is_buffer(raw) != 0 {
            let buf = raw as *const BufferHeader;
            if !buf.is_null() {
                return Some(perry_ffi::read_buffer_bytes(buf).unwrap_or(&[]).to_vec());
            }
        }
    }
    // String (STRING_TAG / SSO / raw) or number/bool — SSO-safe.
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

unsafe fn make_buffer_f64(bytes: &[u8]) -> Option<f64> {
    let buf = alloc_buffer(bytes);
    if buf.is_null() {
        return None;
    }
    Some(f64::from_bits(POINTER_TAG | (buf as u64 & POINTER_MASK)))
}

unsafe fn call_one_shot_callback(callback: i64, result: Result<Vec<u8>, String>) {
    if callback == 0 {
        return;
    }
    let roots = TransientRootScope::enter();
    let callback = roots.root_addr(callback);
    match result {
        Ok(bytes) => {
            let err = f64::from_bits(JsValue::NULL.bits());
            let out = roots.root_nanbox(
                make_buffer_f64(&bytes)
                    .unwrap_or_else(|| f64::from_bits(JsValue::UNDEFINED.bits())),
            );
            let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader).call2(
                perry_ffi::JsThis::UNDEFINED,
                err,
                out.get(),
            );
        }
        Err(msg) => {
            let err = roots.root_nanbox(build_error_object(&msg));
            let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader).call2(
                perry_ffi::JsThis::UNDEFINED,
                err.get(),
                f64::from_bits(JsValue::UNDEFINED.bits()),
            );
        }
    }
}

unsafe fn event_name(value: f64) -> Option<String> {
    let ptr = js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(data, len))
        .ok()
        .map(|s| s.to_string())
}

// ── instance ops ───────────────────────────────────────────────────────────────

fn schedule(g: &mut Statics, handle: i64) {
    if let Some(s) = g.streams.get_mut(&handle) {
        if !s.driver.scheduled && !s.destroyed {
            s.driver.scheduled = true;
            g.pending.push_back(ZlibEvent::Pump(handle));
        }
    }
}

fn stream_write(handle: i64, bytes: Vec<u8>, cb: i64) -> bool {
    let mut g = statics().lock().unwrap();
    let accepted = match g.streams.get_mut(&handle) {
        Some(s) if !s.ended && !s.destroyed => {
            s.wrote_data = true;
            s.driver.input_bytes += bytes.len();
            s.driver.work.push_back(Work::Write(bytes, 0, cb));
            let accepted = s.driver.input_bytes < s.driver.writable_hwm;
            s.driver.need_drain |= !accepted;
            accepted
        }
        _ => {
            return false;
        }
    };
    schedule(&mut g, handle);
    drop(g);
    notify_main_thread();
    accepted
}

fn stream_flush(handle: i64, cb: i64) {
    let mut g = statics().lock().unwrap();
    if let Some(s) = g.streams.get_mut(&handle) {
        if !s.ended && !s.destroyed {
            s.driver.work.push_back(Work::Flush(cb));
        }
    }
    schedule(&mut g, handle);
    drop(g);
    notify_main_thread();
}

/// `.params(level, strategy, cb?)` (#3285) — validate the args (throwing
/// Node-compatible errors on bad input), retune subsequent compression, then
/// queue the callback.
///
/// `js_zlib_validate_params` runs first and may `js_throw` (longjmp) — so it
/// MUST run before we take the registry lock, or a thrown error would leave the
/// mutex poisoned. flate2 exposes no mid-stream `deflateParams`, so retuning is
/// modeled by rebuilding the encoder at the new level when no data has been
/// written yet (the common case: `params()` before the first `write`). After
/// data is written we only validate + flush, since the already-emitted bytes
/// can't be relevelled. Decoders/brotli ignore the level (matching the encoder
/// the codec was created with).
unsafe fn stream_params(handle: i64, level: f64, strategy: f64, cb: i64) {
    // Validates + clamps; diverges via js_throw on a bad level/strategy.
    let clamped = js_zlib_validate_params(level, strategy);
    let mut g = statics().lock().unwrap();
    if let Some(s) = g.streams.get_mut(&handle) {
        if !s.ended && !s.wrote_data {
            let level = Compression::new(clamped as u32);
            s.level = level;
            if !s.driver.is_decoder() {
                s.codec_state = make_codec_state_with_level(s.codec, level);
            }
        }
        if !s.ended {
            s.driver.work.push_back(Work::Flush(cb));
        }
    }
    schedule(&mut g, handle);
    drop(g);
    notify_main_thread();
}

fn stream_reset(handle: i64) {
    let mut g = statics().lock().unwrap();
    if let Some(s) = g.streams.get_mut(&handle) {
        s.driver = Driver::new(
            s.codec,
            s.driver.chunk_size,
            s.driver.readable_hwm,
            s.driver.writable_hwm,
        );
        s.codec_state = if s.driver.is_decoder() {
            None
        } else {
            make_codec_state_with_level(s.codec, s.level)
        };
        s.ended = false;
        s.wrote_data = false;
        s.bytes_written = 0;
    }
}

fn stream_bytes_written(handle: i64) -> f64 {
    statics()
        .lock()
        .unwrap()
        .streams
        .get(&handle)
        .map(|s| s.bytes_written as f64)
        .unwrap_or(0.0)
}

fn finish_stream(handle: i64) {
    let mut g = statics().lock().unwrap();
    if let Some(s) = g.streams.get_mut(&handle) {
        if !s.ended && !s.destroyed {
            s.ended = true;
            s.driver.work.push_back(Work::End);
            schedule(&mut g, handle);
        }
    }
    drop(g);
    notify_main_thread();
}

fn destroy_stream(handle: i64, message: Option<String>) -> bool {
    let roots = TransientRootScope::enter();
    let error = message
        .as_ref()
        .map(|msg| roots.root_nanbox(unsafe { build_error_object(msg) }));
    let mut g = statics().lock().unwrap();
    let Some(s) = g.streams.get_mut(&handle) else {
        return false;
    };
    if s.destroyed {
        return false;
    }
    s.destroyed = true;
    if let Some(error) = &error {
        s.error = Some(error.get());
    }
    let mut callbacks = s.driver.cancel_callbacks();
    callbacks.append(&mut s.end_callbacks);
    s.codec_state = None;
    s.pipes.clear();
    s.driver.work.clear();
    s.driver.output.clear();
    s.driver.output_bytes = 0;
    s.driver.done = true;
    // Drop decoder and its compressed input immediately.
    s.driver = Driver::new(
        Codec::Gzip,
        s.driver.chunk_size,
        s.driver.readable_hwm,
        s.driver.writable_hwm,
    );
    g.pending.retain(|e| event_stream_handle(e) != Some(handle));
    for cb in callbacks {
        g.pending.push_back(ZlibEvent::WriteError(handle, cb));
    }
    if message.is_some() {
        g.pending.push_back(ZlibEvent::DestroyError(handle));
    }
    g.pending.push_back(ZlibEvent::Close(handle));
    drop(g);
    notify_main_thread();
    true
}

fn stream_on(handle: i64, event: String, cb: i64, once: bool) {
    ensure_gc_scanner_registered();
    statics()
        .lock()
        .unwrap()
        .listeners
        .entry(handle)
        .or_default()
        .entry(event)
        .or_default()
        .push((cb, once));
    resume_stream(handle, false);
}

fn stream_off(handle: i64, event: &str, cb: i64) {
    if let Some(events) = statics().lock().unwrap().listeners.get_mut(&handle) {
        if let Some(list) = events.get_mut(event) {
            if let Some(at) = list.iter().rposition(|&(c, _)| c == cb) {
                list.remove(at);
            }
        }
    }
}

fn stream_pipe(handle: i64, dest_bits: u64) {
    if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
        s.pipes.push(dest_bits);
    }
    resume_stream(handle, false);
}

fn resume_stream(handle: i64, explicit: bool) {
    let mut g = statics().lock().unwrap();
    let consumer = g
        .listeners
        .get(&handle)
        .and_then(|m| m.get("data"))
        .is_some_and(|v| !v.is_empty());
    if let Some(s) = g.streams.get_mut(&handle) {
        if explicit {
            s.driver.paused = false;
        }
        if !s.driver.paused && (explicit || consumer || !s.pipes.is_empty()) {
            s.driver.flowing = true;
        }
    }
    schedule(&mut g, handle);
    drop(g);
    notify_main_thread();
}

fn pump_stream(handle: i64) {
    let mut g = statics().lock().unwrap();
    let Some(s) = g.streams.get_mut(&handle) else {
        return;
    };
    s.driver.scheduled = false;
    if s.destroyed {
        return;
    }
    let events = match s
        .driver
        .produce(&mut s.codec_state, handle, &mut s.bytes_written)
    {
        Ok(events) => events,
        Err(message) => {
            drop(g);
            destroy_stream(handle, Some(message));
            return;
        }
    };
    g.pending.extend(events);
    let s = g.streams.get_mut(&handle).unwrap();
    if s.driver.flowing && s.driver.pipe_waiters == 0 {
        if let Some(bytes) = s.driver.output.pop_front() {
            g.pending.push_back(ZlibEvent::Data(handle, bytes));
            // The next pump is scheduled AFTER data delivery so a listener's
            // pause/destroy applies before any further codec work.
            return;
        }
        if s.driver.done {
            g.pending.push_back(ZlibEvent::End(handle));
            return;
        }
    }
    if s.driver.can_progress() {
        schedule(&mut g, handle);
    }
}

/// Stream handle an event targets, if it is handle-scoped. `Callback` /
/// `OneShotCallback` carry only a closure, so they are not tied to a stream.
fn event_stream_handle(ev: &ZlibEvent) -> Option<i64> {
    match ev {
        ZlibEvent::Pump(id)
        | ZlibEvent::Drain(id)
        | ZlibEvent::Close(id)
        | ZlibEvent::Data(id, _)
        | ZlibEvent::Finish(id)
        | ZlibEvent::End(id)
        | ZlibEvent::DestroyError(id)
        | ZlibEvent::WriteError(id, _) => Some(*id),
        ZlibEvent::Callback(_) | ZlibEvent::OneShotCallback(_, _, _) => None,
    }
}

fn stream_async_id(handle: i64) -> u64 {
    statics()
        .lock()
        .ok()
        .and_then(|g| g.streams.get(&handle).map(|stream| stream.async_id))
        .unwrap_or(0)
}

// ── dynamic method dispatch for external zlib handles ─────────────────────────

/// True iff `handle` indexes a live zlib stream.
#[no_mangle]
pub extern "C" fn js_ext_zlib_is_stream_handle(handle: i64) -> i32 {
    if statics().lock().unwrap().streams.contains_key(&handle) {
        1
    } else {
        0
    }
}

/// Dispatch `.write`/`.end`/`.on`/`.once`/`.pipe`/`.flush`/`.close`/`.destroy`
/// on a zlib stream handle. Method name arrives as a UTF-8 ptr+len; args are
/// NaN-boxed f64s.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_zlib_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let method = if method_ptr.is_null() || method_len == 0 {
        return f64::from_bits(UNDEFINED);
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned()
    };
    let args: &[f64] = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    // The stream re-boxed as a POINTER_TAG handle (for `.on()` chaining).
    let self_ref = f64::from_bits(POINTER_TAG | (handle as u64 & POINTER_MASK));
    match method.as_str() {
        "write" if !args.is_empty() => {
            let cb = args
                .iter()
                .skip(1)
                .rev()
                .find(|v| js_zlib_is_callback(**v) != 0)
                .map(|v| (v.to_bits() & POINTER_MASK) as i64)
                .unwrap_or(0);
            let accepted =
                chunk_to_bytes(args[0]).is_some_and(|bytes| stream_write(handle, bytes, cb));
            f64::from_bits(if accepted {
                TRUE_BITS
            } else {
                JsValue::FALSE.bits()
            })
        }
        "end" => {
            let cb = args
                .iter()
                .rev()
                .find(|v| js_zlib_is_callback(**v) != 0)
                .map(|v| (v.to_bits() & POINTER_MASK) as i64)
                .unwrap_or(0);
            if cb != 0 {
                let mut g = statics().lock().unwrap();
                if let Some(s) = g.streams.get_mut(&handle) {
                    if s.finished {
                        g.pending.push_back(ZlibEvent::Callback(cb));
                    } else if s.destroyed {
                        g.pending.push_back(ZlibEvent::WriteError(handle, cb));
                    } else {
                        s.end_callbacks.push(cb);
                    }
                }
            }
            if let Some(chunk) = args
                .first()
                .copied()
                .filter(|v| js_zlib_is_callback(*v) == 0)
            {
                if let Some(bytes) = chunk_to_bytes(chunk) {
                    stream_write(handle, bytes, 0);
                }
            }
            finish_stream(handle);
            notify_main_thread();
            self_ref
        }
        "iterator" | "@@asyncIterator" => js_zlib_stream_iterator(
            self_ref,
            args.first().copied().unwrap_or(f64::from_bits(UNDEFINED)),
        ),
        "on" | "once" | "addListener" if args.len() >= 2 => {
            if let Some(ev) = event_name(args[0]) {
                let cb = (args[1].to_bits() & POINTER_MASK) as i64;
                stream_on(handle, ev, cb, method == "once");
            }
            self_ref
        }
        "listenerCount" if !args.is_empty() => {
            let count = event_name(args[0]).map_or(0, |event| {
                let g = statics().lock().unwrap();
                g.listeners
                    .get(&handle)
                    .and_then(|events| events.get(&event))
                    .map_or(0, Vec::len)
            });
            count as f64
        }
        // #11620: a `for await` that stops early detaches its listeners.
        "off" | "removeListener" if args.len() >= 2 => {
            if let Some(ev) = event_name(args[0]) {
                let cb = (args[1].to_bits() & POINTER_MASK) as i64;
                stream_off(handle, &ev, cb);
            }
            self_ref
        }
        "pipe" if !args.is_empty() => {
            stream_pipe(handle, args[0].to_bits());
            args[0] // Node's `.pipe(dest)` returns `dest` for chaining
        }
        "close" | "destroy" => {
            let destroyed = destroy_stream(handle, None);
            if let Some(reason) = args.first().copied().filter(|v| {
                !JsValue::from_bits(v.to_bits()).is_undefined()
                    && !JsValue::from_bits(v.to_bits()).is_null()
            }) {
                if destroyed {
                    let mut g = statics().lock().unwrap();
                    if let Some(s) = g.streams.get_mut(&handle) {
                        s.error = Some(reason);
                    }
                    let at = g
                        .pending
                        .iter()
                        .position(|e| matches!(e, ZlibEvent::Close(id) if *id == handle))
                        .unwrap_or(g.pending.len());
                    g.pending.insert(at, ZlibEvent::DestroyError(handle));
                }
            }
            self_ref
        }
        "_perryIteratorState" => {
            let g = statics().lock().unwrap();
            g.streams
                .get(&handle)
                .map(|s| {
                    if s.readable_ended {
                        1.0
                    } else if s.error.is_some() {
                        3.0
                    } else if s.destroyed {
                        2.0
                    } else {
                        0.0
                    }
                })
                .unwrap_or(2.0)
        }
        "_perryIteratorError" => statics()
            .lock()
            .unwrap()
            .streams
            .get(&handle)
            .and_then(|s| s.error)
            .unwrap_or(f64::from_bits(UNDEFINED)),
        "pause" => {
            if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
                s.driver.paused = true;
                s.driver.flowing = false;
            }
            self_ref
        }
        "resume" => {
            resume_stream(handle, true);
            self_ref
        }
        "_perryDrain" => {
            if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
                s.driver.pipe_waiters = s.driver.pipe_waiters.saturating_sub(1);
            }
            resume_stream(handle, false);
            self_ref
        }
        // `.flush([kind], cb?)` — Node's signature is `flush([kind], callback)`.
        // `kind` is numeric; the callback is the POINTER_TAG arg (if any).
        "flush" => {
            let cb = args
                .iter()
                .rev()
                .find(|a| (a.to_bits() >> 48) == 0x7FFD)
                .map(|a| (a.to_bits() & POINTER_MASK) as i64)
                .unwrap_or(0);
            stream_flush(handle, cb);
            f64::from_bits(UNDEFINED)
        }
        // `.params(level, strategy, cb)` — level/strategy are numeric, cb is the
        // trailing POINTER_TAG arg. Validation may throw synchronously.
        "params" => {
            let level = args.first().copied().unwrap_or(f64::from_bits(UNDEFINED));
            let strategy = args.get(1).copied().unwrap_or(f64::from_bits(UNDEFINED));
            let cb = args
                .iter()
                .rev()
                .find(|a| (a.to_bits() >> 48) == 0x7FFD)
                .map(|a| (a.to_bits() & POINTER_MASK) as i64)
                .unwrap_or(0);
            stream_params(handle, level, strategy, cb);
            self_ref
        }
        "reset" => {
            stream_reset(handle);
            f64::from_bits(UNDEFINED)
        }
        _ => f64::from_bits(UNDEFINED),
    }
}

#[no_mangle]
pub extern "C" fn js_ext_zlib_stream_bytes_written(handle: i64) -> f64 {
    stream_bytes_written(handle)
}

/// State lives on the handle record; no payload survives end/destroy.
#[no_mangle]
pub extern "C" fn js_ext_zlib_stream_property(handle: i64, which: i32) -> f64 {
    let g = statics().lock().unwrap();
    let Some(s) = g.streams.get(&handle) else {
        return f64::from_bits(UNDEFINED);
    };
    let boolean = |b| {
        f64::from_bits(if b {
            JsValue::TRUE.bits()
        } else {
            JsValue::FALSE.bits()
        })
    };
    match which {
        0 => s.driver.output_bytes as f64,
        1 => s.driver.readable_hwm as f64,
        2 => s.driver.input_bytes as f64,
        3 => s.driver.writable_hwm as f64,
        4 => boolean(s.destroyed),
        5 => boolean(s.readable_ended),
        6 => boolean(s.finished),
        _ => f64::from_bits(UNDEFINED),
    }
}

// ── pump (drained on the main thread from perry-stdlib) ─────────────────────────

fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    let mut g = statics().lock().unwrap();
    let Some(list) = g.listeners.get_mut(&id).and_then(|m| m.get_mut(event)) else {
        return Vec::new();
    };
    let callbacks = list.iter().map(|&(cb, _)| cb).collect();
    list.retain(|&(_, once)| !once);
    callbacks
}

fn pipes_for(id: i64) -> Vec<u64> {
    statics()
        .lock()
        .unwrap()
        .streams
        .get(&id)
        .map(|s| s.pipes.clone())
        .unwrap_or_default()
}

/// Forward a piped chunk: `dest.write(Buffer.from(bytes))`. Builds the method-
/// name string then the chunk Buffer back-to-back (the chunk comes from an
/// owned `Vec<u8>`), so dispatch roots the arg before any further allocation.
unsafe fn forward_write(handle: i64, dest_bits: u64, bytes: &[u8]) {
    let roots = TransientRootScope::enter();
    let dest = roots.root_nanbox(f64::from_bits(dest_bits));
    let name = roots.root_addr(alloc_string("write").as_raw() as i64);
    if name.get() == 0 {
        return;
    }
    let Some(buf) = make_buffer_f64(bytes) else {
        return;
    };
    let buf = roots.root_nanbox(buf);
    let args = [buf.get()];
    let written = js_native_call_method_str_key(dest.get(), name.get(), args.as_ptr(), 1);
    if written.to_bits() == JsValue::FALSE.bits() {
        if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
            s.driver.pipe_waiters += 1;
        }
        let cb = roots.root_addr(js_zlib_pipe_drain_callback(f64::from_bits(
            POINTER_TAG | handle as u64,
        )));
        let event = roots.root_nanbox(f64::from_bits(
            STRING_TAG | alloc_string("drain").as_raw() as u64,
        ));
        let name = alloc_string("once").as_raw();
        let args = [event.get(), f64::from_bits(POINTER_TAG | cb.get() as u64)];
        js_native_call_method_str_key(dest.get(), name as i64, args.as_ptr(), 2);
    }
}

unsafe fn forward_end(dest_bits: u64) {
    let roots = TransientRootScope::enter();
    let dest = roots.root_nanbox(f64::from_bits(dest_bits));
    let name = alloc_string("end").as_raw();
    if name.is_null() {
        return;
    }
    js_native_call_method_str_key(dest.get(), name as i64, std::ptr::null(), 0);
}

unsafe fn build_error_object(msg: &str) -> f64 {
    let truncated = msg.contains("unexpected end") || msg.contains("UnexpectedEof");
    js_zlib_stream_error(msg.as_ptr(), msg.len(), truncated as i32)
}

struct ZlibEventDispatch {
    event: Option<ZlibEvent>,
    callback: Option<TransientRootedAddr>,
}

unsafe extern "C" fn zlib_event_dispatch_thunk(data: *mut c_void) -> f64 {
    let call = &mut *(data as *mut ZlibEventDispatch);
    let event = call
        .event
        .take()
        .expect("zlib event dispatch thunk must run exactly once");
    match event {
        ZlibEvent::Data(id, bytes) => {
            {
                let mut g = statics().lock().unwrap();
                let Some(s) = g.streams.get_mut(&id) else {
                    return f64::from_bits(UNDEFINED);
                };
                if s.destroyed {
                    return f64::from_bits(UNDEFINED);
                }
                if !s.driver.flowing || s.driver.pipe_waiters != 0 {
                    s.driver.output.push_front(bytes);
                    return f64::from_bits(UNDEFINED);
                }
                s.driver.output_bytes -= bytes.len();
            }
            let roots = TransientRootScope::enter();
            let callbacks = roots.root_addrs(&listeners_for(id, "data"));
            let destinations = pipes_for(id)
                .into_iter()
                .map(|bits| roots.root_nanbox(f64::from_bits(bits)))
                .collect::<Vec<_>>();
            {
                if !callbacks.is_empty() {
                    if let Some(buffer) = make_buffer_f64(&bytes) {
                        let buffer = roots.root_nanbox(buffer);
                        for callback in callbacks {
                            if callback.get() != 0 {
                                let _ =
                                    JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                                        .call1(perry_ffi::JsThis::UNDEFINED, buffer.get());
                            }
                        }
                    }
                }
                for destination in destinations {
                    forward_write(id, destination.get().to_bits(), &bytes);
                }
            }
            let mut g = statics().lock().unwrap();
            if g.streams
                .get(&id)
                .is_some_and(|s| !s.destroyed && s.driver.can_progress())
            {
                schedule(&mut g, id);
            }
        }
        ZlibEvent::Pump(id) => pump_stream(id),
        ZlibEvent::Drain(id) => {
            let roots = TransientRootScope::enter();
            for callback in roots.root_addrs(&listeners_for(id, "drain")) {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call0(perry_ffi::JsThis::UNDEFINED);
                }
            }
        }
        ZlibEvent::Close(id) => {
            let roots = TransientRootScope::enter();
            let callbacks = roots.root_addrs(&listeners_for(id, "close"));
            let mut g = statics().lock().unwrap();
            // Keep the handle's terminal state, as Node keeps it on the
            // stream object. Its codec/input/output have already been freed.
            g.listeners.remove(&id);
            drop(g);
            for callback in callbacks {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call0(perry_ffi::JsThis::UNDEFINED);
                }
            }
        }
        ZlibEvent::Finish(id) => {
            if let Some(s) = statics().lock().unwrap().streams.get_mut(&id) {
                s.finished = true;
            }
            let roots = TransientRootScope::enter();
            let end_callbacks = statics()
                .lock()
                .unwrap()
                .streams
                .get_mut(&id)
                .map(|s| std::mem::take(&mut s.end_callbacks))
                .unwrap_or_default();
            let end_callbacks = roots.root_addrs(&end_callbacks);
            let callbacks = roots.root_addrs(&listeners_for(id, "finish"));
            for callback in end_callbacks.into_iter().chain(callbacks) {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call0(perry_ffi::JsThis::UNDEFINED);
                }
            }
        }
        ZlibEvent::End(id) => {
            let roots = TransientRootScope::enter();
            let end_callbacks = roots.root_addrs(&listeners_for(id, "end"));
            let destinations = pipes_for(id)
                .into_iter()
                .map(|bits| roots.root_nanbox(f64::from_bits(bits)))
                .collect::<Vec<_>>();
            let close_callbacks = roots.root_addrs(&listeners_for(id, "close"));
            {
                let mut g = statics().lock().unwrap();
                if let Some(s) = g.streams.get_mut(&id) {
                    s.destroyed = true;
                    s.readable_ended = true;
                    s.codec_state = None;
                    s.pipes.clear();
                    s.driver = Driver::new(
                        Codec::Gzip,
                        s.driver.chunk_size,
                        s.driver.readable_hwm,
                        s.driver.writable_hwm,
                    );
                }
                g.listeners.remove(&id);
            }
            for callback in end_callbacks {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call0(perry_ffi::JsThis::UNDEFINED);
                }
            }
            for destination in destinations {
                forward_end(destination.get().to_bits());
            }
            for callback in close_callbacks {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call0(perry_ffi::JsThis::UNDEFINED);
                }
            }
        }
        ZlibEvent::DestroyError(id) => {
            let roots = TransientRootScope::enter();
            let reason = statics()
                .lock()
                .unwrap()
                .streams
                .get(&id)
                .and_then(|s| s.error)
                .unwrap_or(f64::from_bits(UNDEFINED));
            let reason = roots.root_nanbox(reason);
            for callback in roots.root_addrs(&listeners_for(id, "error")) {
                if callback.get() != 0 {
                    let _ = JsClosure::from_raw(callback.get() as *const RawClosureHeader)
                        .call1(perry_ffi::JsThis::UNDEFINED, reason.get());
                }
            }
        }
        ZlibEvent::WriteError(id, cb) => {
            let roots = TransientRootScope::enter();
            let cb = roots.root_addr(call.callback.as_ref().map(|cb| cb.get()).unwrap_or(cb));
            let reason = statics()
                .lock()
                .unwrap()
                .streams
                .get(&id)
                .and_then(|s| s.error);
            let reason = reason.unwrap_or_else(|| {
                js_zlib_stream_error(
                    b"Cannot call write after a stream was destroyed".as_ptr(),
                    b"Cannot call write after a stream was destroyed".len(),
                    2,
                )
            });
            let reason = roots.root_nanbox(reason);
            if cb.get() != 0 {
                let _ = JsClosure::from_raw(cb.get() as *const RawClosureHeader)
                    .call1(perry_ffi::JsThis::UNDEFINED, reason.get());
            }
        }
        ZlibEvent::Callback(callback) => {
            let callback = call
                .callback
                .as_ref()
                .map(|cb| cb.get())
                .unwrap_or(callback);
            if callback != 0 {
                let _ = JsClosure::from_raw(callback as *const RawClosureHeader)
                    .call0(perry_ffi::JsThis::UNDEFINED);
            }
        }
        ZlibEvent::OneShotCallback(_, _, _) => {
            unreachable!("one-shot zlib events use the two-phase provider path")
        }
    }
    f64::from_bits(UNDEFINED)
}

unsafe extern "C" fn zlib_empty_phase_thunk(_data: *mut c_void) -> f64 {
    f64::from_bits(UNDEFINED)
}

struct ZlibOneShotDispatch {
    callback: TransientRootedAddr,
    result: Option<Result<Vec<u8>, String>>,
}

unsafe extern "C" fn zlib_one_shot_dispatch_thunk(data: *mut c_void) -> f64 {
    let call = &mut *(data as *mut ZlibOneShotDispatch);
    call_one_shot_callback(
        call.callback.get(),
        call.result
            .take()
            .expect("zlib one-shot dispatch thunk must run exactly once"),
    );
    f64::from_bits(UNDEFINED)
}

/// Drain the calling agent's queued zlib stream events: the main thread's from
/// its loop, a worker's from its own pump. Registered as an extension pump.
#[no_mangle]
pub unsafe extern "C" fn js_ext_zlib_process_pending() -> i32 {
    // Pop from the shared queue so reentrant pause/destroy takes effect before
    // the next event. Never hold the registry mutex across a JavaScript call.
    //
    // The loop is bounded to the queue length AT ENTRY so that callbacks which
    // repeatedly enqueue new work (e.g. write/flush in a tight loop) cannot
    // starve the event loop indefinitely. Newly added events are picked up on the
    // next pump invocation; `notify_main_thread()` ensures that call happens.
    let initial_count = statics().lock().unwrap().pending.len();
    let mut count = 0i32;
    for _ in 0..initial_count {
        let ev = {
            let mut g = statics().lock().unwrap();
            match g.pending.pop_front() {
                Some(ev) => ev,
                None => break,
            }
        };
        count += 1;
        let event_async_id = event_stream_handle(&ev).map(stream_async_id).unwrap_or(0);

        let ev = match ev {
            ZlibEvent::OneShotCallback(callback, result, async_id) => {
                let scope = TransientRootScope::enter();
                let callback = scope.root_addr(callback);
                // Node exposes the native codec completion and delivery of the
                // JavaScript callback as two phases of the same ZLIB resource.
                js_async_hooks_provider_run_catching_deferred_destroy_on_error(
                    async_id,
                    4,
                    zlib_empty_phase_thunk,
                    std::ptr::null_mut(),
                );
                let mut call = ZlibOneShotDispatch {
                    callback,
                    result: Some(result),
                };
                js_async_hooks_provider_run_catching_deferred_destroy(
                    async_id,
                    4,
                    zlib_one_shot_dispatch_thunk,
                    &mut call as *mut ZlibOneShotDispatch as *mut c_void,
                );
                continue;
            }
            event => event,
        };

        let terminal = matches!(&ev, ZlibEvent::End(_) | ZlibEvent::Close(_));
        let scope = TransientRootScope::enter();
        let callback = if let ZlibEvent::WriteError(_, cb) | ZlibEvent::Callback(cb) = &ev {
            Some(scope.root_addr(*cb))
        } else {
            None
        };
        let mut call = ZlibEventDispatch {
            event: Some(ev),
            callback,
        };
        if event_async_id == 0 {
            zlib_event_dispatch_thunk(&mut call as *mut ZlibEventDispatch as *mut c_void);
        } else if terminal {
            js_async_hooks_provider_run_catching_deferred_destroy(
                event_async_id,
                4,
                zlib_event_dispatch_thunk,
                &mut call as *mut ZlibEventDispatch as *mut c_void,
            );
        } else {
            js_async_hooks_provider_run_catching(
                event_async_id,
                zlib_event_dispatch_thunk,
                &mut call as *mut ZlibEventDispatch as *mut c_void,
            );
        }
    }
    // The bounded turn deliberately leaves newly scheduled work for the next
    // pump. Wake that turn even when no new JavaScript write arrives; otherwise
    // a subsequent stream can sleep with native output still queued.
    let pending = !statics().lock().unwrap().pending.is_empty();
    if pending {
        notify_main_thread();
    }
    count
}

/// Keep the event loop alive while zlib stream events are queued. Registered
/// directly with perry-runtime alongside the extension pump.
#[no_mangle]
pub extern "C" fn js_ext_zlib_has_active_handles() -> i32 {
    if statics().lock().unwrap().pending.is_empty() {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
