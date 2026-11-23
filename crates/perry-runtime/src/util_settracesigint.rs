//! `util.setTraceSigInt(enable)` (#2514) — toggle printing a JS stack trace on
//! SIGINT. Node reads `enable` for truthiness only and returns `undefined`.
//!
//! Perry does not install a SIGINT stack-trace handler, so the call is a no-op
//! with Node's observable contract: any argument in, `undefined` out.

use crate::value::TAG_UNDEFINED;

#[no_mangle]
pub extern "C" fn js_util_set_trace_sig_int(_enable: f64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
