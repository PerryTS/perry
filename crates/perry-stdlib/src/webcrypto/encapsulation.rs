use super::util::*;

unsafe fn reject_type_error_with_code(message: &str, code: &'static str) -> *mut Promise {
    let msg = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, code);
    let err = perry_runtime::error::js_typeerror_new(msg);
    let value = f64::from_bits(JSValue::pointer(err as *const u8).bits());
    perry_runtime::js_promise_rejected(value)
}

unsafe fn reject_missing_args(method: &str, required: usize, present: usize) -> *mut Promise {
    let message = format!(
        "Failed to execute '{method}' on 'SubtleCrypto': {required} arguments required, but only {present} present."
    );
    reject_type_error_with_code(&message, "ERR_MISSING_ARGS")
}

unsafe fn reject_unsupported_algorithm() -> *mut Promise {
    reject_with_dom_exception("NotSupportedError", "Unrecognized algorithm name")
}

unsafe fn encapsulation_stub(method: &str, required: usize, args_len: usize) -> *mut Promise {
    if args_len < required {
        return reject_missing_args(method, required, args_len);
    }
    reject_unsupported_algorithm()
}

pub unsafe fn js_webcrypto_encapsulate_bits(
    _args_ptr: *const f64,
    args_len: usize,
) -> *mut Promise {
    encapsulation_stub("encapsulateBits", 2, args_len)
}

pub unsafe fn js_webcrypto_decapsulate_bits(
    _args_ptr: *const f64,
    args_len: usize,
) -> *mut Promise {
    encapsulation_stub("decapsulateBits", 3, args_len)
}

pub unsafe fn js_webcrypto_encapsulate_key(_args_ptr: *const f64, args_len: usize) -> *mut Promise {
    encapsulation_stub("encapsulateKey", 5, args_len)
}

pub unsafe fn js_webcrypto_decapsulate_key(_args_ptr: *const f64, args_len: usize) -> *mut Promise {
    encapsulation_stub("decapsulateKey", 6, args_len)
}
