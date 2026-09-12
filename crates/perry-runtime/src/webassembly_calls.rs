//! Numeric argument/result marshalling for Wasm function calls.

use super::*;

extern "C" fn js_wasm_instance_result_then(
    closure: *const crate::closure::ClosureHeader,
    on_fulfilled: f64,
    _on_rejected: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 0));
    let on_fulfilled = scope.root_nanbox_f64(on_fulfilled);
    let outcome = crate::exception::catch_js_throw(|| unsafe {
        crate::closure::js_native_call_value(
            on_fulfilled.get_nanbox_f64(),
            [value.get_nanbox_f64()].as_ptr(),
            1,
        )
    });
    let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    promise.with_mut_ptr(|p: *mut crate::promise::Promise| match outcome {
        Ok(result) => crate::promise::js_promise_resolve(p, result),
        Err(reason) => crate::promise::js_promise_reject(p, reason),
    });
    promise
        .with_mut_ptr(|p: *mut crate::promise::Promise| crate::value::js_nanbox_pointer(p as i64))
}

fn instance_result_object(module: f64, instance: f64) -> f64 {
    let object = crate::object::js_object_alloc(0, 0);
    let object = object_set(object, b"module", module);
    let object = object_set(object, b"instance", instance);
    object_value(object)
}

macro_rules! wasm_export_call_shim {
    ($name:ident $(, $arg:ident)*) => {
        pub(super) extern "C" fn $name(
            closure: *const crate::closure::ClosureHeader,
            $($arg: f64),*
        ) -> f64 {
            call_captured_wasm_export(closure, &[$($arg),*])
        }
    };
}

wasm_export_call_shim!(js_wasm_export_call_0);
wasm_export_call_shim!(js_wasm_export_call_1, a0);
wasm_export_call_shim!(js_wasm_export_call_2, a0, a1);
wasm_export_call_shim!(js_wasm_export_call_3, a0, a1, a2);
wasm_export_call_shim!(js_wasm_export_call_4, a0, a1, a2, a3);
wasm_export_call_shim!(js_wasm_export_call_5, a0, a1, a2, a3, a4);
wasm_export_call_shim!(js_wasm_export_call_6, a0, a1, a2, a3, a4, a5);
wasm_export_call_shim!(js_wasm_export_call_7, a0, a1, a2, a3, a4, a5, a6);
wasm_export_call_shim!(js_wasm_export_call_8, a0, a1, a2, a3, a4, a5, a6, a7);
wasm_export_call_shim!(js_wasm_export_call_9, a0, a1, a2, a3, a4, a5, a6, a7, a8);
wasm_export_call_shim!(
    js_wasm_export_call_10,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9
);
wasm_export_call_shim!(
    js_wasm_export_call_11,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10
);
wasm_export_call_shim!(
    js_wasm_export_call_12,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11
);
wasm_export_call_shim!(
    js_wasm_export_call_13,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12
);
wasm_export_call_shim!(
    js_wasm_export_call_14,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13
);
wasm_export_call_shim!(
    js_wasm_export_call_15,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13,
    a14
);
wasm_export_call_shim!(
    js_wasm_export_call_16,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13,
    a14,
    a15
);

pub(super) fn wasm_export_call_shim_for_arity(arity: usize) -> (*const u8, u32) {
    match arity {
        0 => (js_wasm_export_call_0 as *const u8, 0),
        1 => (js_wasm_export_call_1 as *const u8, 1),
        2 => (js_wasm_export_call_2 as *const u8, 2),
        3 => (js_wasm_export_call_3 as *const u8, 3),
        4 => (js_wasm_export_call_4 as *const u8, 4),
        5 => (js_wasm_export_call_5 as *const u8, 5),
        6 => (js_wasm_export_call_6 as *const u8, 6),
        7 => (js_wasm_export_call_7 as *const u8, 7),
        8 => (js_wasm_export_call_8 as *const u8, 8),
        9 => (js_wasm_export_call_9 as *const u8, 9),
        10 => (js_wasm_export_call_10 as *const u8, 10),
        11 => (js_wasm_export_call_11 as *const u8, 11),
        12 => (js_wasm_export_call_12 as *const u8, 12),
        13 => (js_wasm_export_call_13 as *const u8, 13),
        14 => (js_wasm_export_call_14 as *const u8, 14),
        15 => (js_wasm_export_call_15 as *const u8, 15),
        _ => (js_wasm_export_call_16 as *const u8, 16),
    }
}

pub(super) fn is_wasm_export_call_shim(function: *const u8) -> bool {
    matches!(
        function,
        f if f == js_wasm_export_call_0 as *const u8
            || f == js_wasm_export_call_1 as *const u8
            || f == js_wasm_export_call_2 as *const u8
            || f == js_wasm_export_call_3 as *const u8
            || f == js_wasm_export_call_4 as *const u8
            || f == js_wasm_export_call_5 as *const u8
            || f == js_wasm_export_call_6 as *const u8
            || f == js_wasm_export_call_7 as *const u8
            || f == js_wasm_export_call_8 as *const u8
            || f == js_wasm_export_call_9 as *const u8
            || f == js_wasm_export_call_10 as *const u8
            || f == js_wasm_export_call_11 as *const u8
            || f == js_wasm_export_call_12 as *const u8
            || f == js_wasm_export_call_13 as *const u8
            || f == js_wasm_export_call_14 as *const u8
            || f == js_wasm_export_call_15 as *const u8
            || f == js_wasm_export_call_16 as *const u8
    )
}

/// Perry's synchronous wasm adapter reads `.instance` immediately, while
/// Emscripten's async side-module path calls `.then(...)`. Publish both views:
/// the direct result is a thenable whose fulfillment value is a plain result
/// object, avoiding recursive thenable assimilation.
pub(super) fn make_instance_result(module: *mut c_void, inst: *mut c_void, imports: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(make_instance_value(
        module,
        inst,
        imports,
        nanbox_undefined(),
    ));
    let module = scope.root_nanbox_f64(make_module_object(module));
    let settlement = scope.root_nanbox_f64(instance_result_object(
        module.get_nanbox_f64(),
        instance.get_nanbox_f64(),
    ));
    let direct = scope.root_nanbox_f64(instance_result_object(
        module.get_nanbox_f64(),
        instance.get_nanbox_f64(),
    ));
    let function = js_wasm_instance_result_then as *const u8;
    crate::closure::js_register_closure_arity(function, 2);
    let then = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(function, 1));
    then.with_mut_ptr(|t: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(t, 0, settlement.get_nanbox_f64())
    });
    let direct_ptr = JSValue::from_bits(direct.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let direct_ptr = object_set(
        direct_ptr,
        b"then",
        then.with_mut_ptr(|t: *mut crate::closure::ClosureHeader| {
            crate::value::js_nanbox_pointer(t as i64)
        }),
    );
    object_value(direct_ptr)
}

/// The most arguments any exported-function shim forwards, matching Perry's
/// existing `js_closure_call0..16` dynamic-call ABI. Keeping the
/// marshalling buffers on the stack keeps a Wasm call allocation-free.
const MAX_WASM_ARGS: usize = 16;
const MAX_WASM_RESULTS: usize = 16;

fn encode_wasm_args(
    args: &[f64],
    kinds: &mut [u8; MAX_WASM_ARGS],
    bits: &mut [u64; MAX_WASM_ARGS],
) {
    for (index, value) in args.iter().take(MAX_WASM_ARGS).enumerate() {
        if let Some(i64_bits) = js_to_wasm_i64_bits(*value) {
            kinds[index] = WASM_VAL_KIND_I64;
            bits[index] = i64_bits;
            continue;
        }
        let as_i32 = *value as i32;
        if (as_i32 as f64) == *value && value.is_finite() {
            kinds[index] = WASM_VAL_KIND_I32;
            bits[index] = as_i32 as u32 as u64;
        } else {
            kinds[index] = WASM_VAL_KIND_F64;
            bits[index] = value.to_bits();
        }
    }
}

pub(super) fn js_to_wasm_i64_bits(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    let js = JSValue::from_bits(bits);
    let raw = (bits & POINTER_MASK) as *const crate::bigint::BigIntHeader;
    let is_bigint = js.is_bigint()
        || (js.is_pointer()
            && unsafe { crate::value::addr_class::try_read_gc_header(raw as usize) }
                .is_some_and(|header| header.obj_type == crate::gc::GC_TYPE_BIGINT));
    if !is_bigint {
        return None;
    }
    let bigint = crate::bigint::clean_bigint_ptr(raw);
    (!bigint.is_null()).then(|| unsafe { (*bigint).limbs[0] })
}

pub(super) fn wasm_i64_to_js(bits: u64) -> f64 {
    let bigint = crate::bigint::js_bigint_from_i64(bits as i64);
    f64::from_bits(JSValue::bigint_ptr(bigint).bits())
}

pub(super) fn decode_wasm_value(kind: u8, bits: u64) -> f64 {
    match kind {
        WASM_VAL_KIND_I32 => (bits as u32 as i32) as f64,
        WASM_VAL_KIND_I64 => wasm_i64_to_js(bits),
        WASM_VAL_KIND_F32 => f32::from_bits(bits as u32) as f64,
        WASM_VAL_KIND_F64 => f64::from_bits(bits),
        _ => nanbox_undefined(),
    }
}

fn decode_wasm_results(
    out_kinds: &[u8; MAX_WASM_RESULTS],
    out_bits: &[u64; MAX_WASM_RESULTS],
    out_count: usize,
) -> f64 {
    match out_count {
        0 => nanbox_undefined(),
        1 => decode_wasm_value(out_kinds[0], out_bits[0]),
        count => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let array = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(
                count.min(MAX_WASM_RESULTS) as u32,
            )));
            for index in 0..count.min(MAX_WASM_RESULTS) {
                let array_ptr = JSValue::from_bits(array.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::array::ArrayHeader>()
                    as *mut crate::array::ArrayHeader;
                let array_ptr = crate::array::js_array_push_f64(
                    array_ptr,
                    decode_wasm_value(out_kinds[index], out_bits[index]),
                );
                array.set_nanbox_f64(array_value(array_ptr));
            }
            array.get_nanbox_f64()
        }
    }
}

pub(super) fn call_export_by_handle(inst: *mut c_void, handle: usize, args: &[f64]) -> f64 {
    let mut kinds = [WASM_VAL_KIND_NONE; MAX_WASM_ARGS];
    let mut bits = [0u64; MAX_WASM_ARGS];
    encode_wasm_args(args, &mut kinds, &mut bits);
    let arg_count = args.len().min(MAX_WASM_ARGS);
    let mut out_kinds = [WASM_VAL_KIND_NONE; MAX_WASM_RESULTS];
    let mut out_bits = [0u64; MAX_WASM_RESULTS];
    let mut out_count = 0usize;
    let mut err: *mut c_char = std::ptr::null_mut();
    let ok = unsafe {
        perry_wasm_host_call_export_by_handle(
            inst,
            handle,
            kinds.as_ptr(),
            bits.as_ptr(),
            arg_count,
            out_kinds.as_mut_ptr(),
            out_bits.as_mut_ptr(),
            MAX_WASM_RESULTS,
            &mut out_count,
            &mut err,
        )
    };
    if ok == 0 {
        emit_error_to_stderr("WebAssembly.RuntimeError", err);
        return nanbox_undefined();
    }
    if !err.is_null() {
        unsafe { perry_wasm_host_string_free(err) };
    }
    decode_wasm_results(&out_kinds, &out_bits, out_count)
}

pub(super) fn call_external_function(external: *mut c_void, args: &[f64]) -> f64 {
    let mut kinds = [WASM_VAL_KIND_NONE; MAX_WASM_ARGS];
    let mut bits = [0u64; MAX_WASM_ARGS];
    encode_wasm_args(args, &mut kinds, &mut bits);
    let arg_count = args.len().min(MAX_WASM_ARGS);
    let mut out_kinds = [WASM_VAL_KIND_NONE; MAX_WASM_RESULTS];
    let mut out_bits = [0u64; MAX_WASM_RESULTS];
    let mut out_count = 0usize;
    let mut err: *mut c_char = std::ptr::null_mut();
    let ok = unsafe {
        perry_wasm_host_func_call(
            external,
            kinds.as_ptr(),
            bits.as_ptr(),
            arg_count,
            out_kinds.as_mut_ptr(),
            out_bits.as_mut_ptr(),
            MAX_WASM_RESULTS,
            &mut out_count,
            &mut err,
        )
    };
    if ok == 0 {
        emit_error_to_stderr("WebAssembly.RuntimeError", err);
        return nanbox_undefined();
    }
    if !err.is_null() {
        unsafe { perry_wasm_host_string_free(err) };
    }
    decode_wasm_results(&out_kinds, &out_bits, out_count)
}

pub(super) fn call_export_n(inst_jsval: f64, name_jsval: f64, args: &[f64]) -> f64 {
    let inst = unbox_pointer(inst_jsval);
    if inst.is_null() {
        eprintln!("WebAssembly.callExport: instance handle is null/undefined");
        return nanbox_undefined();
    }
    let Some((name_ptr, name_len)) = extract_string_bytes(name_jsval) else {
        eprintln!("WebAssembly.callExport: export name must be a string");
        return nanbox_undefined();
    };
    let mut kinds = [WASM_VAL_KIND_NONE; MAX_WASM_ARGS];
    let mut bits = [0u64; MAX_WASM_ARGS];
    encode_wasm_args(args, &mut kinds, &mut bits);
    let arg_count = args.len().min(MAX_WASM_ARGS);
    let mut out_kinds = [WASM_VAL_KIND_NONE; MAX_WASM_RESULTS];
    let mut out_bits = [0u64; MAX_WASM_RESULTS];
    let mut out_count = 0usize;
    let mut err: *mut c_char = std::ptr::null_mut();
    let ok = unsafe {
        perry_wasm_host_call_export(
            inst,
            name_ptr as *const c_char,
            name_len,
            kinds.as_ptr(),
            bits.as_ptr(),
            arg_count,
            out_kinds.as_mut_ptr(),
            out_bits.as_mut_ptr(),
            MAX_WASM_RESULTS,
            &mut out_count,
            &mut err,
        )
    };
    if ok == 0 {
        emit_error_to_stderr("WebAssembly.RuntimeError", err);
        return nanbox_undefined();
    }
    if !err.is_null() {
        unsafe { perry_wasm_host_string_free(err) };
    }
    decode_wasm_results(&out_kinds, &out_bits, out_count)
}
