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
    fn js_typed_array_new_empty(kind: i32, length: i32) -> *mut c_void;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    pub(crate) fn js_zlib_resolve_level(opts: f64) -> i32;
    pub(crate) fn js_zlib_validate_params(level: f64, strategy: f64) -> i32;
    pub(crate) fn js_zlib_validate_options(opts: f64, min_window_bits: i32);
    pub(crate) fn js_zlib_validate_buffer_arg(data_bits: i64);
    pub(crate) fn js_zlib_validate_callback(callback: f64) -> i64;
}
mod allocation;
mod driver;
mod one_shot_callback;
mod zlib_encoder;
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

/// `zlib.brotliDecompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_bits` must be the raw NaN-box bit pattern of the data argument.
#[no_mangle]
pub unsafe extern "C" fn js_zlib_brotli_decompress_sync(data_bits: i64) -> f64 {
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits).map(|d| brotli_decompress_bytes(&d)) {
        Some(Ok(out)) => value_bytes(&out),
        Some(Err(_)) => throw_brotli_decode_error(),
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
    perry_ffi::throw_with_code(&format!("zstd: {}", err), "Z_DATA_ERROR", ErrorKind::Error)
}

/// `zlib.zstdCompressSync(data)` -> Buffer. `_opts` is accepted (codegen
/// passes the options slot through) but zstd params are not wired up —
/// matches perry-stdlib's copy.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_compress_sync(data_value: f64, _opts: f64) -> f64 {
    let data_bits = data_value.to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits).map(|d| zstd::bulk::compress(&d, ZSTD_DEFAULT_LEVEL)) {
        Some(Ok(out)) => value_bytes(&out),
        Some(Err(e)) => throw_zstd_error(&e),
        None => f64::from_bits(UNDEFINED),
    }
}

/// `zlib.zstdDecompressSync(data)` -> Buffer.
///
/// # Safety
/// `data_value` is the raw NaN-boxed data argument (string or Buffer).
#[no_mangle]
pub unsafe extern "C" fn js_zlib_zstd_decompress_sync(data_value: f64, _opts: f64) -> f64 {
    let data_bits = data_value.to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    match read_input_from_bits(data_bits).map(|d| zstd::stream::decode_all(d.as_slice())) {
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
    js_zlib_validate_options(opts.get(), if matches!(codec, Codec::Gzip) { 9 } else { 8 });
    let level = if matches!(codec, Codec::Gzip | Codec::Deflate | Codec::DeflateRaw) {
        compression_from_opts(opts.get())
    } else {
        Compression::default()
    };
    let chunk_size = js_zlib_stream_option(opts.get(), 0);
    let _base = roots.root_nanbox(np::prototype(&BASE, "zlib"));
    let proto = roots.root_nanbox(np::prototype(family, "zlib"));
    let codec_payload = driver::Payload::new(codec, level, chunk_size).unwrap();
    let native_bytes = codec_payload.external_bytes();
    let owner = if let Some(owner) = existing {
        assert!(np::attach(owner.get(), family, codec_payload, native_bytes));
        owner
    } else {
        roots.root_nanbox(np::alloc(family, codec_payload, proto.get(), native_bytes))
    };
    let zstd = matches!(codec, Codec::ZstdCompress | Codec::ZstdDecompress);
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
    own_new!(
        "_events",
        f64::from_bits(perry_ffi::alloc_null_proto_object(&[]).bits())
    );
    assert!(ns::init_transform_in_place(owner.get(), opts.get()));
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
    own_new!("_handle", f64::from_bits(perry_ffi::alloc_object().bits()));
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
    np::own(owner.get(), "_info", f64::from_bits(UNDEFINED));
    np::own(owner.get(), "_maxOutputLength", 9007199254740991.0);
    np::own(
        owner.get(),
        "_rejectGarbageAfterEnd",
        f64::from_bits(JsValue::FALSE.bits()),
    );
    if family_kind == 0 {
        let level = field(opts.get(), "level");
        np::own(
            owner.get(),
            "_level",
            if level.to_bits() == UNDEFINED {
                -1.0
            } else {
                level
            },
        );
        let strategy = field(opts.get(), "strategy");
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
