//! Normalize async codec overloads and queue their rooted completion callback.
use super::{
    compression_from_opts, ensure_aux_pump_registered, ensure_gc_scanner_registered,
    js_async_hooks_provider_init, js_zlib_validate_buffer_arg, js_zlib_validate_callback,
    js_zlib_validate_options, read_input_from_bits, statics, ZlibEvent, UNDEFINED,
};
use flate2::Compression;
use perry_ffi::{notify_main_thread, TransientRootScope};

extern "C" {
    fn js_value_is_closure(value_bits: i64) -> i32;
}

pub(crate) unsafe fn queue_one_shot_callback<F>(
    data_value: f64,
    options: f64,
    callback_value: f64,
    label: &'static str,
    op: F,
) where
    F: FnOnce(&[u8], Compression) -> std::io::Result<Vec<u8>>,
{
    let (options, callback_value) = if js_value_is_closure(options.to_bits() as i64) != 0 {
        (f64::from_bits(UNDEFINED), options)
    } else {
        (options, callback_value)
    };
    let scope = TransientRootScope::enter();
    let data_value = scope.root_nanbox(data_value);
    let callback_value = scope.root_nanbox(callback_value);
    let options = scope.root_nanbox(options);
    // Use the same option validation and level mapping as the sync encoders.
    let level = match label {
        "Gzip" | "Deflate" | "DeflateRaw" => {
            js_zlib_validate_options(options.get(), if label == "Gzip" { 9 } else { 8 });
            compression_from_opts(options.get())
        }
        _ => Compression::default(),
    };
    let _ = js_zlib_validate_callback(callback_value.get());
    let data_bits = data_value.get().to_bits() as i64;
    js_zlib_validate_buffer_arg(data_bits);
    let result = match read_input_from_bits(data_bits) {
        Some(data) => op(&data, level).map_err(|e| format!("{} error: {}", label, e)),
        None => Err("Invalid input data".to_string()),
    };
    ensure_aux_pump_registered();
    ensure_gc_scanner_registered();
    let async_id = js_async_hooks_provider_init(b"ZLIB".as_ptr(), b"ZLIB".len());
    // Provider init delivers user hooks and may move the callback. Re-read the
    // rooted value only after it returns, immediately before publishing it in
    // the scanned pending queue.
    let callback = js_zlib_validate_callback(callback_value.get());
    statics()
        .lock()
        .unwrap()
        .pending
        .push_back(ZlibEvent::OneShotCallback(callback, result, async_id));
    notify_main_thread();
}
