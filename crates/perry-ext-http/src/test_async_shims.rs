// Unit-test binaries for `perry-ext-http` do not link the host stdlib/runtime
// archive that normally provides the perry_ffi async bridge (the real symbols
// live in `perry-stdlib::perry_ffi_async`, only linked into the final user
// program). Provide synchronous, test-only shims for the `perry_ffi_*` async
// externs the crate references so `cargo test -p perry-ext-http` links — same
// pattern as `perry-ext-net` / `perry-ext-fetch`.

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

// Linking the ws dispatch extension also retains its synchronous polling
// helper; real networking is exercised by the compiled HTTP/WebSocket
// integration tests. Likewise `js_bun_tcp_listen` turns the loop from its
// bind-poll loop (`perry_ffi::run_pending`), so linking perry-ext-net's object code into
// this crate's test binaries pulls the extern in with it.
#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(_budget_ms: u64) {}

// P5: `perry-ext-net`'s TLS layer holds an `'upgradeToTLS'` promise as a
// `JsNativeAsyncCompletion` (the runtime's pinned, root-scanned handle — #9552
// — rather than a bare `*mut Promise` in a side table). ext-net's own shims are
// `#[cfg(test)]`, so they exist only in ITS test binary; this crate links
// ext-net as an ordinary rlib and therefore has to satisfy the same symbols in
// its own. Synchronous no-ops: nothing in this crate's unit tests settles one.

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_new(_flags: u32) -> *mut perry_ffi::NativeAsyncCompletion {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_promise(
    _token: *mut perry_ffi::NativeAsyncCompletion,
) -> *mut perry_ffi::Promise {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_resolve_bits(
    _token: *mut perry_ffi::NativeAsyncCompletion,
    _bits: u64,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_bits(
    _token: *mut perry_ffi::NativeAsyncCompletion,
    _bits: u64,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_string(
    _token: *mut perry_ffi::NativeAsyncCompletion,
    _data: *const u8,
    _len: usize,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_cancel(
    _token: *mut perry_ffi::NativeAsyncCompletion,
) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_attach_handle(
    _token: *mut perry_ffi::NativeAsyncCompletion,
    _handle_bits: u64,
    _cleanup_flags: u32,
) -> i32 {
    0
}

// #10428: the client handle-dispatch extension registered from
// `ensure_gc_scanner_registered` references the Agent's `createConnection`
// path, which retains perry-ext-net's TLS connect and its host-provided
// SNI/ALPN preflight hook. Same no-op shim perry-ext-net's own tests use.
#[no_mangle]
pub extern "C" fn js_tls_client_preflight(
    _port: f64,
    _servername_ptr: *const u8,
    _servername_len: usize,
    _options: f64,
) -> i32 {
    0
}
