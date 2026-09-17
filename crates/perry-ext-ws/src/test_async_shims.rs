// Unit-test binaries for `perry-ext-ws` do not link the host stdlib/runtime
// archive that normally provides the perry_ffi async bridge (the real symbols
// live in `perry-stdlib::perry_ffi_async`, only linked into the final user
// program). Provide test-only shims for the `perry_ffi_*` async externs this
// crate references so `cargo test -p perry-ext-ws` links — same pattern as
// `perry-ext-http` / `perry-ext-net`.
//
// These are the native-async *token* symbols (`JsNativeAsyncCompletion`), which
// is what `js_ws_connect`'s promise is now held as: the connect settles from
// the completion sink, a different turn from the call that created it, and the
// token is the runtime's pinned, root-scanned handle rather than a bare
// `*mut Promise` parked across that gap.
//
// A null token is deliberate. These tests cover framing, masking, the URL
// parse, the upgrade ordering and the clients-set surface — none of them
// settles a promise — and a shim that pretended to allocate one would be a
// second, untested implementation of the runtime's token. A test that does
// need settlement belongs in an integration suite that links the real runtime.

use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_new(_flags: u32) -> *mut c_void {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_promise(_token: *mut c_void) -> *mut c_void {
    std::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_resolve_bits(_token: *mut c_void, _bits: u64) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_string(
    _token: *mut c_void,
    _message: *const u8,
    _len: usize,
) -> i32 {
    0
}
