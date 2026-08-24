//! Test-only host shims for the standalone sharp extension test binary.

use perry_ffi::Promise;
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn perry_ffi_promise_new() -> *mut Promise {
    perry_runtime::promise::js_promise_new() as *mut Promise
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    perry_runtime::promise::js_promise_resolve(
        promise as *mut perry_runtime::Promise,
        f64::from_bits(invoke(ctx)),
    );
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    perry_runtime::promise::js_promise_reject(
        promise as *mut perry_runtime::Promise,
        f64::from_bits(invoke(ctx)),
    );
}

#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    invoke(ctx);
}
