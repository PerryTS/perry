//! node:zlib: ordinary runtime Transforms owning only codecs in payload cells.

use flate2::Compression;
use perry_ffi::{
    alloc_string, bytes, native_payload as np, native_stream as ns, ErrorKind, JsThis, JsValue,
    RawClosureHeader, TransientRootScope,
};
use std::ffi::c_void;
use std::io::Read;
const UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
extern "C" {
    fn js_nm_install_zlib();
    fn js_object_set_property_key(owner: f64, key: f64, value: f64) -> f64;
    fn js_zlib_stream_error(message: *const u8, len: usize, truncated: i32) -> f64;
    fn js_zlib_is_callback(value: f64) -> i32;
    fn js_zlib_stream_option(opts: f64, which: i32) -> usize;
    fn js_zlib_decode_completion_options(opts: f64) -> i32;
    fn js_zlib_brotli_max_output_length(opts: f64) -> usize;
    fn js_zlib_gzip_codec_options(opts: f64) -> f64;
    fn js_zlib_gzip_base_options(opts: f64) -> f64;
    fn js_zlib_gzip_transform_options(opts: f64, phase: i32) -> f64;
    fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut c_void;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    pub(crate) fn js_zlib_resolve_level(opts: f64) -> i32;
    pub(crate) fn js_zlib_validate_params(level: f64, strategy: f64) -> i32;
    pub(crate) fn js_zlib_validate_options(opts: f64, min_window_bits: i32);
    fn js_zlib_sync_codec_options(opts: f64, min_window_bits: i32) -> f64;
    fn js_zlib_sync_finish_options(opts: f64, codec: f64) -> f64;
    fn js_zlib_deflate_options(opts: f64, min_window_bits: i32) -> f64;
    fn js_zlib_zstd_dictionary(opts: f64) -> f64;
    pub(crate) fn js_zlib_validate_buffer_arg(data_bits: i64);
    pub(crate) fn js_zlib_validate_callback(callback: f64) -> i64;
}
mod allocation;
mod deflate_context;
mod driver;
mod inflate_context;
pub(crate) use deflate_context::encode_bytes as deflate_dictionary_bytes;
pub(crate) use driver::{decode_bytes, decode_options_bytes, is_trailing_garbage};
pub(crate) use inflate_context::needs_dictionary;
mod one_shot_callback;
mod zlib_encoder;
mod zsys;
pub(crate) use one_shot_callback::queue_one_shot_callback;

/// Runs a test body as its own agent, the way a worker runs (#11417). Unclaimed
/// libtest threads all resolve to the primary agent, so concurrent tests share
/// one timer store: one test's check phase ran another test's immediates, from
/// another thread's arena. Retiring on drop purges what the test left queued.
#[cfg(test)]
pub(crate) struct OwnAgent(perry_runtime::agent::AgentId);
#[cfg(test)]
impl OwnAgent {
    pub(crate) fn enter() -> Self {
        Self(perry_runtime::agent::enter_worker_agent())
    }
}
#[cfg(test)]
impl Drop for OwnAgent {
    fn drop(&mut self) {
        perry_runtime::agent::retire_agent(self.0);
    }
}

pub(crate) unsafe fn read_input_from_bits(bits: i64) -> Option<Vec<u8>> {
    let value = JsValue::from_bits(bits as u64);
    if let Some(bytes) = bytes::no_gc(|scope| bytes::borrow(value, scope).map(|b| b.to_vec())) {
        return Some(bytes);
    }
    let ptr = js_get_string_pointer_unified(f64::from_bits(bits as u64))
        as *const perry_ffi::StringHeader;
    if ptr.is_null() {
        None
    } else {
        perry_ffi::read_bytes(perry_ffi::JsString::from_raw(ptr as *mut _)).map(|s| s.to_vec())
    }
}
/// The runtime validates public brands; the byte ABI preserves view bounds.
/// Copy before another getter, allocation, callback or detach can run.
pub(crate) unsafe fn dictionary_options(opts: f64, min_window_bits: i32) -> Option<Vec<u8>> {
    let roots = TransientRootScope::enter();
    let dict = roots.root_nanbox(js_zlib_deflate_options(opts, min_window_bits));
    copy_dictionary(dict.get())
}

unsafe fn copy_dictionary(dictionary: f64) -> Option<Vec<u8>> {
    let roots = TransientRootScope::enter();
    let dict = roots.root_nanbox(dictionary);
    if dict.get().to_bits() == UNDEFINED {
        return None;
    }
    let copy = bytes::no_gc(|scope| {
        bytes::borrow(JsValue::from_bits(dict.get().to_bits()), scope).map(|bytes| bytes.to_vec())
    });
    Some(copy.unwrap_or_else(|| {
        perry_ffi::throw_with_code(
            "Cannot read dictionary bytes",
            "ERR_INVALID_ARG_TYPE",
            ErrorKind::TypeError,
        )
    }))
}
/// Native-only snapshot: no JS values or input borrows survive policy getters.
#[derive(Default)]
pub(crate) struct DecodePolicy {
    reject_garbage: bool,
    allow_incomplete: bool,
}
pub(crate) unsafe fn completion_options(opts: f64) -> DecodePolicy {
    let flags = js_zlib_decode_completion_options(opts);
    DecodePolicy {
        reject_garbage: flags & 1 != 0,
        allow_incomplete: flags & 2 != 0,
    }
}
/// Capture user getters once per constructor phase; later consumers see only
/// data properties. Copy dictionary bytes before the Transform spread can run.
pub(crate) unsafe fn sync_options(opts: f64, min_window_bits: i32) -> (f64, Option<Vec<u8>>) {
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(opts);
    let codec = roots.root_nanbox(js_zlib_sync_codec_options(opts.get(), min_window_bits));
    let dictionary = dictionary_options(codec.get(), min_window_bits);
    (
        js_zlib_sync_finish_options(opts.get(), codec.get()),
        dictionary,
    )
}
pub(crate) fn sync_info(opts: f64) -> bool {
    field(opts, "_syncInfo").to_bits() == JsValue::TRUE.bits()
}

/// The returned engine is the sole owner of the codec that produces buffer.
/// No codec/native reference crosses progress, field allocation or error throws.
pub(crate) unsafe fn sync_info_result(
    data: Vec<u8>,
    opts: f64,
    codec: Codec,
    dictionary: Option<Vec<u8>>,
) -> f64 {
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(opts);
    let family = if matches!(codec, Codec::Deflate) {
        &DEFLATE
    } else {
        &INFLATE
    };
    // Use an isolated captured dictionary value: the original view may already
    // have been mutated by a later getter. The real owner uses these exact bytes.
    let dictionary = dictionary.map(|bytes| roots.root_nanbox(value_bytes(&bytes)));
    if let Some(dict) = &dictionary {
        np::own(opts.get(), "dictionary", dict.get());
    }
    let owner = roots.root_nanbox(create_stream(codec, family, opts.get(), None));
    let info = roots.root_nanbox(field(opts.get(), "info"));
    np::own(owner.get(), "_info", info.get());
    let mut consumed = 0;
    let mut output = Vec::new();
    let error = loop {
        let (status, failure, native_bytes) = {
            let payload = payload(owner.get()).expect("sync codec owner");
            let mut out = ns::StepOut {
                consumed: 0,
                out: std::ptr::null(),
                out_len: 0,
                status: ns::StepStatus::NEED_INPUT,
                code: 0,
                external_bytes: 0,
            };
            payload.step(
                &ns::StepIn {
                    op: ns::StreamOp::FINAL,
                    flush_kind: 0,
                    input: data.as_ptr().add(consumed),
                    len: data.len() - consumed,
                },
                &mut out,
            );
            consumed += out.consumed.min(data.len() - consumed);
            if out.out_len != 0 {
                output.extend_from_slice(std::slice::from_raw_parts(out.out, out.out_len));
            }
            let failure = if out.status == ns::StepStatus::ERROR {
                Some(payload.error_details(out.code))
            } else {
                None
            };
            (out.status, failure, payload.external_bytes())
        };
        np::set_external_bytes(owner.get(), &CODEC_VTABLE, native_bytes);
        if failure.is_some() || status != ns::StepStatus::MORE {
            break failure;
        }
    };
    drop(data);
    progress(owner.get());
    release(owner.get());
    if let Some((message, code, _)) = error {
        drop(output);
        perry_ffi::throw_with_code(&message, &code, ErrorKind::Error);
    }
    let buffer = roots.root_nanbox(value_bytes(&output));
    drop(output);
    let result = roots.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    np::own(result.get(), "buffer", buffer.get());
    np::own(result.get(), "engine", owner.get());
    result.get()
}

pub(crate) unsafe fn compression_from_opts(opts: f64) -> Compression {
    Compression::new(js_zlib_resolve_level(opts) as u32)
}
pub(crate) fn value_bytes(bytes: &[u8]) -> f64 {
    f64::from_bits(bytes::from_slice(bytes::Brand::Buffer, bytes).bits())
}

fn brotli_compress_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut r = brotli::CompressorReader::new(data, 4096, 11, 22);
    let _ = r.read_to_end(&mut out);
    out
}

#[cfg(test)]
fn brotli_decompress_bytes(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    brotli::Decompressor::new(data, 4096).read_to_end(&mut out)?;
    Ok(out)
}

enum BrotliDecodeError {
    Decode,
    OutputLimit,
}

fn brotli_decompress_limited(data: &[u8], limit: usize) -> Result<Vec<u8>, BrotliDecodeError> {
    let mut decoder = brotli::Decompressor::new(data, 4096);
    let mut output = Vec::new();
    let mut scratch = [0; 4096];
    loop {
        let remaining = limit.saturating_sub(output.len());
        // One bounded lookahead byte distinguishes exact-limit EOF from overflow.
        let available = remaining.saturating_add(1).min(scratch.len());
        let count = decoder
            .read(&mut scratch[..available])
            .map_err(|_| BrotliDecodeError::Decode)?;
        if count > remaining {
            return Err(BrotliDecodeError::OutputLimit);
        }
        if count == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&scratch[..count]);
    }
}

fn throw_brotli_decode_error() -> ! {
    perry_ffi::throw_with_code(
        "Decompression failed",
        "ERR__ERROR_FORMAT_PADDING_2",
        ErrorKind::Error,
    )
}

/// `zlib.brotliCompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_bits` must be the raw NaN-box bit pattern of the data argument.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress_sync(data_bits: i64) -> f64 {
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits) {
        Some(d) => value_bytes(&brotli_compress_bytes(&d)),
        None => f64::from_bits(UNDEFINED),
    }
}

/// `zlib.brotliDecompressSync(data, options?)` -> Buffer.
///
/// # Safety
/// `data_bits` and `options` must be raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_sync(data_bits: i64, options: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let data = roots.root_nanbox(f64::from_bits(data_bits as u64));
    let options = roots.root_nanbox(options);
    let limit = js_zlib_brotli_max_output_length(options.get());
    js_zlib_validate_buffer_arg(data.get().to_bits() as i64);
    match read_input_from_bits(data.get().to_bits() as i64)
        .map(|d| brotli_decompress_limited(&d, limit))
    {
        Some(Ok(out)) => value_bytes(&out),
        Some(Err(BrotliDecodeError::OutputLimit)) => perry_ffi::throw_with_code(
            &format!("Cannot create a Buffer larger than {limit} bytes"),
            "ERR_BUFFER_TOO_LARGE",
            ErrorKind::RangeError,
        ),
        Some(Err(BrotliDecodeError::Decode)) => throw_brotli_decode_error(),
        _ => f64::from_bits(UNDEFINED),
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
    queue_one_shot_callback(data_value, options, callback_value, Codec::BrotliCompress);
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
    queue_one_shot_callback(data_value, options, callback_value, Codec::BrotliDecompress);
}

fn throw_zstd_error(err: &std::io::Error) -> ! {
    let code = driver::zstd_error_details(err)
        .map(|(code, _)| code)
        .unwrap_or_else(|| "Z_DATA_ERROR".into());
    perry_ffi::throw_with_code(&format!("zstd: {}", err), &code, ErrorKind::Error)
}

/// The same bulk operations as zstd::bulk::Compressor, retaining typed errors.
fn zstd_compress_bytes(data: &[u8], dictionary: Option<&[u8]>) -> std::io::Result<Vec<u8>> {
    let mut context = zstd::zstd_safe::CCtx::create();
    context
        .set_parameter(zstd::zstd_safe::CParameter::CompressionLevel(
            ZSTD_DEFAULT_LEVEL,
        ))
        .map_err(driver::zstd_error)?;
    context
        .load_dictionary(dictionary.unwrap_or(&[]))
        .map_err(driver::zstd_error)?;
    let mut output = Vec::with_capacity(zstd::zstd_safe::compress_bound(data.len()));
    context
        .compress2(&mut output, data)
        .map_err(driver::zstd_error)?;
    Ok(output)
}

/// `zlib.zstdCompressSync(data, options?)` -> Buffer.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress_sync(data_value: f64, opts: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let data = roots.root_nanbox(data_value);
    js_zlib_validate_buffer_arg(data.get().to_bits() as i64);
    let dictionary = copy_dictionary(js_zlib_zstd_dictionary(opts));
    let data_bits = data.get().to_bits() as i64;
    match read_input_from_bits(data_bits).map(|d| zstd_compress_bytes(&d, dictionary.as_deref())) {
        Some(Ok(out)) => value_bytes(&out),
        Some(Err(e)) => throw_zstd_error(&e),
        None => f64::from_bits(UNDEFINED),
    }
}

/// `zlib.zstdDecompressSync(data, options?)` -> Buffer.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress_sync(data_value: f64, opts: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let data = roots.root_nanbox(data_value);
    js_zlib_validate_buffer_arg(data.get().to_bits() as i64);
    let dictionary = copy_dictionary(js_zlib_zstd_dictionary(opts));
    let data_bits = data.get().to_bits() as i64;
    match read_input_from_bits(data_bits).map(|d| {
        driver::decode_with_dictionary(&d, Codec::ZstdDecompress, dictionary).map_err(|err| {
            // Preserve decode_all's existing incomplete-input message.
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "incomplete frame")
            } else {
                err
            }
        })
    }) {
        Some(Ok(out)) => value_bytes(&out),
        Some(Err(e)) => throw_zstd_error(&e),
        None => f64::from_bits(UNDEFINED),
    }
}

/// `zlib.zstdCompress(data, options?, callback)` -> undefined.
///
/// # Safety
/// `data_value` and `callback_value` are raw NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress(data_value: f64, options: f64, callback_value: f64) {
    queue_one_shot_callback(data_value, options, callback_value, Codec::ZstdCompress);
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
    queue_one_shot_callback(data_value, options, callback_value, Codec::ZstdDecompress);
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum Codec {
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

unsafe extern "C" fn step(payload: *mut c_void, input: &ns::StepIn, output: &mut ns::StepOut) {
    (&mut *(payload as *mut driver::Payload)).step(input, output);
    #[cfg(test)]
    if output.status == ns::StepStatus::ERROR && sabotage("error_as_eof") {
        output.status = ns::StepStatus::ENDED;
    }
}
unsafe fn payload(owner: f64) -> Option<&'static mut driver::Payload> {
    np::get_attached(owner, &CODEC_VTABLE)
}
unsafe extern "C" fn progress(owner: f64) {
    let written = payload(owner).map(|p| p.bytes_written());
    if let Some(written) = written {
        np::own(owner, "bytesWritten", written as f64);
    }
}
unsafe fn error_field(owner: f64, key: &str, value: f64) {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(owner);
    let value = roots.root_nanbox(value);
    let key = roots.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(key).as_raw()).bits(),
    ));
    js_object_set_property_key(owner.get(), key.get(), value.get());
}
unsafe extern "C" fn error(owner: f64, code: u32) -> f64 {
    let (message, name, errno) = payload(owner)
        .map(|p| p.error_details(code))
        .unwrap_or_else(|| ("Decompression failed".into(), "Z_DATA_ERROR".into(), -3));
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(owner);
    let err = roots.root_nanbox(js_zlib_stream_error(
        message.as_ptr(),
        message.len(),
        if code == 5 { 1 } else { 0 },
    ));
    let name = roots.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(&name).as_raw()).bits(),
    ));
    error_field(err.get(), "code", name.get());
    error_field(err.get(), "errno", errno as f64);
    // The native binding's onerror destroys the stream without calling the
    // outstanding _transform continuation (Node 26.5.1's pending write).
    method(owner.get(), "destroy", &[err.get()]);
    err.get()
}
unsafe extern "C" fn release(owner: f64) {
    #[cfg(test)]
    if sabotage("release_in_finalizer_only") {
        return;
    }
    // Skip only the close-time drop: the stream looks released, and the
    // payload and its buffers wait for the sweep's drop.
    #[cfg(test)]
    if sabotage("buffers_on_drop_only") {
        np::own(owner, "_handle", f64::from_bits(JsValue::NULL.bits()));
        return;
    }
    np::close_attached(owner, &CODEC_VTABLE);
    #[cfg(test)]
    if sabotage("keep_handle_field") {
        return;
    }
    np::own(owner, "_handle", f64::from_bits(JsValue::NULL.bits()));
}
static HOOKS: ns::StreamHooks = ns::StreamHooks {
    kind: ns::StreamKind::TRANSFORM,
    timing: ns::StepTiming::DEFERRED,
    lazy: false,
    step,
    error,
    release,
    after_step: Some(progress),
};
static CODEC_VTABLE: ns::PayloadVTable = ns::payload_vtable::<driver::Payload>(Some(&HOOKS));

static GZIP: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::GZIP,
    "Gzip",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_gzip(opts: f64) -> f64 {
    create_stream(Codec::Gzip, &GZIP, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gzip_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::Gzip, &GZIP, opts, Some(owner))
}

static GUNZIP: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::GUNZIP,
    "Gunzip",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_gunzip(opts: f64) -> f64 {
    create_stream(Codec::Gunzip, &GUNZIP, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_gunzip_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::Gunzip, &GUNZIP, opts, Some(owner))
}

static DEFLATE: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::DEFLATE,
    "Deflate",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_deflate(opts: f64) -> f64 {
    create_stream(Codec::Deflate, &DEFLATE, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::Deflate, &DEFLATE, opts, Some(owner))
}

static INFLATE: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::INFLATE,
    "Inflate",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_inflate(opts: f64) -> f64 {
    create_stream(Codec::Inflate, &INFLATE, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::Inflate, &INFLATE, opts, Some(owner))
}

static DEFLATE_RAW: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::DEFLATE_RAW,
    "DeflateRaw",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_deflate_raw(opts: f64) -> f64 {
    create_stream(Codec::DeflateRaw, &DEFLATE_RAW, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_deflate_raw_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::DeflateRaw, &DEFLATE_RAW, opts, Some(owner))
}

static INFLATE_RAW: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::INFLATE_RAW,
    "InflateRaw",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_inflate_raw(opts: f64) -> f64 {
    create_stream(Codec::InflateRaw, &INFLATE_RAW, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_inflate_raw_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::InflateRaw, &INFLATE_RAW, opts, Some(owner))
}

static UNZIP: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::UNZIP,
    "Unzip",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_unzip(opts: f64) -> f64 {
    create_stream(Codec::Unzip, &UNZIP, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_unzip_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::Unzip, &UNZIP, opts, Some(owner))
}

static BROTLI_COMPRESS: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::BROTLI_COMPRESS,
    "BrotliCompress",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_brotli_compress(opts: f64) -> f64 {
    create_stream(Codec::BrotliCompress, &BROTLI_COMPRESS, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_compress_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::BrotliCompress, &BROTLI_COMPRESS, opts, Some(owner))
}

static BROTLI_DECOMPRESS: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::BROTLI_DECOMPRESS,
    "BrotliDecompress",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_brotli_decompress(opts: f64) -> f64 {
    create_stream(Codec::BrotliDecompress, &BROTLI_DECOMPRESS, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_init(owner: f64, opts: f64) -> f64 {
    create_stream(
        Codec::BrotliDecompress,
        &BROTLI_DECOMPRESS,
        opts,
        Some(owner),
    )
}

static ZSTD_COMPRESS: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::ZSTD_COMPRESS,
    "ZstdCompress",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_zstd_compress(opts: f64) -> f64 {
    create_stream(Codec::ZstdCompress, &ZSTD_COMPRESS, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::ZstdCompress, &ZSTD_COMPRESS, opts, Some(owner))
}

static ZSTD_DECOMPRESS: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::ZSTD_DECOMPRESS,
    "ZstdDecompress",
    false,
    &CODEC_VTABLE,
);
#[no_mangle]
pub unsafe extern "C" fn js_zlib_create_zstd_decompress(opts: f64) -> f64 {
    create_stream(Codec::ZstdDecompress, &ZSTD_DECOMPRESS, opts, None)
}
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress_init(owner: f64, opts: f64) -> f64 {
    create_stream(Codec::ZstdDecompress, &ZSTD_DECOMPRESS, opts, Some(owner))
}

static BASE: np::PayloadFamily = np::PayloadFamily::new::<driver::Payload>(
    perry_ffi::native_class_ids::ZLIB_BASE,
    "#ZlibBase",
    false,
    &CODEC_VTABLE,
)
.with_installer(install_base);
unsafe extern "C" fn install_base(proto: *mut c_void) {
    np::prototype_method(
        proto,
        "reset",
        perry_ffi::js_function_info!(reset,0;with_declared(0),with_flags(perry_ffi::FN_BUILTIN)),
        0,
    );
    np::prototype_method(
        proto,
        "flush",
        perry_ffi::js_function_info!(flush,2;with_declared(2),with_flags(perry_ffi::FN_BUILTIN)),
        2,
    );
    np::prototype_method(
        proto,
        "close",
        perry_ffi::js_function_info!(close,1;with_declared(1),with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    np::prototype_method(
        proto,
        "params",
        perry_ffi::js_function_info!(params,3;with_declared(3),with_flags(perry_ffi::FN_BUILTIN)),
        3,
    );
    np::prototype_method(
        proto,
        "_transform",
        perry_ffi::js_function_info!(transform,3;with_declared(3),with_flags(perry_ffi::FN_BUILTIN)),
        3,
    );
    np::prototype_method(
        proto,
        "_flush",
        perry_ffi::js_function_info!(final_flush,1;with_declared(1),with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    np::prototype_method(
        proto,
        "_final",
        perry_ffi::js_function_info!(final_callback,1;with_declared(1),with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    np::prototype_method(
        proto,
        "_destroy",
        perry_ffi::js_function_info!(destroy_callback,2;with_declared(2),with_flags(perry_ffi::FN_BUILTIN)),
        2,
    );
}
fn field(owner: f64, key: &str) -> f64 {
    f64::from_bits(perry_ffi::object_field_by_name(JsValue::from_bits(owner.to_bits()), key).bits())
}
unsafe fn method(owner: f64, name: &str, args: &[f64]) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(owner);
    let args: Vec<_> = args.iter().map(|a| roots.root_nanbox(*a)).collect();
    let method = roots.root_nanbox(field(owner.get(), name));
    let args: Vec<_> = args.iter().map(|a| a.get()).collect();
    perry_ffi::call_value(method.get(), JsThis::from_f64(owner.get()), &args)
}
/// Provider imports install constructor metadata before module initialization,
/// including prototype reads that precede the first factory call.
#[no_mangle]
pub unsafe extern "C" fn js_ext_zlib_nm_install() {
    js_nm_install_zlib();
}

unsafe fn create_stream(
    codec: Codec,
    family: &'static np::PayloadFamily,
    opts: f64,
    existing: Option<f64>,
) -> f64 {
    // Direct codegen calls can bypass namespace imports. Install the existing
    // metadata hooks before materializing the constructor's prototype chain.
    #[cfg(test)]
    let install = !sabotage("skip_registry_bootstrap");
    #[cfg(not(test))]
    let install = true;
    if install {
        js_nm_install_zlib();
    }
    let roots = TransientRootScope::enter();
    let opts = roots.root_nanbox(opts);
    let existing = existing.map(|o| roots.root_nanbox(o));
    // Only Gzip uses the pinned constructor phases. Shared sync/callback and
    // other codec consumers retain their existing validation/read schedule.
    let mut dictionary = None;
    let gzip_options = if matches!(codec, Codec::Gzip) {
        Some(roots.root_nanbox(js_zlib_gzip_codec_options(opts.get())))
    } else {
        // Zero selects the wrapped decoder default, not a lower nonzero bound.
        let min_window_bits = match codec {
            Codec::Inflate | Codec::Gunzip | Codec::Unzip => 0,
            _ => 8,
        };
        if matches!(
            codec,
            Codec::Deflate | Codec::Inflate | Codec::ZstdCompress | Codec::ZstdDecompress
        ) {
            dictionary = dictionary_options(opts.get(), min_window_bits);
        } else {
            js_zlib_validate_options(opts.get(), min_window_bits);
        }
        None
    };
    let level = if let Some(captured) = &gzip_options {
        let level = field(captured.get(), "level");
        Compression::new(if level < 0.0 { 6 } else { level as u32 })
    } else if matches!(codec, Codec::Deflate | Codec::DeflateRaw) {
        compression_from_opts(opts.get())
    } else {
        Compression::default()
    };
    // Own the Gzip engine before any base/Transform getter can throw. The
    // scratch sizing seam below never replaces or reconstructs that engine.
    let chunk_size = if gzip_options.is_some() {
        16384
    } else {
        js_zlib_stream_option(opts.get(), 0)
    };
    let _base = roots.root_nanbox(np::prototype(&BASE, "zlib"));
    let proto = roots.root_nanbox(np::prototype(family, "zlib"));
    let zstd = matches!(codec, Codec::ZstdCompress | Codec::ZstdDecompress);
    let codec_payload = match dictionary {
        Some(dictionary) => {
            driver::Payload::with_dictionary(codec, level, chunk_size, Some(dictionary))
        }
        None => driver::Payload::new(codec, level, chunk_size),
    };
    let codec_payload = if zstd {
        codec_payload.unwrap_or_else(|err| throw_zstd_error(&err))
    } else {
        codec_payload.unwrap()
    };
    let native_bytes = codec_payload.external_bytes();
    let owner = if let Some(owner) = existing {
        assert!(np::attach(owner.get(), family, codec_payload, native_bytes));
        owner
    } else {
        roots.root_nanbox(np::alloc(family, codec_payload, proto.get(), native_bytes))
    };
    macro_rules! own_new {
        ($key:literal,$value:expr) => {{
            let value = roots.root_nanbox($value);
            np::own(owner.get(), $key, value.get());
        }};
    }
    // Node's pre-fields precede the Transform constructor; zstd puts its
    // write state last. The runtime owns all actual stream state.
    if !zstd {
        own_new!(
            "_writeState",
            f64::from_bits(JsValue::from_object_ptr(js_typed_array_new_empty(5, 2)).bits())
        );
    }
    let gzip_base = gzip_options
        .as_ref()
        .map(|_| roots.root_nanbox(js_zlib_gzip_base_options(opts.get())));
    let chunk_size = if let Some(base) = &gzip_base {
        let chunk_size = field(base.get(), "chunkSize") as usize;
        let native_bytes = {
            let payload = payload(owner.get()).expect("owned Gzip codec");
            payload.resize_constructor_scratch(chunk_size).unwrap();
            payload.external_bytes()
        };
        np::set_external_bytes(owner.get(), &CODEC_VTABLE, native_bytes);
        chunk_size
    } else {
        chunk_size
    };
    let transform_source = gzip_options
        .as_ref()
        .map(|_| roots.root_nanbox(js_zlib_gzip_transform_options(opts.get(), 0)));
    let transform_options = transform_source
        .as_ref()
        .map(|source| roots.root_nanbox(js_zlib_gzip_transform_options(source.get(), 1)));
    own_new!(
        "_events",
        f64::from_bits(perry_ffi::alloc_null_proto_object(&[]).bits())
    );
    assert!(ns::init_transform_in_place(
        owner.get(),
        transform_options
            .as_ref()
            .map_or_else(|| opts.get(), |copy| copy.get())
    ));
    np::own(owner.get(), "_maxListeners", f64::from_bits(UNDEFINED));
    np::own(owner.get(), "_eventsCount", 0.0);
    let prefinish = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(prefinish, 0),
            0,
        ))
        .bits(),
    ));
    let event = roots.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string("prefinish").as_raw()).bits(),
    ));
    method(owner.get(), "on", &[event.get(), prefinish.get()]);
    np::own(owner.get(), "bytesWritten", 0.0);
    let handle = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::runtime_layouts::alloc_runtime_record(
            std::ptr::addr_of!(perry_ffi::runtime_layouts::RECORD_PCHECKED031),
        ))
        .bits(),
    ));
    let handle_close = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(close_codec_handle, 0),
            1,
        ))
        .bits(),
    ));
    perry_ffi::set_closure_capture_f64(
        JsValue::from_bits(handle_close.get().to_bits()).as_pointer::<RawClosureHeader>(),
        0,
        owner.get(),
    );
    np::own(handle.get(), "close", handle_close.get());
    np::own(owner.get(), "_handle", handle.get());
    own_new!("_outBuffer", value_bytes(&vec![0; chunk_size]));
    np::own(owner.get(), "_outOffset", 0.0);
    np::own(owner.get(), "_chunkSize", chunk_size as f64);
    let family_kind = if zstd {
        2
    } else if matches!(codec, Codec::BrotliCompress | Codec::BrotliDecompress) {
        1
    } else {
        0
    };
    for (key, value) in [
        ("_defaultFlushFlag", 0.0),
        ("_finishFlushFlag", if family_kind == 0 { 4.0 } else { 2.0 }),
        (
            "_defaultFullFlushFlag",
            if family_kind == 0 { 3.0 } else { 1.0 },
        ),
    ] {
        np::own(owner.get(), key, value);
    }
    np::own(owner.get(), "_flushBoundIdx", family_kind as f64);
    if let Some(base) = &gzip_base {
        for (key, option) in [
            ("_defaultFlushFlag", "flush"),
            ("_finishFlushFlag", "finishFlush"),
        ] {
            let value = field(base.get(), option);
            np::own(owner.get(), key, value);
        }
        let source = transform_source.as_ref().unwrap();
        own_new!("_info", field(source.get(), "info"));
        own_new!("_maxOutputLength", field(base.get(), "maxOutputLength"));
        let reject = field(source.get(), "rejectGarbageAfterEnd");
        np::own(
            owner.get(),
            "_rejectGarbageAfterEnd",
            f64::from_bits(if reject.to_bits() == JsValue::TRUE.bits() {
                JsValue::TRUE.bits()
            } else {
                JsValue::FALSE.bits()
            }),
        );
    } else {
        np::own(owner.get(), "_info", f64::from_bits(UNDEFINED));
        np::own(owner.get(), "_maxOutputLength", 9007199254740991.0);
        np::own(
            owner.get(),
            "_rejectGarbageAfterEnd",
            f64::from_bits(JsValue::FALSE.bits()),
        );
    }
    if family_kind == 0 {
        let level = field(
            gzip_options
                .as_ref()
                .map_or_else(|| opts.get(), |captured| captured.get()),
            "level",
        );
        np::own(
            owner.get(),
            "_level",
            if level.to_bits() == UNDEFINED {
                -1.0
            } else {
                level
            },
        );
        let strategy = field(
            gzip_options
                .as_ref()
                .map_or_else(|| opts.get(), |captured| captured.get()),
            "strategy",
        );
        np::own(
            owner.get(),
            "_strategy",
            if strategy.to_bits() == UNDEFINED {
                0.0
            } else {
                strategy
            },
        );
        np::own(
            owner.get(),
            "_mode",
            match codec {
                Codec::Deflate => 1.0,
                Codec::Inflate => 2.0,
                Codec::Gzip => 3.0,
                Codec::Gunzip => 4.0,
                Codec::DeflateRaw => 5.0,
                Codec::InflateRaw => 6.0,
                _ => 7.0,
            },
        );
    }
    if zstd {
        #[cfg(test)]
        if std::env::var("PERRY_TEST_ZLIB_CONSTRUCTOR_GC").is_ok() {
            perry_runtime::gc::js_gc_collect();
        }
        own_new!(
            "_writeState",
            f64::from_bits(JsValue::from_object_ptr(js_typed_array_new_empty(5, 2)).bits())
        );
    }
    #[cfg(test)]
    if sabotage("own_codec_methods") {
        np::own(owner.get(), "_transform", field(owner.get(), "_transform"));
    }
    owner.get()
}
extern "C" fn prefinish(_: *const RawClosureHeader, _: JsThis) -> f64 {
    f64::from_bits(UNDEFINED)
}
extern "C" fn reset(_: *const RawClosureHeader, this: JsThis) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let bytes = unsafe {
        match payload(owner.get()) {
            Some(p) => {
                p.reset().unwrap();
                p.external_bytes()
            }
            None => perry_ffi::throw_with_code(
                "zlib binding closed",
                "ERR_INTERNAL_ASSERTION",
                ErrorKind::Error,
            ),
        }
    };
    np::set_external_bytes(owner.get(), &CODEC_VTABLE, bytes);
    progress_safe(owner.get());
    f64::from_bits(UNDEFINED)
}
fn progress_safe(owner: f64) {
    unsafe {
        progress(owner);
    }
}
extern "C" fn flush(_: *const RawClosureHeader, this: JsThis, kind: f64, callback: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let cb = roots.root_nanbox(if unsafe { js_zlib_is_callback(kind) } != 0 {
        kind
    } else {
        callback
    });
    let kind = if unsafe { js_zlib_is_callback(kind) } != 0 || kind.to_bits() == UNDEFINED {
        field(owner.get(), "_defaultFullFlushFlag")
    } else {
        kind
    };
    ns::request_flush(owner.get(), kind as i32, cb.get());
    f64::from_bits(UNDEFINED)
}
// Closing the binding releases only its codec, not the owning Transform.
// Keep the real owner traced even when JS retains the handle after destroy.
extern "C" fn close_codec_handle(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(closure, 0) });
    unsafe { np::close_attached(owner.get(), &CODEC_VTABLE) };
    f64::from_bits(UNDEFINED)
}
extern "C" fn close(_: *const RawClosureHeader, this: JsThis, callback: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let callback = roots.root_nanbox(callback);
    unsafe {
        if js_zlib_is_callback(callback.get()) != 0 {
            let event = roots.root_nanbox(f64::from_bits(
                JsValue::from_string_ptr(alloc_string("close").as_raw()).bits(),
            ));
            method(owner.get(), "once", &[event.get(), callback.get()]);
        }
        method(owner.get(), "destroy", &[]);
    }
    f64::from_bits(UNDEFINED)
}
extern "C" fn transform(
    _: *const RawClosureHeader,
    this: JsThis,
    chunk: f64,
    _enc: f64,
    cb: f64,
) -> f64 {
    ns::prototype_step(this.as_f64(), chunk, cb, false);
    f64::from_bits(UNDEFINED)
}
extern "C" fn final_flush(_: *const RawClosureHeader, this: JsThis, cb: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let cb = roots.root_nanbox(cb);
    let empty = roots.root_nanbox(value_bytes(&[]));
    let done = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(final_write_done, 1),
            2,
        ))
        .bits(),
    ));
    for (index, value) in [owner.get(), cb.get()].into_iter().enumerate() {
        unsafe {
            perry_ffi::set_closure_capture_f64(
                JsValue::from_bits(done.get().to_bits()).as_pointer(),
                index as u32,
                value,
            );
        }
    }
    unsafe {
        method(
            owner.get(),
            "_transform",
            &[empty.get(), f64::from_bits(UNDEFINED), done.get()],
        );
    }
    f64::from_bits(UNDEFINED)
}
extern "C" fn final_write_done(closure: *const RawClosureHeader, this: JsThis, err: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(closure, 0) });
    let cb = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(closure, 1) });
    let err = roots.root_nanbox(err);
    if err.get().to_bits() != UNDEFINED && err.get().to_bits() != JsValue::NULL.bits() {
        unsafe {
            perry_ffi::call_value(cb.get(), this, &[err.get()]);
        }
    } else {
        ns::prototype_step(owner.get(), f64::from_bits(UNDEFINED), cb.get(), true);
    }
    f64::from_bits(UNDEFINED)
}
extern "C" fn final_callback(_: *const RawClosureHeader, this: JsThis, cb: f64) -> f64 {
    unsafe { perry_ffi::call_value(cb, this, &[]) };
    f64::from_bits(UNDEFINED)
}
extern "C" fn destroy_callback(_: *const RawClosureHeader, this: JsThis, err: f64, cb: f64) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let err = roots.root_nanbox(err);
    let cb = roots.root_nanbox(cb);
    unsafe {
        release(owner.get());
        perry_ffi::call_value(cb.get(), JsThis::from_f64(owner.get()), &[err.get()])
    };
    f64::from_bits(UNDEFINED)
}
extern "C" fn params(
    _: *const RawClosureHeader,
    this: JsThis,
    level: f64,
    strategy: f64,
    cb: f64,
) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(this.as_f64());
    let cb = roots.root_nanbox(cb);
    let level = unsafe { js_zlib_validate_params(level, strategy) };
    let job = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(params_done, 0),
            4,
        ))
        .bits(),
    ));
    for (index, value) in [owner.get(), level as f64, strategy, cb.get()]
        .into_iter()
        .enumerate()
    {
        unsafe {
            perry_ffi::set_closure_capture_f64(
                JsValue::from_bits(job.get().to_bits()).as_pointer(),
                index as u32,
                value,
            )
        };
    }
    ns::request_flush(owner.get(), 2, job.get());
    f64::from_bits(UNDEFINED)
}
extern "C" fn params_done(c: *const RawClosureHeader, _: JsThis) -> f64 {
    let roots = TransientRootScope::enter();
    let owner = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(c, 0) });
    let level = unsafe { perry_ffi::closure_capture_f64(c, 1) };
    let strategy = unsafe { perry_ffi::closure_capture_f64(c, 2) };
    let cb = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(c, 3) });
    let bytes = unsafe {
        payload(owner.get()).map(|p| {
            p.params(Compression::new(level as u32), strategy as i32);
            p.external_bytes()
        })
    };
    if let Some(bytes) = bytes {
        np::set_external_bytes(owner.get(), &CODEC_VTABLE, bytes);
        np::own(owner.get(), "_level", level);
        np::own(owner.get(), "_strategy", strategy);
    }
    if unsafe { js_zlib_is_callback(cb.get()) } != 0 {
        unsafe {
            perry_ffi::call_value(cb.get(), JsThis::from_f64(owner.get()), &[]);
        }
    }
    f64::from_bits(UNDEFINED)
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod runtime_tests;

#[cfg(test)]
fn sabotage(name: &str) -> bool {
    std::env::var("PERRY_TEST_ZLIB_SABOTAGE").is_ok_and(|fault| fault == name)
}
