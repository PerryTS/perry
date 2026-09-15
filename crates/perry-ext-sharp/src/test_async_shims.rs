//! Test-only host shims for the standalone sharp extension test binary.

use perry_ffi::{NativeAsyncCompletion, Promise};
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

// perry-ffi async ABI v2 (turnloop P4). The standalone test binary has no
// turnloop loop, so the job runs inline and is delivered immediately — the
// same shape the `spawn_blocking` shim above has always had, and the same
// thing `pool::submit_or_run_inline`'s fallback does in a real host on a
// thread with no loop. Returning a nonzero id says "accepted", so the caller
// does NOT also run its own fallback and the work happens exactly once.
#[no_mangle]
pub extern "C" fn perry_ffi_pool_submit(
    ctx: *mut c_void,
    run_on_pool: extern "C" fn(*mut c_void),
    deliver_on_owner: extern "C" fn(*mut c_void, i32),
) -> u64 {
    run_on_pool(ctx);
    deliver_on_owner(ctx, 0);
    1
}

#[no_mangle]
pub extern "C" fn perry_ffi_pool_cancel(_job: u64) -> i32 {
    // The shim completes every job before `submit` returns, so nothing is ever
    // cancellable — which is what a real host reports for a finished job too.
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_pool_turn(_budget_ms: u64) {}

#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking_with_reactor(
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void),
) {
    invoke(ctx);
}

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
