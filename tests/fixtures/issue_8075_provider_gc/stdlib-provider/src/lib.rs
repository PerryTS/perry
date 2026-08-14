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

/// Proves that the stdlib resolves stateful runtime calls to the provider the
/// host loaded first, rather than embedding a second GC/runtime image.
#[no_mangle]
pub extern "C" fn issue_8075_stdlib_runtime_probe() -> usize {
    js_gc_init as *const () as usize
}
