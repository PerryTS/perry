// Unit-test binaries for `perry-ext-http` do not link the host stdlib/runtime
// archive that normally provides the perry_ffi async bridge (the real symbols
// live in `perry-stdlib::perry_ffi_async`, only linked into the final user
// program). Provide synchronous, test-only shims for the `perry_ffi_*` async
// externs the crate references so `cargo test -p perry-ext-http` links — same
// pattern as `perry-ext-net` / `perry-ext-http-server`.

use perry_ffi::Promise;
use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn perry_ffi_promise_new() -> *mut Promise {
    perry_runtime::promise::js_promise_new() as *mut Promise
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_bits(promise: *mut Promise, bits: u64) {
    perry_runtime::promise::js_promise_resolve(
        promise as *mut perry_runtime::Promise,
        f64::from_bits(bits),
    );
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_bits(promise: *mut Promise, bits: u64) {
    perry_runtime::promise::js_promise_reject(
        promise as *mut perry_runtime::Promise,
        f64::from_bits(bits),
    );
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    perry_ffi_promise_resolve_bits(promise, invoke(ctx));
}

#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    invoke(ctx);
}

#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking_with_reactor(
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void),
) {
    invoke(ctx);
}
