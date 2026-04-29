//! No-op stubs for stdlib functions that may be referenced by compiled code
//! but are only available when perry-stdlib is linked.
//!
//! These stubs allow binaries to link in runtime-only mode even when the source
//! code references stdlib features (e.g., WebSocket). The stubs return safe
//! default values (null pointers, 0.0, etc.) so the program links and runs,
//! though the stdlib features will be non-functional.
//!
//! When perry-stdlib IS linked, its real implementations are used instead
//! (the linker picks stdlib over runtime since only one is ever linked).

use crate::promise::Promise;
use crate::string::StringHeader;
use std::ptr;

// === WebSocket stubs ===
// On iOS, perry-stdlib provides the real WebSocket implementation (using
// NSURLSessionWebSocketTask). On Android, perry-ui-android provides a real
// WebSocket implementation using tungstenite+rustls. These stubs must NOT
// be compiled for either platform, otherwise the real implementations will
// be shadowed by the no-op stubs.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod ws_stubs {
    use crate::promise::Promise;
    use crate::string::StringHeader;
    use std::ptr;

    #[no_mangle]
    pub extern "C" fn js_ws_connect(_url_ptr: *const StringHeader) -> *mut Promise {
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_connect_start(_url_nanboxed: f64) -> f64 {
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_send(_handle: i64, _message_ptr: *const StringHeader) {}

    #[no_mangle]
    pub extern "C" fn js_ws_close(_handle: i64) {}

    #[no_mangle]
    pub extern "C" fn js_ws_is_open(_handle: i64) -> f64 {
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_message_count(_handle: i64) -> f64 {
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_receive(_handle: i64) -> *mut StringHeader {
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_wait_for_message(_handle: i64, _timeout_ms: f64) -> *mut Promise {
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_on(
        _handle: i64,
        _event_name_ptr: *const StringHeader,
        _callback_ptr: i64,
    ) -> i64 {
        0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_server_new(_opts_f64: f64) -> i64 {
        0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_server_close(_handle: i64) {}

    #[no_mangle]
    pub extern "C" fn js_ws_process_pending() -> i32 {
        0
    }
}

// === Stdlib dispatch stubs ===
// On Android, perry-ui-android provides a real js_stdlib_process_pending
// that processes WebSocket promise resolves.
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_stdlib_process_pending() -> i32 {
    0
}

#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_stdlib_init_dispatch() {}
