use perry_ffi::{NativeAsyncCompletion, Promise};
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
pub extern "C" fn perry_ffi_promise_reject_deferred(
    promise: *mut Promise,
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void) -> u64,
) {
    perry_ffi_promise_reject_bits(promise, invoke(ctx));
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

// The native-completion ABI is linked into perry-ffi even though ext-net's
// tests do not exercise it. Keep inert definitions here so the standalone
// crate test binary does not need the full Perry host archive.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_new(_flags: u32) -> *mut NativeAsyncCompletion {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_promise(
    _token: *mut NativeAsyncCompletion,
) -> *mut Promise {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_resolve_bits(
    _token: *mut NativeAsyncCompletion,
    _bits: u64,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_bits(
    _token: *mut NativeAsyncCompletion,
    _bits: u64,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_string(
    _token: *mut NativeAsyncCompletion,
    _data: *const u8,
    _len: usize,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_cancel(_token: *mut NativeAsyncCompletion) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_attach_handle(
    _token: *mut NativeAsyncCompletion,
    _handle_bits: u64,
    _cleanup_flags: u32,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(_budget_ms: u64) {}

#[no_mangle]
pub extern "C" fn js_tls_client_preflight(
    _port: f64,
    _servername_ptr: *const u8,
    _servername_len: usize,
    _options: f64,
) -> i32 {
    0
}
