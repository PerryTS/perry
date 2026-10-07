//! One-shot work is an immediate-queue closure. Input, options and callback
//! remain traced captures through queue lifetime and moving collections.
use super::*;
use std::io::Write;
extern "C" {
    fn js_async_hooks_provider_init(name: *const u8, len: usize) -> u64;
    fn js_async_hooks_provider_run_catching_deferred_destroy(
        id: u64,
        turns: u32,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
}
pub(crate) unsafe fn queue_one_shot_callback(data: f64, options: f64, callback: f64, codec: Codec) {
    let roots = TransientRootScope::enter();
    let data = roots.root_nanbox(data);
    let (options, callback) = if js_zlib_is_callback(options) != 0 {
        (f64::from_bits(UNDEFINED), options)
    } else {
        (options, callback)
    };
    let options = roots.root_nanbox(options);
    let callback = roots.root_nanbox(callback);
    if matches!(codec, Codec::Gzip | Codec::Deflate | Codec::DeflateRaw) {
        js_zlib_validate_options(
            options.get(),
            if matches!(codec, Codec::Gzip) { 9 } else { 8 },
        );
    }
    js_zlib_validate_callback(callback.get());
    js_zlib_validate_buffer_arg(data.get().to_bits() as i64);
    let id = js_async_hooks_provider_init(b"ZLIB".as_ptr(), 4);
    let job = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(job, 0),
            5,
        ))
        .bits(),
    ));
    for (index, value) in [
        data.get(),
        options.get(),
        callback.get(),
        codec as u32 as f64,
        id as f64,
    ]
    .into_iter()
    .enumerate()
    {
        #[cfg(test)]
        let value = if index == 2 && sabotage("raw_one_shot_callback") {
            JsValue::from_bits(value.to_bits()).as_pointer::<RawClosureHeader>() as usize as f64
        } else {
            value
        };
        perry_ffi::set_closure_capture_f64(
            JsValue::from_bits(job.get().to_bits()).as_pointer(),
            index as u32,
            value,
        );
    }
    assert!(ns::queue_immediate(job.get()));
}
extern "C" fn job(c: *const RawClosureHeader, _: JsThis) -> f64 {
    let roots = TransientRootScope::enter();
    let data = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(c, 0) });
    let options = roots.root_nanbox(unsafe { perry_ffi::closure_capture_f64(c, 1) });
    let callback = unsafe { perry_ffi::closure_capture_f64(c, 2) };
    #[cfg(test)]
    let callback = if sabotage("raw_one_shot_callback") {
        f64::from_bits(JsValue::from_object_ptr(callback as usize as *mut RawClosureHeader).bits())
    } else {
        callback
    };
    let callback = roots.root_nanbox(callback);
    let codec = unsafe { perry_ffi::closure_capture_f64(c, 3) } as u32;
    let id = unsafe { perry_ffi::closure_capture_f64(c, 4) } as u64;
    let mut call = (data, options, callback, codec);
    unsafe {
        js_async_hooks_provider_run_catching_deferred_destroy(
            id,
            4,
            run,
            std::ptr::from_mut(&mut call).cast(),
        )
    }
}
unsafe extern "C" fn run(context: *mut c_void) -> f64 {
    let call = &*(context
        as *const (
            perry_ffi::TransientRootedNanbox,
            perry_ffi::TransientRootedNanbox,
            perry_ffi::TransientRootedNanbox,
            u32,
        ));
    let data = read_input_from_bits(call.0.get().to_bits() as i64).unwrap();
    let level = if matches!(call.3, 0 | 2 | 4) {
        compression_from_opts(call.1.get())
    } else {
        Compression::default()
    };
    let result = match call.3 {
        0 => crate::gzip_bytes_with(&data, level),
        1 => crate::gunzip_bytes(&data),
        2 => crate::deflate_bytes_with(&data, level),
        3 => crate::inflate_bytes(&data),
        4 => crate::deflate_raw_bytes_with(&data, level),
        5 => crate::inflate_raw_bytes(&data),
        6 => crate::unzip_bytes(&data),
        7 => Ok(brotli_compress_bytes(&data)),
        8 => brotli_decompress_bytes(&data),
        9 => (|| {
            // Node's callback helper uses the streaming encoder: its frame
            // omits the pledged content size present in zstdCompressSync.
            let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), ZSTD_DEFAULT_LEVEL)?;
            encoder.write_all(&data)?;
            encoder.finish()
        })(),
        10 => zstd::stream::decode_all(data.as_slice()),
        _ => unreachable!("private one-shot codec capture"),
    };
    let roots = TransientRootScope::enter();
    let (err, value) = match result {
        Ok(bytes) => (f64::from_bits(JsValue::NULL.bits()), value_bytes(&bytes)),
        Err(error) => {
            let message = error.to_string();
            (
                js_zlib_stream_error(
                    message.as_ptr(),
                    message.len(),
                    if error.kind() == std::io::ErrorKind::UnexpectedEof {
                        1
                    } else {
                        0
                    },
                ),
                f64::from_bits(UNDEFINED),
            )
        }
    };
    let err = roots.root_nanbox(err);
    let value = roots.root_nanbox(value);
    perry_ffi::call_value(call.2.get(), JsThis::UNDEFINED, &[err.get(), value.get()]);
    f64::from_bits(UNDEFINED)
}
