// Unit-test binaries for `perry-ext-ws` do not link the host stdlib/runtime
// archive that normally provides the perry_ffi async bridge (the real symbols
// live in `perry-stdlib::perry_ffi_async`, only linked into the final user
// program). Provide synchronous, test-only shims for the `perry_ffi_*` async
// externs this crate references so `cargo test -p perry-ext-ws` links — same
// pattern as `perry-ext-http` / `perry-ext-net`.

use std::ffi::c_void;

#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking_with_reactor(
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void),
) {
    invoke(ctx);
}

// The server accept loop and the per-client IO pump reach this through
// `perry_ffi::async_runtime::spawn_async`. Running the future is not the point
// of these unit tests (they cover framing/masking and the clients-set surface),
// so the shim is a no-op rather than a runtime spin-up.
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_async(_ctx: *mut c_void) {}
