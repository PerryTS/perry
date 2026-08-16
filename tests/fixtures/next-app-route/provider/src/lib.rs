//! Provider image for the production Next App Route dylib gate.
//!
//! The final-link driver retains and exports the exact Perry ABI symbols the
//! compiled app leaves undefined. These anchors ensure Cargo places the stdlib
//! and HTTP wrapper rlibs on that final link in one coherent tokio build.

extern crate perry_ext_http;
extern crate perry_stdlib;

unsafe extern "C" {
    fn js_gc_init();
}

#[used]
static PIN_STDLIB: extern "C" fn() -> i32 = perry_stdlib::common::js_stdlib_process_pending;

#[used]
static PIN_HTTP: unsafe extern "C" fn() -> i32 = perry_ext_http::js_http_process_pending;

/// Proves that this provider binds stateful runtime calls to the runtime image
/// loaded by the host instead of satisfying them from a private runtime copy.
#[no_mangle]
pub extern "C" fn next_app_route_provider_runtime_probe() -> usize {
    js_gc_init as *const () as usize
}
