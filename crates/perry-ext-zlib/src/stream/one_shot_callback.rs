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

// Only Rust-owned codec state and copied bytes cross the two completions.
// The private ordinary owner is a traced closure capture, not a JS address in
// native state. No constructor/prototype is installed for this attached cell.
struct Completion {
    codec: driver::Payload,
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
    let input = if call.final_phase {
        Vec::new()
    } else {
        read_input_from_bits(call.data.get().to_bits() as i64).unwrap()
    };
    if !call.final_phase {
        let codec = match call.codec {
            0 => Codec::Gzip,
            1 => Codec::Gunzip,
            2 => Codec::Deflate,
            3 => Codec::Inflate,
            4 => Codec::DeflateRaw,
            5 => Codec::InflateRaw,
            6 => Codec::Unzip,
            7 => Codec::BrotliCompress,
            8 => Codec::BrotliDecompress,
            9 => Codec::ZstdCompress,
            10 => Codec::ZstdDecompress,
            _ => unreachable!("private one-shot codec capture"),
        };
        let level = if matches!(codec, Codec::Gzip | Codec::Deflate | Codec::DeflateRaw) {
            compression_from_opts(call.options.get())
        } else {
            Compression::default()
        };
        let codec = driver::Payload::new(codec, level, 16384).unwrap();
        let bytes = codec.external_bytes() + std::mem::size_of::<Vec<u8>>();
        assert!(np::attach(
            call.owner.get(),
            &COMPLETION,
            Completion {
                codec,
                output: Vec::new()
            },
            bytes
        ));
    }
    let mut consumed = 0;
    let error = loop {
        // This borrow ends before any JS allocation, callback or GC. The
        // input is an owned copy and each scratch result is copied at once.
        let state = np::get_mut::<Completion>(call.owner.get(), &COMPLETION).unwrap();
        let mut out = ns::StepOut {
            consumed: 0,
            out: std::ptr::null(),
            out_len: 0,
            status: ns::StepStatus::NEED_INPUT,
            code: 0,
            external_bytes: 0,
        };
        state.codec.step(
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
        let bytes =
            state.codec.external_bytes() + std::mem::size_of::<Vec<u8>>() + state.output.capacity();
        let error = if out.status == ns::StepStatus::ERROR {
            Some((state.codec.error_details(out.code).0, out.code == 5))
        } else {
            None
        };
        np::set_external_bytes(call.owner.get(), &COMPLETION_VTABLE, bytes);
        if error.is_some() || out.status != ns::StepStatus::MORE {
            break error;
        }
    };
    if error.is_none() && !call.final_phase {
        return 1.0;
    }
    let output = {
        let state = np::get_mut::<Completion>(call.owner.get(), &COMPLETION).unwrap();
        std::mem::take(&mut state.output)
    };
    // Release exactly once before invoking user code, including a callback
    // that throws. The provider helper owns after hooks and deferred destroy.
    np::close(call.owner.get(), &COMPLETION);
    let roots = TransientRootScope::enter();
    let err = roots.root_nanbox(if let Some((message, truncated)) = error {
        js_zlib_stream_error(message.as_ptr(), message.len(), i32::from(truncated))
    } else {
        f64::from_bits(JsValue::NULL.bits())
    });
    let value = roots.root_nanbox(if err.get().to_bits() == JsValue::NULL.bits() {
        value_bytes(&output)
    } else {
        f64::from_bits(UNDEFINED)
    });
    perry_ffi::call_value(
        call.callback.get(),
        JsThis::UNDEFINED,
        &[err.get(), value.get()],
    );
    0.0
}
