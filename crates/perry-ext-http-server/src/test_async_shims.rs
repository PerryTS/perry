use perry_ffi::Promise;
use std::ffi::c_void;

// Unit-test binaries do not link the host stdlib/runtime archive that normally
// provides the perry_ffi async bridge. Keep these synchronous shims test-only.

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

// `perry_ffi_spawn_async` is declared extern in perry-ffi's async runtime and
// normally provided by perry-stdlib / the prebuilt static archive at the final
// link. The unit-test binary pulls it in transitively via the perry-ext-net
// rlib but has no perry-stdlib edge in a fresh checkout, so it fails to link on
// this one symbol. None of this crate's unit tests touch the async runtime, so
// a no-op stub lets the test binary link. (Test-only; never shipped.)
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_async(_ctx: *mut c_void) {}
