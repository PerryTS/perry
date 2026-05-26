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
    alloc_buffer, alloc_bytes, alloc_string, gc_register_mutable_root_scanner_named,
    notify_main_thread, BufferHeader, GcRootVisitor, JsClosure, JsValue, RawClosureHeader,
    StringHeader,
};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;

use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder,
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
    fn js_buffer_is_buffer(ptr: i64) -> i32;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_native_call_method_str_key(
        object: f64,
        name_handle: i64,
        args_ptr: *const f64,
        args_len: usize,
    ) -> f64;
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

/// `zlib.brotliCompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress_sync(
    data_ptr: *const StringHeader,
) -> *mut StringHeader {
    match read_input_bytes(data_ptr) {
        Some(d) => alloc_bytes(&brotli_compress_bytes(&d)).as_raw(),
        None => std::ptr::null_mut(),
    }
}

/// `zlib.brotliDecompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_sync(
    data_ptr: *const StringHeader,
) -> *mut StringHeader {
    match read_input_bytes(data_ptr).map(|d| brotli_decompress_bytes(&d)) {
        Some(Ok(out)) => alloc_bytes(&out).as_raw(),
        _ => std::ptr::null_mut(),
    }
}

/// `zlib.brotliCompress(data)` -> Promise<Buffer>.
///
/// # Safety
/// `data_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress(
    data_ptr: *const StringHeader,
) -> *mut perry_ffi::Promise {
    brotli_async(data_ptr, "BrotliCompress", |b| Ok(brotli_compress_bytes(b)))
}

/// `zlib.brotliDecompress(data)` -> Promise<Buffer>.
///
/// # Safety
/// `data_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress(
    data_ptr: *const StringHeader,
) -> *mut perry_ffi::Promise {
    brotli_async(data_ptr, "BrotliDecompress", |b| brotli_decompress_bytes(b))
}

unsafe fn brotli_async<F>(
    data_ptr: *const StringHeader,
    label: &'static str,
    op: F,
) -> *mut perry_ffi::Promise
where
    F: FnOnce(&[u8]) -> std::io::Result<Vec<u8>> + Send + 'static,
{
    let promise = perry_ffi::JsPromise::new();
    let raw = promise.as_raw();
    let Some(data) = read_input_bytes(data_ptr) else {
        promise.reject_string("Invalid input data");
        return raw;
    };
    perry_ffi::spawn_blocking(move || match op(&data) {
        Ok(out) => promise.resolve(JsValue::from_object_ptr(alloc_bytes(&out).as_raw())),
        Err(e) => promise.reject_string(&format!("{} error: {}", label, e)),
    });
    raw
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
        Codec::BrotliCompress => out = brotli_compress_bytes(input),
        Codec::BrotliDecompress => out = brotli_decompress_bytes(input)?,
    }
    Ok(out)
}

// ── registry ─────────────────────────────────────────────────────────────────

struct ZlibStreamState {
    codec: Codec,
    input: Vec<u8>,
    ended: bool,
    /// `.pipe(dest)` destinations as NaN-boxed bits; 'data'/'end' forward here.
    pipes: Vec<u64>,
}

enum ZlibEvent {
    Data(i64, Vec<u8>),
    End(i64),
    Error(i64, String),
}

struct Statics {
    streams: HashMap<i64, ZlibStreamState>,
    listeners: HashMap<i64, HashMap<String, Vec<i64>>>,
    pending: Vec<ZlibEvent>,
    next_id: i64,
}

fn statics() -> &'static Mutex<Statics> {
    static S: std::sync::OnceLock<Mutex<Statics>> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(Statics {
            streams: HashMap::new(),
            listeners: HashMap::new(),
            pending: Vec::new(),
            next_id: 0x60000,
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
    }
}

fn create_stream(codec: Codec) -> i64 {
    ensure_gc_scanner_registered();
    let mut s = statics().lock().unwrap();
    let id = s.next_id;
    s.next_id += 1;
    s.streams.insert(
        id,
        ZlibStreamState {
            codec,
            input: Vec::new(),
            ended: false,
            pipes: Vec::new(),
        },
    );
    id
}

// ── factories ────────────────────────────────────────────────────────────────

macro_rules! factory {
    ($name:ident, $codec:expr) => {
        /// # Safety
        /// FFI entry; `_opts` is the (ignored) NaN-boxed options object.
        #[no_mangle]
        pub unsafe extern "C" fn $name(_opts: f64) -> i64 {
            create_stream($codec)
        }
    };
}
factory!(js_zlib_create_gzip, Codec::Gzip);
factory!(js_zlib_create_gunzip, Codec::Gunzip);
factory!(js_zlib_create_deflate, Codec::Deflate);
factory!(js_zlib_create_inflate, Codec::Inflate);
factory!(js_zlib_create_deflate_raw, Codec::DeflateRaw);
factory!(js_zlib_create_inflate_raw, Codec::InflateRaw);
factory!(js_zlib_create_unzip, Codec::Unzip);
factory!(js_zlib_create_brotli_compress, Codec::BrotliCompress);
factory!(js_zlib_create_brotli_decompress, Codec::BrotliDecompress);

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

fn stream_write(handle: i64, bytes: &[u8]) {
    if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
        if !s.ended {
            s.input.extend_from_slice(bytes);
        }
    }
}

/// Run the codec on the buffered input and queue 'data'/'end' (or 'error').
fn finish_stream(handle: i64) {
    let (codec, input) = {
        let mut g = statics().lock().unwrap();
        match g.streams.get_mut(&handle) {
            Some(s) if !s.ended => {
                s.ended = true;
                (s.codec, std::mem::take(&mut s.input))
            }
            _ => return,
        }
    };
    {
        let mut g = statics().lock().unwrap();
        match run_codec(codec, &input) {
            Ok(out) => {
                if !out.is_empty() {
                    g.pending.push(ZlibEvent::Data(handle, out));
                }
                g.pending.push(ZlibEvent::End(handle));
            }
            Err(e) => g.pending.push(ZlibEvent::Error(handle, format!("{}", e))),
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
}

fn stream_pipe(handle: i64, dest_bits: u64) {
    if let Some(s) = statics().lock().unwrap().streams.get_mut(&handle) {
        s.pipes.push(dest_bits);
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
        // `.flush([cb])` — buffer-until-end model has nothing to flush mid-
        // stream (output emits on `.end()`); accept as a no-op so callers
        // don't hit "flush is not a function".
        "flush" => f64::from_bits(UNDEFINED),
        _ => f64::from_bits(UNDEFINED),
    }
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
    let events: Vec<ZlibEvent> = std::mem::take(&mut statics().lock().unwrap().pending);
    let count = events.len() as i32;
    for ev in events {
        match ev {
            ZlibEvent::Data(id, bytes) => {
                let cbs = listeners_for(id, "data");
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
                for dest in pipes_for(id) {
                    forward_write(dest, &bytes);
                }
            }
            ZlibEvent::End(id) => {
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
                let mut g = statics().lock().unwrap();
                g.listeners.remove(&id);
                g.streams.remove(&id);
            }
            ZlibEvent::Error(id, msg) => {
                let err_f64 = build_error_object(&msg);
                for cb in listeners_for(id, "error") {
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(err_f64);
                    }
                }
                let mut g = statics().lock().unwrap();
                g.listeners.remove(&id);
                g.streams.remove(&id);
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
