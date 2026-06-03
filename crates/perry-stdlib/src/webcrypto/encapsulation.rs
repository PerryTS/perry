use perry_runtime::{js_promise_rejected, js_string_from_bytes, JSValue, Promise};

use super::util::reject_with_dom_exception;

unsafe fn reject_type_error_with_code(message: String, code: &'static str) -> *mut Promise {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, code);
    let err = perry_runtime::error::js_typeerror_new(msg);
    let err_val = f64::from_bits(JSValue::pointer(err as *const u8).bits());
    js_promise_rejected(err_val)
}

unsafe fn reject_missing_args(method: &str, required: usize, present: usize) -> *mut Promise {
    reject_type_error_with_code(
        format!(
            "Failed to execute '{method}' on 'SubtleCrypto': {required} arguments required, but only {present} present."
        ),
        "ERR_MISSING_ARGS",
    )
}

/// Node 24+ exposes WebCrypto encapsulation method names even when callers use
/// them for feature detection. Perry does not implement ML-KEM execution yet,
/// but the surface should be explicit instead of falling through as `undefined`.
pub(super) unsafe fn encapsulation_method_dispatch(
    method: &str,
    required_args: usize,
    present_args: usize,
) -> *mut Promise {
    if present_args < required_args {
        return reject_missing_args(method, required_args, present_args);
    }
    reject_with_dom_exception(
        "NotSupportedError",
        "Unsupported WebCrypto encapsulation algorithm",
    )
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_encapsulate_bits_unimplemented(
    present_args: i64,
) -> *mut Promise {
    encapsulation_method_dispatch("encapsulateBits", 2, present_args.max(0) as usize)
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_decapsulate_bits_unimplemented(
    present_args: i64,
) -> *mut Promise {
    encapsulation_method_dispatch("decapsulateBits", 3, present_args.max(0) as usize)
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_encapsulate_key_unimplemented(
    present_args: i64,
) -> *mut Promise {
    encapsulation_method_dispatch("encapsulateKey", 5, present_args.max(0) as usize)
}

#[no_mangle]
pub unsafe extern "C" fn js_webcrypto_decapsulate_key_unimplemented(
    present_args: i64,
) -> *mut Promise {
    encapsulation_method_dispatch("decapsulateKey", 6, present_args.max(0) as usize)
}
