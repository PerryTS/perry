use perry_ffi::Promise;

// Unit-test binaries do not link the host stdlib/runtime archive that normally
// provides the perry_ffi async bridge. Keep these test-only shims synchronous.

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
