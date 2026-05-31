//! `util.setTraceSigInt(enable)` (#2514) — toggle printing a JS stack trace on
//! SIGINT. `enable` must be a boolean (else `ERR_INVALID_ARG_TYPE`); the call
//! returns `undefined`.
//!
//! Perry does not install a SIGINT stack-trace handler, so this validates the
//! argument and is otherwise a no-op — matching Node's observable contract
//! (boolean in, `undefined` out, throw on non-boolean).

use crate::value::{JSValue, TAG_UNDEFINED};

#[no_mangle]
pub extern "C" fn js_util_set_trace_sig_int(enable: f64) -> f64 {
    if !JSValue::from_bits(enable.to_bits()).is_bool() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"enable\" argument must be of type boolean.",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    f64::from_bits(TAG_UNDEFINED)
}
