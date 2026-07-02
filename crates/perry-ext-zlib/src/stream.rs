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
//! This mirrors the perry-ext-net handle+event pattern, but zlib compression
//! is synchronous so there's no tokio task: input is buffered across
//! `.write()`, the codec runs once on `.end()`, and the resulting
//! 'data'/'end' events are *deferred* onto `ZLIB_PENDING` (drained by
//! `js_ext_zlib_process_pending` on the next loop tick) so listeners
//! registered after `.write()` still fire and `.pipe()` can forward chunks.

use perry_ffi::{
    alloc_buffer, alloc_string, gc_register_mutable_root_scanner_named, notify_main_thread,
    BufferHeader, ErrorKind, GcRootVisitor, JsClosure, JsValue, RawClosureHeader, StringHeader,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::sync::Mutex;

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
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
    fn js_register_aux_pump(f: extern "C" fn() -> i32);
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
    REGISTER.call_once(|| unsafe {
        js_register_aux_pump(process_pending_aux);
        js_register_aux_has_active(js_ext_zlib_has_active_handles);
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
        let len = (*buf).length as usize;
        let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
        return Some(std::slice::from_raw_parts(data, len).to_vec());
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

/// `zlib.brotliCompress(data, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress(data_value: f64, callback_value: f64) {
    queue_one_shot_callback(data_value, callback_value, "BrotliCompress", |b| {
        Ok(brotli_compress_bytes(b))
    });
}

/// `zlib.brotliDecompress(data, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress(data_value: f64, callback_value: f64) {
    queue_one_shot_callback(data_value, callback_value, "BrotliDecompress", |b| {
        brotli_decompress_bytes(b)
    });
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
}

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
        }
    }
}

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
        // Unzip auto-detects the header — kept buffer-until-end (run_codec).
        Codec::Unzip => return None,
    })
}

// ── registry ─────────────────────────────────────────────────────────────────

struct ZlibStreamState {
    codec: Codec,
    level: Compression,
    /// Streaming codec, fed incrementally. `None` for `createUnzip` (uses
    /// `input` + `run_codec` on `.end()`) or once finalized.
    codec_state: Option<CodecState>,
    /// Only used by `createUnzip` (buffer-until-end auto-detect).
    input: Vec<u8>,
    ended: bool,
    /// Set once any chunk has been fed. `.params()` can only rebuild the
    /// encoder at a new level (flate2 has no mid-stream `deflateParams`) before
    /// this flips; after data is written it validates + flushes only (#3285).
    wrote_data: bool,
    bytes_written: usize,
    pending_bytes_written: usize,
    /// `.pipe(dest)` destinations as NaN-boxed bits; 'data'/'end' forward here.
    pipes: Vec<u64>,
    /// Decompressed/compressed output produced BEFORE any consumer (`'data'`
    /// listener or pipe) attached, held until one does — Node's paused-Readable
    /// buffering. Without this, output drained to no listener was dropped and a
    /// consumer that attaches later (gaxios/node-fetch attach `on('data')` only
    /// after `await`ing the fetch) hung waiting on bytes that were already lost.
    output_buffer: Vec<u8>,
    /// `'end'` reached but deferred because no consumer had attached yet; the
    /// stream is kept alive (not removed) so a late consumer can drain
    /// `output_buffer` and then receive `'end'`.
    end_buffered: bool,
}

enum ZlibEvent {
    Data(i64, Vec<u8>),
    End(i64),
    Error(i64, String),
    /// `.flush(cb)` completion callback — invoked (0 args) after its flushed
    /// 'data' is delivered.
    Callback(i64),
    /// `zlib.gzip(data, cb)` style one-shot completion callback.
    OneShotCallback(i64, Result<Vec<u8>, String>),
}

struct Statics {
    streams: HashMap<i64, ZlibStreamState>,
    listeners: HashMap<i64, HashMap<String, Vec<i64>>>,
    pending: VecDeque<ZlibEvent>,
    next_id: i64,
    /// Running total of bytes held across every stream's `output_buffer` (i.e.
    /// output buffered for a consumer that has not attached yet). Maintained by
    /// the buffering path + `drop_buffered_stream` / `flush_buffered` so the
    /// global byte cap can be enforced without rescanning all streams.
    buffered_output_bytes: usize,
    /// Tombstone set for streams dropped by an overflow/eviction cap before a
    /// consumer attached. A late consumer attaching to one of these handles
    /// receives a terminal error event instead of silently hanging.
    evicted_streams: HashSet<i64>,
}

fn statics() -> &'static Mutex<Statics> {
    static S: std::sync::OnceLock<Mutex<Statics>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Statics {
            streams: HashMap::new(),
            listeners: HashMap::new(),
            pending: VecDeque::new(),
            next_id: 0x60000,
            buffered_output_bytes: 0,
            evicted_streams: HashSet::new(),
        })
    })
}

static ZLIB_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the GC root scanner once. Listener closures live only in the
/// `listeners` map; without rooting them a GC between `.on()` and the deferred
/// dispatch would free the closure (same hazard perry-ext-net guards).
fn ensure_gc_scanner_registered() {
    ZLIB_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-zlib", scan_zlib_roots);
    });
}

fn scan_zlib_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut s) = statics().lock() {
        for per_stream in s.listeners.values_mut() {
            for cb_vec in per_stream.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
        // Queued callbacks are referenced only here — root them too, same
        // hazard as listeners.
        for ev in s.pending.iter_mut() {
            match ev {
                ZlibEvent::Callback(cb) | ZlibEvent::OneShotCallback(cb, _) => {
                    visitor.visit_i64_slot(cb);
                }
                _ => {}
            }
        }
    }
}

fn create_stream(codec: Codec, level: Compression) -> i64 {
    ensure_gc_scanner_registered();
    let mut s = statics().lock().unwrap();
    let id = s.next_id;
    s.next_id += 1;
    s.streams.insert(
        id,
        ZlibStreamState {
            codec,
            level,
            codec_state: make_codec_state_with_level(codec, level),
            input: Vec::new(),
            ended: false,
            wrote_data: false,
            bytes_written: 0,
            pending_bytes_written: 0,
            pipes: Vec::new(),
            output_buffer: Vec::new(),
            end_buffered: false,
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
            if $min_wb != 0 {
                js_zlib_validate_options(opts, $min_wb);
            }
            let level = Compression::new(js_zlib_resolve_level(opts) as u32);
            create_stream($codec, level)
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
                let len = (*buf).length as usize;
                let data = (buf as *const u8).add(std::mem::size_of::<BufferHeader>());
                return Some(std::slice::from_raw_parts(data, len).to_vec());
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
    match result {
        Ok(bytes) => {
            let err = f64::from_bits(JsValue::NULL.bits());
            let out = make_buffer_f64(&bytes)
                .unwrap_or_else(|| f64::from_bits(JsValue::UNDEFINED.bits()));
            let _ = JsClosure::from_raw(callback as *const RawClosureHeader).call2(err, out);
        }
        Err(msg) => {
            let err = build_error_object(&msg);
            let _ = JsClosure::from_raw(callback as *const RawClosureHeader)
                .call2(err, f64::from_bits(JsValue::UNDEFINED.bits()));
        }
    }
}

pub(crate) unsafe fn queue_one_shot_callback<F>(
    data_value: f64,
    callback_value: f64,
    label: &'static str,
    op: F,
) where
    F: FnOnce(&[u8]) -> std::io::Result<Vec<u8>>,
{
    let callback = js_zlib_validate_callback(callback_value);
    let data_bits = data_value.to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    let result = match read_input_from_bits(data_bits) {
        Some(data) => op(&data).map_err(|e| format!("{} error: {}", label, e)),
        None => Err("Invalid input data".to_string()),
    };
    ensure_aux_pump_registered();
    ensure_gc_scanner_registered();
    statics()
        .lock()
        .unwrap()
        .pending
        .push_back(ZlibEvent::OneShotCallback(callback, result));
    notify_main_thread();
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

/// Feed a chunk to the streaming codec and queue any output that becomes
/// available immediately (incremental 'data'). For `createUnzip` (no streaming
/// codec) the chunk is buffered until `.end()`.
fn stream_write(handle: i64, bytes: &[u8]) {
    let mut g = statics().lock().unwrap();
    let event = match g.streams.get_mut(&handle) {
        Some(s) if !s.ended => {
            s.wrote_data = true;
            s.pending_bytes_written = s.pending_bytes_written.saturating_add(bytes.len());
            match s.codec_state.as_mut() {
                Some(cs) => match cs.write_chunk(bytes) {
                    Ok(()) => {
                        let out = cs.drain();
                        (!out.is_empty()).then_some(ZlibEvent::Data(handle, out))
                    }
                    Err(e) => Some(ZlibEvent::Error(handle, e.to_string())),
                },
                None => {
                    s.input.extend_from_slice(bytes);
                    None
                }
            }
        }
        _ => return,
    };
    if let Some(ev) = event {
        g.pending.push_back(ev);
        drop(g);
        notify_main_thread();
    }
}

/// `.flush([kind], cb?)` — emit a Z_SYNC_FLUSH (BROTLI_OPERATION_FLUSH) block so
/// a consumer can decode everything written so far, then queue the callback.
fn stream_flush(handle: i64, cb: i64) {
    let mut g = statics().lock().unwrap();
    let data = match g.streams.get_mut(&handle) {
        Some(s) if !s.ended => match s.codec_state.as_mut() {
            Some(cs) => {
                let _ = cs.flush_codec();
                cs.drain()
            }
            None => Vec::new(),
        },
        _ => Vec::new(),
    };
    if !data.is_empty() {
        g.pending.push_back(ZlibEvent::Data(handle, data));
    }
    if cb != 0 {
        g.pending.push_back(ZlibEvent::Callback(cb));
    }
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
            s.codec_state = make_codec_state_with_level(s.codec, level);
        } else if !s.ended {
            if let Some(cs) = s.codec_state.as_mut() {
                let _ = cs.flush_codec();
                let out = cs.drain();
                if !out.is_empty() {
                    g.pending.push_back(ZlibEvent::Data(handle, out));
                }
            }
        }
    }
    if cb != 0 {
        g.pending.push_back(ZlibEvent::Callback(cb));
    }
    drop(g);
    notify_main_thread();
}

fn stream_reset(handle: i64) {
    let mut g = statics().lock().unwrap();
    if let Some(s) = g.streams.get_mut(&handle) {
        s.codec_state = make_codec_state_with_level(s.codec, s.level);
        s.input.clear();
        s.ended = false;
        s.wrote_data = false;
        s.bytes_written = 0;
        s.pending_bytes_written = 0;
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

fn publish_bytes_written(handle: i64) {
    if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
        s.bytes_written = s.pending_bytes_written;
    }
}

/// Finalize the stream and queue the remaining output + 'end' (or 'error').
fn finish_stream(handle: i64) {
    let (codec_state, codec, input) = {
        let mut g = statics().lock().unwrap();
        match g.streams.get_mut(&handle) {
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
        let mut g = statics().lock().unwrap();
        match result {
            Ok(out) => {
                if !out.is_empty() {
                    g.pending.push_back(ZlibEvent::Data(handle, out));
                }
                g.pending.push_back(ZlibEvent::End(handle));
            }
            Err(msg) => g.pending.push_back(ZlibEvent::Error(handle, msg)),
        }
    }
    notify_main_thread();
}

fn stream_on(handle: i64, event: String, cb: i64) {
    ensure_gc_scanner_registered();
    statics()
        .lock()
        .unwrap()
        .listeners
        .entry(handle)
        .or_default()
        .entry(event)
        .or_default()
        .push(cb);
    flush_buffered(handle);
}

fn stream_pipe(handle: i64, dest_bits: u64) {
    if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
        s.pipes.push(dest_bits);
    }
    flush_buffered(handle);
}

/// Once a consumer (a `'data'` listener or a `.pipe(dest)`) attaches, re-queue
/// any output buffered before it arrived, followed by the deferred `'end'`, so a
/// late consumer still receives the full body. Re-queuing (rather than
/// delivering inline) is essential: `consumeBody` attaches `on('data')` then
/// `on('end')` synchronously, so the events must be delivered by a later pump
/// tick when BOTH listeners are present. No-op when there is no buffered output
/// / deferred end, or no data consumer yet.
fn flush_buffered(handle: i64) {
    let mut g = statics().lock().unwrap();
    let has_data_consumer = g
        .listeners
        .get(&handle)
        .and_then(|m| m.get("data"))
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || g.streams
            .get(&handle)
            .map(|s| !s.pipes.is_empty())
            .unwrap_or(false);
    if !has_data_consumer {
        return;
    }
    let Some(s) = g.streams.get_mut(&handle) else {
        // Stream is gone. If it was evicted by an overflow cap, deliver a
        // terminal error so the consumer does not hang indefinitely.
        if g.evicted_streams.remove(&handle) {
            g.pending.push_back(ZlibEvent::Error(
                handle,
                "zlib stream buffer overflow: output discarded before consumer attached"
                    .to_string(),
            ));
            drop(g);
            notify_main_thread();
        }
        return;
    };
    if s.output_buffer.is_empty() && !s.end_buffered {
        return;
    }
    let buf = std::mem::take(&mut s.output_buffer);
    let ended = s.end_buffered;
    s.end_buffered = false;
    g.buffered_output_bytes = g.buffered_output_bytes.saturating_sub(buf.len());
    // Buffered output predates anything still queued for this handle: it was
    // produced and drained by an earlier pump tick, before this consumer
    // attached. `process_pending` drains FIFO, so a plain `push` could deliver
    // these older bytes AFTER newer chunks already queued for the same stream
    // (e.g. a `.write()` landed between the buffering tick and the consumer
    // attaching). Splice ahead of the first pending event for this handle to
    // preserve per-stream order.
    insert_buffered_ahead(&mut g.pending, handle, buf, ended);
    drop(g);
    notify_main_thread();
}

/// Stream handle an event targets, if it is handle-scoped. `Callback` /
/// `OneShotCallback` carry only a closure, so they are not tied to a stream.
fn event_stream_handle(ev: &ZlibEvent) -> Option<i64> {
    match ev {
        ZlibEvent::Data(id, _) | ZlibEvent::End(id) | ZlibEvent::Error(id, _) => Some(*id),
        ZlibEvent::Callback(_) | ZlibEvent::OneShotCallback(_, _) => None,
    }
}

/// Splice late-flushed buffered `Data` (then the deferred `End`) ahead of any
/// newer queued events for `handle`, so the FIFO `process_pending` drain still
/// delivers a late consumer its chunks in write order. Inserts at the first
/// queued event for this handle, or the tail when none is queued (equivalent to
/// a push). `Callback`/`OneShotCallback` are not handle-scoped and are never
/// jumped.
fn insert_buffered_ahead(
    pending: &mut VecDeque<ZlibEvent>,
    handle: i64,
    buf: Vec<u8>,
    ended: bool,
) {
    let mut at = pending
        .iter()
        .position(|ev| event_stream_handle(ev) == Some(handle))
        .unwrap_or(pending.len());
    if !buf.is_empty() {
        pending.insert(at, ZlibEvent::Data(handle, buf));
        at += 1;
    }
    if ended {
        pending.insert(at, ZlibEvent::End(handle));
    }
}

/// Upper bound on never-consumed `end_buffered` streams kept alive for a late
/// consumer. A deferred-`End` stream is normally drained within a tick or two
/// (the consumer attaches right after `await`), so this only trips for a
/// genuinely abandoned handle — one that ends but never gets a `'data'` listener
/// or `.pipe()`. Without a cap those would pin their buffered output for the
/// process lifetime (the handle is a small int, not a GC-tracked object, so
/// nothing finalizes it). Mirrors perry-ext-net's bounded buffer pool.
const MAX_BUFFERED_ENDED_STREAMS: usize = 1024;

/// Fallback eviction for abandoned ended streams: once more than
/// [`MAX_BUFFERED_ENDED_STREAMS`] streams sit `end_buffered` without a consumer,
/// drop the oldest (smallest id ≈ earliest created) until back under the cap,
/// freeing their buffered output and listener entries. A still-wanted late
/// consumer keeps this count tiny, so a stream about to drain is not evicted in
/// practice.
fn evict_excess_buffered_ended(g: &mut Statics) {
    let buffered = g.streams.values().filter(|s| s.end_buffered).count();
    if buffered <= MAX_BUFFERED_ENDED_STREAMS {
        return;
    }
    let mut ids: Vec<i64> = g
        .streams
        .iter()
        .filter(|(_, s)| s.end_buffered)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    for id in ids.into_iter().take(buffered - MAX_BUFFERED_ENDED_STREAMS) {
        drop_buffered_stream(g, id);
    }
}

/// Remove a stream and its listeners, decrementing the buffered-output total by
/// whatever the stream still held. The single removal path so
/// `buffered_output_bytes` always equals the sum of every live `output_buffer`.
///
/// Leaves a tombstone in `evicted_streams` so that a late consumer attaching
/// after an overflow eviction receives a terminal error rather than hanging.
fn drop_buffered_stream(g: &mut Statics, id: i64) {
    if let Some(s) = g.streams.remove(&id) {
        g.buffered_output_bytes = g
            .buffered_output_bytes
            .saturating_sub(s.output_buffer.len());
        g.evicted_streams.insert(id);
    }
    g.listeners.remove(&id);
}

/// Per-stream cap on output buffered for a not-yet-attached consumer. Generous
/// enough for a realistic late consumer (gaxios/node-fetch awaiting a response
/// body) while still bounding a single never-consumed stream — e.g. a
/// decompression bomb fed incrementally with no listener. A no-consumer stream
/// that would exceed it is treated as abandoned and dropped (its buffer freed)
/// rather than grown without limit.
const MAX_BUFFERED_OUTPUT_PER_STREAM: usize = 64 * 1024 * 1024;

/// Global cap on output buffered across ALL not-yet-consumed streams. Bounds
/// total retained decompressed output even when many streams each stay under the
/// per-stream cap; the oldest no-consumer buffers are evicted first.
const MAX_BUFFERED_OUTPUT_TOTAL: usize = 256 * 1024 * 1024;

/// Buffer decompressed output for a stream whose consumer has not attached yet,
/// enforcing the production byte caps. See [`buffer_output_capped`].
fn buffer_output_for_late_consumer(g: &mut Statics, id: i64, bytes: &[u8]) {
    buffer_output_capped(
        g,
        id,
        bytes,
        MAX_BUFFERED_OUTPUT_PER_STREAM,
        MAX_BUFFERED_OUTPUT_TOTAL,
    );
}

/// Append `bytes` to a no-consumer stream's buffer under explicit caps (the
/// caps are parameters so tests can exercise the policy without allocating
/// hundreds of MiB). Policy: a stream that would exceed `per_stream_cap` without
/// a consumer is dropped as abandoned (a never-consumed stream or a hostile
/// decompression bomb) instead of growing unbounded; otherwise the bytes are
/// appended and, if the global total then exceeds `total_cap`, the oldest
/// no-consumer buffers are evicted. A stream that already has a consumer never
/// reaches here — its output is delivered, not buffered — so this never
/// penalizes a consumed stream.
fn buffer_output_capped(
    g: &mut Statics,
    id: i64,
    bytes: &[u8],
    per_stream_cap: usize,
    total_cap: usize,
) {
    let cur = match g.streams.get(&id) {
        Some(s) => s.output_buffer.len(),
        None => return,
    };
    if cur.saturating_add(bytes.len()) > per_stream_cap {
        drop_buffered_stream(g, id);
        return;
    }
    if let Some(s) = g.streams.get_mut(&id) {
        s.output_buffer.extend_from_slice(bytes);
    }
    g.buffered_output_bytes = g.buffered_output_bytes.saturating_add(bytes.len());
    enforce_global_output_cap(g, total_cap);
}

/// Evict the oldest never-consumed buffers (smallest id ≈ earliest created)
/// until the retained total is back under `total_cap`. Only streams that still
/// hold buffered output are candidates; a consumed stream has already drained
/// its buffer and contributes nothing.
fn enforce_global_output_cap(g: &mut Statics, total_cap: usize) {
    if g.buffered_output_bytes <= total_cap {
        return;
    }
    let mut ids: Vec<i64> = g
        .streams
        .iter()
        .filter(|(_, s)| !s.output_buffer.is_empty())
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    for id in ids {
        if g.buffered_output_bytes <= total_cap {
            break;
        }
        drop_buffered_stream(g, id);
    }
}

// ── dispatch (called from perry-stdlib's external-zlib-pump arm) ───────────────

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
            if let Some(bytes) = chunk_to_bytes(args[0]) {
                stream_write(handle, &bytes);
            }
            f64::from_bits(TRUE_BITS) // Node's writable.write() returns a boolean
        }
        "end" => {
            if let Some(chunk) = args.first().copied() {
                if let Some(bytes) = chunk_to_bytes(chunk) {
                    stream_write(handle, &bytes);
                }
            }
            finish_stream(handle);
            self_ref
        }
        "on" | "once" | "addListener" if args.len() >= 2 => {
            if let Some(ev) = event_name(args[0]) {
                let cb = (args[1].to_bits() & POINTER_MASK) as i64;
                stream_on(handle, ev, cb);
            }
            self_ref
        }
        "pipe" if !args.is_empty() => {
            stream_pipe(handle, args[0].to_bits());
            args[0] // Node's `.pipe(dest)` returns `dest` for chaining
        }
        "close" | "destroy" => {
            finish_stream(handle);
            f64::from_bits(UNDEFINED)
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

// ── pump (drained on the main thread from perry-stdlib) ─────────────────────────

fn listeners_for(id: i64, event: &str) -> Vec<i64> {
    statics()
        .lock()
        .unwrap()
        .listeners
        .get(&id)
        .and_then(|m| m.get(event).cloned())
        .unwrap_or_default()
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
unsafe fn forward_write(dest_bits: u64, bytes: &[u8]) {
    let name = alloc_string("write").as_raw();
    if name.is_null() {
        return;
    }
    let buf = match make_buffer_f64(bytes) {
        Some(b) => b,
        None => return,
    };
    let args = [buf];
    js_native_call_method_str_key(f64::from_bits(dest_bits), name as i64, args.as_ptr(), 1);
}

unsafe fn forward_end(dest_bits: u64) {
    let name = alloc_string("end").as_raw();
    if name.is_null() {
        return;
    }
    js_native_call_method_str_key(f64::from_bits(dest_bits), name as i64, std::ptr::null(), 0);
}

/// `{ message: msg }` error object so `s.on('error', e => e.message)` works.
unsafe fn build_error_object(msg: &str) -> f64 {
    let (packed, shape) = perry_ffi::build_object_shape(&["message"]);
    let obj = perry_ffi::js_object_alloc_with_shape(shape, 1, packed.as_ptr(), packed.len() as u32);
    let s = alloc_string(msg).as_raw();
    if obj.is_null() {
        return f64::from_bits(STRING_TAG | (s as u64 & POINTER_MASK));
    }
    perry_ffi::js_object_set_field(obj, 0, JsValue::from_string_ptr(s));
    f64::from_bits(POINTER_TAG | (obj as u64 & POINTER_MASK))
}

/// Drain queued zlib stream events on the main thread. Wired into perry-stdlib's
/// `js_stdlib_process_pending` via the external-zlib-pump feature.
#[no_mangle]
pub unsafe extern "C" fn js_ext_zlib_process_pending() -> i32 {
    // Drain ONE event at a time from the SHARED queue (not a detached snapshot).
    // A JS callback fired while processing an event can attach a late consumer,
    // whose `flush_buffered` splices the older buffered bytes back into this same
    // queue; popping from the front means that splice lands AHEAD of a newer
    // same-handle event still waiting in the drain, so FIFO order is preserved. A
    // snapshot drain (`mem::take` into a local vec) would strand the buffered
    // bytes on the next tick, behind newer data delivered now. The lock is held
    // only to pop — never across a callback.
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
        match ev {
            ZlibEvent::Data(id, bytes) => {
                publish_bytes_written(id);
                let cbs = listeners_for(id, "data");
                let dests = pipes_for(id);
                if cbs.is_empty() && dests.is_empty() {
                    // No consumer attached yet — buffer instead of dropping, so a
                    // `.on('data')`/`.pipe()` that attaches later (after `await`)
                    // still receives the body (flushed by `flush_buffered`),
                    // bounded by the per-stream + global byte caps.
                    buffer_output_for_late_consumer(&mut statics().lock().unwrap(), id, &bytes);
                } else {
                    if !cbs.is_empty() {
                        if let Some(buf_f64) = make_buffer_f64(&bytes) {
                            for cb in cbs {
                                if cb != 0 {
                                    let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                                        .call1(buf_f64);
                                }
                            }
                        }
                    }
                    for dest in dests {
                        forward_write(dest, &bytes);
                    }
                }
            }
            ZlibEvent::End(id) => {
                publish_bytes_written(id);
                // Defer `'end'` (keep the stream + its buffer alive) when no
                // consumer has attached yet — otherwise removing the stream here
                // would strand a `.on('data')`/`.on('end')` that attaches later
                // (gaxios attaches them only after `await`ing the fetch), hanging
                // the body-consume. `flush_buffered` re-queues End once a
                // consumer attaches and the buffer has drained.
                let has_consumer =
                    !listeners_for(id, "data").is_empty() || !pipes_for(id).is_empty();
                if !has_consumer {
                    let mut g = statics().lock().unwrap();
                    let deferred = match g.streams.get_mut(&id) {
                        Some(s) => {
                            s.end_buffered = true;
                            true
                        }
                        None => false,
                    };
                    if deferred {
                        // Cap how many never-consumed ended streams we retain so
                        // an abandoned handle (one that never gets a `'data'`
                        // listener or pipe) can't pin its buffered output for the
                        // process lifetime; drop the oldest excess.
                        evict_excess_buffered_ended(&mut g);
                        continue;
                    }
                    // Stream already gone — release the lock and fall through to
                    // the (no-op) delivery + removal below.
                    drop(g);
                }
                for cb in listeners_for(id, "end") {
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                for cb in listeners_for(id, "finish") {
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                for dest in pipes_for(id) {
                    forward_end(dest);
                }
                for cb in listeners_for(id, "close") {
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop_buffered_stream(&mut statics().lock().unwrap(), id);
            }
            ZlibEvent::Callback(cb) => {
                if cb != 0 {
                    let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                }
            }
            ZlibEvent::OneShotCallback(cb, result) => {
                call_one_shot_callback(cb, result);
            }
            ZlibEvent::Error(id, msg) => {
                let err_f64 = build_error_object(&msg);
                for cb in listeners_for(id, "error") {
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(err_f64);
                    }
                }
                drop_buffered_stream(&mut statics().lock().unwrap(), id);
            }
        }
    }
    count
}

/// Keep the event loop alive while zlib stream events are queued. Wired into
/// perry-stdlib's `js_stdlib_has_active_handles`.
#[no_mangle]
pub extern "C" fn js_ext_zlib_has_active_handles() -> i32 {
    if statics().lock().unwrap().pending.is_empty() {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    /// Drive the streaming codec like the FFI ops do: write each chunk +
    /// drain, flush + drain between chunks, then finish — and reassemble the
    /// full compressed stream.
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
        assert_eq!(&c[..2], &[0x1f, 0x8b]); // gzip magic
        assert_eq!(
            run_codec(Codec::Gunzip, &c).unwrap(),
            b"hello streaming world"
        );
    }

    #[test]
    fn gunzip_run_codec_reads_all_members() {
        let a = stream_compress(Codec::Gzip, &[b"first "]);
        let b = stream_compress(Codec::Gzip, &[b"second "]);
        let c = stream_compress(Codec::Gzip, &[b"third"]);
        let mut concatenated = Vec::new();
        concatenated.extend_from_slice(&a);
        concatenated.extend_from_slice(&b);
        concatenated.extend_from_slice(&c);
        assert_eq!(
            run_codec(Codec::Gunzip, &concatenated).unwrap(),
            b"first second third"
        );
    }

    #[test]
    fn deflate_stream_is_zlib_format_and_roundtrips() {
        let c = stream_compress(Codec::Deflate, &[b"AAAA", b"BBBB"]);
        assert_eq!(c[0], 0x78); // zlib header (NOT raw deflate)
        assert_eq!(run_codec(Codec::Inflate, &c).unwrap(), b"AAAABBBB");
    }

    #[test]
    fn deflate_raw_stream_roundtrips() {
        let c = stream_compress(Codec::DeflateRaw, &[b"raw ", b"deflate"]);
        assert_eq!(run_codec(Codec::InflateRaw, &c).unwrap(), b"raw deflate");
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
    fn brotli_decompress_rejects_invalid_data() {
        assert!(brotli_decompress_bytes(b"not a brotli stream").is_err());
    }

    // ── late-flush ordering (insert-ahead) ───────────────────────────────────

    fn data_bytes(pending: &VecDeque<ZlibEvent>) -> Vec<(i64, Vec<u8>)> {
        pending
            .iter()
            .filter_map(|ev| match ev {
                ZlibEvent::Data(id, b) => Some((*id, b.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn buffered_output_is_spliced_ahead_of_newer_queued_chunk() {
        // A newer chunk for this handle is already queued (a `.write()` landed
        // between the buffering tick and the consumer attaching, so the stream
        // is NOT ended yet); the late flush of the OLDER buffered bytes must
        // still be delivered first under the FIFO drain.
        let handle = 0x60000;
        let mut pending: VecDeque<ZlibEvent> =
            VecDeque::from(vec![ZlibEvent::Data(handle, b"newer".to_vec())]);
        insert_buffered_ahead(&mut pending, handle, b"older".to_vec(), false);

        assert_eq!(
            data_bytes(&pending),
            vec![(handle, b"older".to_vec()), (handle, b"newer".to_vec())],
            "older buffered bytes must precede the newer queued chunk"
        );
        // No End yet — the stream has not ended.
        assert!(!pending.iter().any(|ev| matches!(ev, ZlibEvent::End(_))));
    }

    #[test]
    fn deferred_end_trails_buffered_data() {
        // The realistic end_buffered case: the stream ended with no consumer,
        // so all output is buffered and there is no newer queued chunk. The
        // flush emits the buffered Data immediately followed by End.
        let handle = 0x60002;
        let mut pending: VecDeque<ZlibEvent> = VecDeque::new();
        insert_buffered_ahead(&mut pending, handle, b"body".to_vec(), true);
        assert!(matches!(&pending[0], ZlibEvent::Data(h, b) if *h == handle && b == b"body"));
        assert!(matches!(pending.back(), Some(ZlibEvent::End(h)) if *h == handle));
    }

    #[test]
    fn buffered_output_appends_when_queue_has_no_event_for_handle() {
        let handle = 0x60001;
        let mut pending: VecDeque<ZlibEvent> = VecDeque::new();
        insert_buffered_ahead(&mut pending, handle, b"body".to_vec(), true);
        assert!(matches!(&pending[0], ZlibEvent::Data(h, b) if *h == handle && b == b"body"));
        assert!(matches!(&pending[1], ZlibEvent::End(h) if *h == handle));
    }

    #[test]
    fn insert_ahead_does_not_jump_other_handles() {
        // An event for a DIFFERENT handle queued first must not be reordered —
        // buffered output is spliced only ahead of ITS OWN handle's events.
        let mine = 0x60010;
        let other = 0x60011;
        let mut pending: VecDeque<ZlibEvent> = VecDeque::from(vec![
            ZlibEvent::Data(other, b"other".to_vec()),
            ZlibEvent::Data(mine, b"newer".to_vec()),
        ]);
        insert_buffered_ahead(&mut pending, mine, b"older".to_vec(), false);
        assert_eq!(
            data_bytes(&pending),
            vec![
                (other, b"other".to_vec()),
                (mine, b"older".to_vec()),
                (mine, b"newer".to_vec()),
            ]
        );
    }

    #[test]
    fn reentrant_flush_during_drain_delivers_older_bytes_first() {
        // Models the one-at-a-time drain: events are popped from the SHARED queue
        // (not a detached snapshot). When processing the first event triggers a
        // late consumer to attach (a reentrant `flush_buffered`), its older
        // buffered bytes are spliced into the SAME queue ahead of the newer chunk
        // still waiting in the drain — so the FIFO pop delivers them first. A
        // snapshot drain would strand the older bytes on the next tick, behind the
        // newer chunk delivered now (this assert would then fail).
        let h = 0x60000;
        let mut pending: VecDeque<ZlibEvent> = VecDeque::from(vec![
            // Stand-in for "an event whose handler attaches a consumer for `h`".
            ZlibEvent::Callback(0),
            // A newer chunk for `h` already queued ahead in this same drain.
            ZlibEvent::Data(h, b"newer".to_vec()),
        ]);
        let mut delivered: Vec<Vec<u8>> = Vec::new();
        while let Some(ev) = pending.pop_front() {
            match ev {
                ZlibEvent::Callback(_) => {
                    // Reentrant flush of `h`'s older buffered bytes mid-drain.
                    insert_buffered_ahead(&mut pending, h, b"older".to_vec(), false);
                }
                ZlibEvent::Data(_, b) => delivered.push(b),
                _ => {}
            }
        }
        assert_eq!(delivered, vec![b"older".to_vec(), b"newer".to_vec()]);
    }

    // ── abandoned-stream eviction cap + byte caps ────────────────────────────

    fn empty_statics() -> Statics {
        Statics {
            streams: HashMap::new(),
            listeners: HashMap::new(),
            pending: VecDeque::new(),
            next_id: 0x60000,
            buffered_output_bytes: 0,
            evicted_streams: HashSet::new(),
        }
    }

    fn no_consumer_state() -> ZlibStreamState {
        ZlibStreamState {
            codec: Codec::Gzip,
            level: Compression::default(),
            codec_state: None,
            input: Vec::new(),
            ended: false,
            wrote_data: true,
            bytes_written: 0,
            pending_bytes_written: 0,
            pipes: Vec::new(),
            output_buffer: Vec::new(),
            end_buffered: false,
        }
    }

    fn ended_buffered_state() -> ZlibStreamState {
        ZlibStreamState {
            codec: Codec::Gzip,
            level: Compression::default(),
            codec_state: None,
            input: Vec::new(),
            ended: true,
            wrote_data: true,
            bytes_written: 0,
            pending_bytes_written: 0,
            pipes: Vec::new(),
            output_buffer: b"buffered".to_vec(),
            end_buffered: true,
        }
    }

    #[test]
    fn evicts_oldest_excess_abandoned_ended_streams() {
        let mut g = empty_statics();
        let extra = 5;
        for i in 0..(MAX_BUFFERED_ENDED_STREAMS + extra) as i64 {
            g.streams.insert(0x60000 + i, ended_buffered_state());
            g.listeners.insert(0x60000 + i, HashMap::new());
        }

        evict_excess_buffered_ended(&mut g);

        assert_eq!(
            g.streams.values().filter(|s| s.end_buffered).count(),
            MAX_BUFFERED_ENDED_STREAMS,
            "buffered-ended streams must be capped"
        );
        // The oldest `extra` handles (smallest ids) are the ones dropped.
        for i in 0..extra as i64 {
            assert!(!g.streams.contains_key(&(0x60000 + i)));
            assert!(!g.listeners.contains_key(&(0x60000 + i)));
        }
        assert!(g.streams.contains_key(&(0x60000 + extra as i64)));
    }

    #[test]
    fn eviction_is_a_noop_under_the_cap() {
        let mut g = empty_statics();
        for i in 0..8i64 {
            g.streams.insert(0x60000 + i, ended_buffered_state());
        }
        evict_excess_buffered_ended(&mut g);
        assert_eq!(g.streams.len(), 8, "nothing evicted while under the cap");
    }

    #[test]
    fn buffering_accumulates_and_tracks_total_bytes() {
        let mut g = empty_statics();
        g.streams.insert(0x60000, no_consumer_state());
        buffer_output_capped(&mut g, 0x60000, b"hello", 1024, 1024);
        buffer_output_capped(&mut g, 0x60000, b"world", 1024, 1024);
        assert_eq!(g.streams[&0x60000].output_buffer, b"helloworld");
        assert_eq!(g.buffered_output_bytes, 10);
    }

    #[test]
    fn per_stream_byte_cap_drops_overlarge_abandoned_stream() {
        let mut g = empty_statics();
        g.streams.insert(0x60000, no_consumer_state());
        g.listeners.insert(0x60000, HashMap::new());
        // Under the per-stream cap: accepted and accounted.
        buffer_output_capped(&mut g, 0x60000, b"1234", 8, 1024);
        assert_eq!(g.buffered_output_bytes, 4);
        // Crossing the per-stream cap with no consumer: the stream is dropped and
        // its buffer freed rather than grown unbounded.
        buffer_output_capped(&mut g, 0x60000, b"56789", 8, 1024);
        assert!(!g.streams.contains_key(&0x60000));
        assert!(!g.listeners.contains_key(&0x60000));
        assert_eq!(g.buffered_output_bytes, 0);
    }

    #[test]
    fn global_byte_cap_evicts_oldest_buffers() {
        let mut g = empty_statics();
        // Per-stream cap large (never trips); global cap 10 bytes.
        for i in 0..4i64 {
            let id = 0x60000 + i;
            g.streams.insert(id, no_consumer_state());
            buffer_output_capped(&mut g, id, b"abcd", 1024, 10);
        }
        assert!(
            g.buffered_output_bytes <= 10,
            "total stays under the global cap"
        );
        // Oldest (smallest ids) evicted first; newest retained.
        assert!(!g.streams.contains_key(&0x60000));
        assert!(!g.streams.contains_key(&0x60001));
        assert!(g.streams.contains_key(&0x60003));
        // The running total still equals the sum of the remaining buffers.
        let sum: usize = g.streams.values().map(|s| s.output_buffer.len()).sum();
        assert_eq!(g.buffered_output_bytes, sum);
    }

    // ── eviction tombstone ───────────────────────────────────────────────────

    #[test]
    fn drop_buffered_stream_leaves_tombstone() {
        let mut g = empty_statics();
        g.streams.insert(0x60000, no_consumer_state());
        g.listeners.insert(0x60000, HashMap::new());

        drop_buffered_stream(&mut g, 0x60000);

        assert!(!g.streams.contains_key(&0x60000), "stream removed");
        assert!(
            g.evicted_streams.contains(&0x60000),
            "tombstone set so a late consumer gets an error instead of hanging"
        );
    }

    #[test]
    fn tombstone_consumed_and_error_queued_when_late_consumer_attaches() {
        // Simulate the flush_buffered tombstone path: stream absent, tombstone
        // present, and a data consumer has just attached.
        let mut g = empty_statics();
        g.streams.insert(0x60000, no_consumer_state());
        drop_buffered_stream(&mut g, 0x60000);

        // Attach a listener (simulates stream_on registering a 'data' cb).
        g.listeners
            .entry(0x60000)
            .or_default()
            .entry("data".to_string())
            .or_default()
            .push(1);

        // The tombstone check logic (inline from flush_buffered).
        let stream_exists = g.streams.contains_key(&0x60000);
        assert!(!stream_exists);
        assert!(g.evicted_streams.contains(&0x60000));
        if g.evicted_streams.remove(&0x60000) {
            g.pending.push_back(ZlibEvent::Error(
                0x60000,
                "zlib stream buffer overflow: output discarded before consumer attached"
                    .to_string(),
            ));
        }

        assert!(
            !g.evicted_streams.contains(&0x60000),
            "tombstone consumed on first late attachment"
        );
        assert!(
            matches!(g.pending.front(), Some(ZlibEvent::Error(id, _)) if *id == 0x60000),
            "error queued for the late consumer"
        );
    }

    // ── bounded drain ────────────────────────────────────────────────────────

    #[test]
    fn drain_bounded_to_initial_count_leaves_new_work_for_next_tick() {
        // Model the `for _ in 0..initial_count` loop: events enqueued by a
        // callback during the drain are NOT processed in the same pump call.
        let mut pending: VecDeque<ZlibEvent> = VecDeque::from(vec![
            ZlibEvent::Callback(0), // the only event present at loop entry
        ]);
        let mut processed = 0usize;
        let initial_count = pending.len(); // 1
        for _ in 0..initial_count {
            let Some(ev) = pending.pop_front() else {
                break;
            };
            processed += 1;
            if let ZlibEvent::Callback(_) = ev {
                // A callback that enqueues two more events mid-drain.
                pending.push_back(ZlibEvent::Callback(0));
                pending.push_back(ZlibEvent::Callback(0));
            }
        }
        assert_eq!(processed, 1, "only the initial batch is drained");
        assert_eq!(
            pending.len(),
            2,
            "newly enqueued events are deferred to the next pump tick"
        );
    }
}
