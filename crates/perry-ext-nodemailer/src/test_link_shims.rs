//! Link-only stand-ins for symbols a unit-test binary cannot resolve.
//!
//! In a real program this wrapper is co-linked with perry-stdlib, which
//! defines the `js_perry_smtp_*` transport seam and the `perry_ffi_promise_*`
//! async bridge. A `cargo test -p perry-ext-nodemailer` binary links neither,
//! yet `turnloop_bridge` and `perry_ffi::JsPromise` reference them, so the
//! test binary failed to link at all (#10354 introduced the seam). No unit
//! test reaches the transport or settles a promise: `available()` answers
//! "no engine" and everything else aborts loudly if a future test ever does,
//! instead of silently pretending a mail was sent (same approach as
//! perry-ext-events' `test_async_shims`).

use perry_ffi::Promise;
use std::ffi::c_void;

fn unreachable_in_unit_tests(symbol: &str) -> ! {
    eprintln!(
        "{symbol} was called from a perry-ext-nodemailer unit test; the real \
         symbol lives in perry-stdlib, which unit-test binaries do not link"
    );
    std::process::abort()
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_available() -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_begin() -> i64 {
    unreachable_in_unit_tests("js_perry_smtp_begin")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_set_server(
    _draft: i64,
    _host_ptr: *const u8,
    _host_len: usize,
    _port: u16,
    _implicit_tls: i32,
    _require_tls: i32,
) {
    unreachable_in_unit_tests("js_perry_smtp_set_server")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_set_auth(
    _draft: i64,
    _user_ptr: *const u8,
    _user_len: usize,
    _pass_ptr: *const u8,
    _pass_len: usize,
) {
    unreachable_in_unit_tests("js_perry_smtp_set_auth")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_set_from(_draft: i64, _ptr: *const u8, _len: usize) {
    unreachable_in_unit_tests("js_perry_smtp_set_from")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_add_recipient(_draft: i64, _ptr: *const u8, _len: usize) {
    unreachable_in_unit_tests("js_perry_smtp_add_recipient")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_set_message(
    _draft: i64,
    _id_ptr: *const u8,
    _id_len: usize,
    _body_ptr: *const u8,
    _body_len: usize,
) {
    unreachable_in_unit_tests("js_perry_smtp_set_message")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_submit(
    _draft: i64,
    _verify: i32,
    _ctx: usize,
    _done: extern "C" fn(usize, *const c_void),
) -> i32 {
    unreachable_in_unit_tests("js_perry_smtp_submit")
}

#[no_mangle]
pub extern "C" fn js_perry_smtp_cancel(_draft: i64) {
    unreachable_in_unit_tests("js_perry_smtp_cancel")
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_new() -> *mut Promise {
    unreachable_in_unit_tests("perry_ffi_promise_new")
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_bits(_promise: *mut Promise, _bits: u64) {
    unreachable_in_unit_tests("perry_ffi_promise_resolve_bits")
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_deferred(
    _promise: *mut Promise,
    _ctx: *mut c_void,
    _invoke: extern "C" fn(*mut c_void) -> u64,
) {
    unreachable_in_unit_tests("perry_ffi_promise_reject_deferred")
}

#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_deferred(
    _promise: *mut Promise,
    _ctx: *mut c_void,
    _invoke: extern "C" fn(*mut c_void) -> u64,
) {
    unreachable_in_unit_tests("perry_ffi_promise_resolve_deferred")
}
