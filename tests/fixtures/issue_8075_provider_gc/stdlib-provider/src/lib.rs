//! Minimal test wrapper for Perry's separately loaded stdlib provider.
//!
//! The custom final-link driver binds its runtime calls to the process-wide
//! runtime dylib before leaving the rlib available for Rust generic glue.

extern crate perry_stdlib;

unsafe extern "C" {
    fn js_gc_init();
}

#[used]
static PIN_STDLIB: extern "C" fn() -> i32 = perry_stdlib::common::js_stdlib_process_pending;

// A cdylib only retains Rust dependency code reachable from this wrapper.
// Keep the exact Web Fetch/Streams surface used by #8038's later-loaded app;
// otherwise the app links with dynamic lookups but dlopen fails on the first
// missing `js_fetch_*` symbol. This function is never called, so its sentinel
// arguments only create linker references to the provider implementations.
#[used]
static PIN_ISSUE_8038_RESPONSE_SURFACE: unsafe extern "C" fn() = pin_issue_8038_response_surface;

unsafe extern "C" fn pin_issue_8038_response_surface() {
    let _ = perry_stdlib::js_headers_new();
    let _ = perry_stdlib::js_headers_set(0.0, std::ptr::null(), std::ptr::null());
    let _ = perry_stdlib::js_headers_append(0.0, std::ptr::null(), std::ptr::null());
    let _ = perry_stdlib::js_headers_get(0.0, std::ptr::null());
    let _ = perry_stdlib::js_response_body_init_ptr(0.0);
    let _ = perry_stdlib::js_response_new(std::ptr::null(), 0.0, std::ptr::null(), 0.0);
    let _ = perry_stdlib::js_response_get_headers(0.0);
    let _ = perry_stdlib::js_fetch_response_status(0.0);
    let _ = perry_stdlib::js_fetch_response_status_text(0.0);
    let _ = perry_stdlib::js_response_body(0.0);
    let _ = perry_stdlib::js_readable_stream_new_from_source_object(0.0, 0.0);
    let _ = perry_stdlib::js_readable_stream_get_reader_with_options(0.0, 0.0);
    let _ = perry_stdlib::js_reader_read(0.0);
    perry_stdlib::js_stdlib_init_dispatch();
}

/// Proves that the stdlib resolves stateful runtime calls to the provider the
/// host loaded first, rather than embedding a second GC/runtime image.
#[no_mangle]
pub extern "C" fn issue_8075_stdlib_runtime_probe() -> usize {
    js_gc_init as *const () as usize
}
