//! Callback helpers use the existing codec driver for a write followed by a
//! separately queued empty final flush. Both completions belong to one ZLIB.
use super::*;

extern "C" {
    fn js_async_hooks_provider_init(name: *const u8, len: usize) -> u64;
    fn js_async_hooks_provider_defer_destroy(id: u64, turns: u32);
    fn js_async_hooks_provider_run_catching_deferred_destroy(
        id: u64,
        turns: u32,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
    fn js_async_hooks_provider_run_catching_deferred_destroy_on_error(
        id: u64,
        turns: u32,
        callback: unsafe extern "C" fn(*mut c_void) -> f64,
        data: *mut c_void,
    ) -> f64;
}

// Only Rust-owned input/output cross the two completions. The working codec
// belongs exclusively to the real public engine in owner.engine.
// The private ordinary owner is a traced closure capture, not a JS address in
// native state. No constructor/prototype is installed for this attached cell.
struct Completion {
    input: Vec<u8>,
    output: Vec<u8>,
}
static COMPLETION_VTABLE: ns::PayloadVTable = ns::payload_vtable::<Completion>(None);
static COMPLETION: np::PayloadFamily =
    np::PayloadFamily::new::<Completion>(0, "ZlibCompletion", false, &COMPLETION_VTABLE);

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
    let owner = roots.root_nanbox(f64::from_bits(
        perry_ffi::alloc_null_proto_object(&[]).bits(),
    ));
    let id = js_async_hooks_provider_init(b"ZLIB".as_ptr(), 4);
    queue_job(&[
        data.get(),
        options.get(),
        callback.get(),
        codec as u32 as f64,
        id as f64,
        owner.get(),
        0.0,
    ]);
}

unsafe fn queue_job(values: &[f64; 7]) {
    let roots = TransientRootScope::enter();
    let values: Vec<_> = values.iter().map(|v| roots.root_nanbox(*v)).collect();
    let job = roots.root_nanbox(f64::from_bits(
        JsValue::from_object_ptr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(job, 0),
            7,
        ))
        .bits(),
    ));
    for (index, value) in values.iter().enumerate() {
        let value = value.get();
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

struct Call {
    data: perry_ffi::TransientRootedNanbox,
    options: perry_ffi::TransientRootedNanbox,
    callback: perry_ffi::TransientRootedNanbox,
    owner: perry_ffi::TransientRootedNanbox,
    codec: u32,
    final_phase: bool,
}
extern "C" fn job(c: *const RawClosureHeader, _: JsThis) -> f64 {
    let roots = TransientRootScope::enter();
    unsafe {
        let data = roots.root_nanbox(perry_ffi::closure_capture_f64(c, 0));
        let options = roots.root_nanbox(perry_ffi::closure_capture_f64(c, 1));
        let callback = perry_ffi::closure_capture_f64(c, 2);
        #[cfg(test)]
        let callback = if sabotage("raw_one_shot_callback") {
            f64::from_bits(
                JsValue::from_object_ptr(callback as usize as *mut RawClosureHeader).bits(),
            )
        } else {
            callback
        };
        let callback = roots.root_nanbox(callback);
        let codec = perry_ffi::closure_capture_f64(c, 3) as u32;
        let id = perry_ffi::closure_capture_f64(c, 4) as u64;
        let owner = roots.root_nanbox(perry_ffi::closure_capture_f64(c, 5));
        let final_phase = perry_ffi::closure_capture_f64(c, 6) != 0.0;
        let mut call = Call {
            data,
            options,
            callback,
            owner,
            codec,
            final_phase,
        };
        let result = if final_phase {
            js_async_hooks_provider_run_catching_deferred_destroy(
                id,
                4,
                run,
                std::ptr::from_mut(&mut call).cast(),
            )
        } else {
            js_async_hooks_provider_run_catching_deferred_destroy_on_error(
                id,
                4,
                run,
                std::ptr::from_mut(&mut call).cast(),
            )
        };
        // Queue only after the first scope restored successfully. A throwing
        // hook cannot leave a queued completion that calls the user later.
        if !final_phase {
            if result == 1.0 {
                queue_job(&[
                    f64::from_bits(UNDEFINED),
                    f64::from_bits(UNDEFINED),
                    call.callback.get(),
                    codec as f64,
                    id as f64,
                    call.owner.get(),
                    1.0,
                ]);
            } else {
                js_async_hooks_provider_defer_destroy(id, 4);
            }
        }
    }
    f64::from_bits(UNDEFINED)
}

unsafe extern "C" fn run(context: *mut c_void) -> f64 {
    let call = &*(context as *const Call);
    let roots = TransientRootScope::enter();
    if !call.final_phase {
        let input = read_input_from_bits(call.data.get().to_bits() as i64).unwrap();
        let bytes = std::mem::size_of::<Completion>() + input.capacity();
        // Transfer the copy BEFORE the deferred getter or inherited factory
        // code can throw. Abandonment retains the existing native-GC fallback.
        assert!(np::attach(
            call.owner.get(),
            &COMPLETION,
            Completion {
                input,
                output: Vec::new()
            },
            bytes,
        ));
        let (codec, family) = match call.codec {
            0 => (Codec::Gzip, &GZIP),
            1 => (Codec::Gunzip, &GUNZIP),
            2 => (Codec::Deflate, &DEFLATE),
            3 => (Codec::Inflate, &INFLATE),
            4 => (Codec::DeflateRaw, &DEFLATE_RAW),
            5 => (Codec::InflateRaw, &INFLATE_RAW),
            6 => (Codec::Unzip, &UNZIP),
            7 => (Codec::BrotliCompress, &BROTLI_COMPRESS),
            8 => (Codec::BrotliDecompress, &BROTLI_DECOMPRESS),
            9 => (Codec::ZstdCompress, &ZSTD_COMPRESS),
            10 => (Codec::ZstdDecompress, &ZSTD_DECOMPRESS),
            _ => unreachable!("private one-shot codec capture"),
        };
        let level = if matches!(codec, Codec::Gzip | Codec::Deflate | Codec::DeflateRaw) {
            compression_from_opts(call.options.get())
        } else {
            Compression::default()
        };
        // The original callback path resolved only level. Do not re-read its
        // getters or start applying previously ignored options in the factory.
        let snapshot = roots.root_nanbox(f64::from_bits(
            perry_ffi::alloc_null_proto_object(&[]).bits(),
        ));
        np::own(snapshot.get(), "level", level.level() as f64);
        let engine = roots.root_nanbox(create_stream(codec, family, snapshot.get(), None));
        // Both jobs capture the ordinary owner; this field is traced/re-written.
        // No JS address is hidden in Completion, and no second driver exists.
        np::own(call.owner.get(), "engine", engine.get());
    }
    let engine = roots.root_nanbox(field(call.owner.get(), "engine"));
    let mut consumed = 0;
    let error = loop {
        let (status, error, codec_bytes, completion_bytes) = {
            // No native borrow survives accounting, property writes or JS.
            let state = np::get_mut::<Completion>(call.owner.get(), &COMPLETION).unwrap();
            let codec = payload(engine.get()).unwrap();
            let input = if call.final_phase {
                &[][..]
            } else {
                state.input.as_slice()
            };
            let mut out = ns::StepOut {
                consumed: 0,
                out: std::ptr::null(),
                out_len: 0,
                status: ns::StepStatus::NEED_INPUT,
                code: 0,
                external_bytes: 0,
            };
            codec.step(
                &ns::StepIn {
                    op: if call.final_phase {
                        ns::StreamOp::FINAL
                    } else {
                        ns::StreamOp::WRITE
                    },
                    flush_kind: 0,
                    input: input.as_ptr().add(consumed),
                    len: input.len() - consumed,
                },
                &mut out,
            );
            consumed += out.consumed.min(input.len() - consumed);
            if out.out_len > 0 {
                state
                    .output
                    .extend_from_slice(std::slice::from_raw_parts(out.out, out.out_len));
            }
            let error = if out.status == ns::StepStatus::ERROR {
                Some((codec.error_details(out.code).0, out.code == 5))
            } else {
                None
            };
            (
                out.status,
                error,
                codec.external_bytes(),
                std::mem::size_of::<Completion>()
                    + state.input.capacity()
                    + state.output.capacity(),
            )
        };
        np::set_external_bytes(engine.get(), &CODEC_VTABLE, codec_bytes);
        np::set_external_bytes(call.owner.get(), &COMPLETION_VTABLE, completion_bytes);
        if error.is_some() || status != ns::StepStatus::MORE {
            break error;
        }
    };
    // WRITE input is no longer needed. Drop its allocation rather than merely
    // clearing length, and restate private accounting before publishing progress.
    let completion_bytes = {
        let state = np::get_mut::<Completion>(call.owner.get(), &COMPLETION).unwrap();
        drop(std::mem::take(&mut state.input));
        std::mem::size_of::<Completion>() + state.output.capacity()
    };
    np::set_external_bytes(call.owner.get(), &COMPLETION_VTABLE, completion_bytes);
    progress(engine.get());
    if error.is_none() && !call.final_phase {
        return 1.0;
    }
    // The same public engine did the work. Its codec is closed once before
    // allocating the result or invoking possibly reentrant/throwing user code.
    release(engine.get());
    let err = roots.root_nanbox(if let Some((message, truncated)) = error {
        let err = js_zlib_stream_error(message.as_ptr(), message.len(), i32::from(truncated));
        drop(message);
        err
    } else {
        f64::from_bits(JsValue::NULL.bits())
    });
    let value = roots.root_nanbox(if err.get().to_bits() == JsValue::NULL.bits() {
        let (ptr, len) = {
            let state = np::get_mut::<Completion>(call.owner.get(), &COMPLETION).unwrap();
            (state.output.as_ptr(), state.output.len())
        };
        // Rust storage stays attached to the rooted owner during allocation.
        // No native reference crosses it; the factory copies these stable bytes.
        value_bytes(std::slice::from_raw_parts(ptr, len))
    } else {
        f64::from_bits(UNDEFINED)
    });
    // Close drops output capacity too, even when user code throws past Rust Drop.
    np::close(call.owner.get(), &COMPLETION);
    if err.get().to_bits() == JsValue::NULL.bits() {
        perry_ffi::call_value(
            call.callback.get(),
            JsThis::from_f64(engine.get()),
            &[err.get(), value.get()],
        );
    } else {
        perry_ffi::call_value(
            call.callback.get(),
            JsThis::from_f64(engine.get()),
            &[err.get()],
        );
    }
    0.0
}
