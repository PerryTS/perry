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
//!
//! Each stub funnels through `crate::stub_diag::perry_stub_warn` so a
//! call in runtime-only mode prints `[perry] warning: ...` once per
//! symbol per process — see issue #464 and `src/stub_diag.rs`.

use crate::stub_diag::perry_stub_warn;

#[cfg(not(any(
    target_os = "ios",
    target_os = "android",
    feature = "external-ws-symbols"
)))]
const WS_REASON: &str =
    "WebSocket symbol from perry-stdlib not linked into this binary (runtime-only build)";
const READLINE_REASON: &str =
    "readline symbol from perry-stdlib not linked into this binary (runtime-only build)";
const STDLIB_DISPATCH_REASON: &str =
    "stdlib dispatch symbol from perry-stdlib not linked into this binary (runtime-only build)";
#[cfg(not(feature = "external-fetch-symbols"))]
const FETCH_REASON: &str =
    "fetch symbol from perry-stdlib not linked into this binary (runtime-only build)";

// === WebSocket stubs ===
// On iOS, perry-stdlib provides the real WebSocket implementation (using
// NSURLSessionWebSocketTask). On Android, perry-ui-android provides a real
// WebSocket implementation using tungstenite+rustls. These stubs must NOT
// be compiled for either platform, otherwise the real implementations will
// be shadowed by the no-op stubs.
#[cfg(not(any(
    target_os = "ios",
    target_os = "android",
    feature = "external-ws-symbols"
)))]
mod ws_stubs {
    use super::{perry_stub_warn, WS_REASON};
    use crate::promise::Promise;
    use crate::string::StringHeader;
    use std::ptr;

    #[no_mangle]
    pub extern "C" fn js_ws_connect(_url_ptr: *const StringHeader) -> *mut Promise {
        perry_stub_warn("js_ws_connect", WS_REASON, None);
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_connect_start(_url_nanboxed: f64) -> f64 {
        perry_stub_warn("js_ws_connect_start", WS_REASON, None);
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_send(_handle: i64, _message_ptr: *const StringHeader) {
        perry_stub_warn("js_ws_send", WS_REASON, None);
    }

    #[no_mangle]
    pub extern "C" fn js_ws_close(_handle: i64) {
        perry_stub_warn("js_ws_close", WS_REASON, None);
    }

    #[no_mangle]
    pub extern "C" fn js_ws_is_open(_handle: i64) -> f64 {
        perry_stub_warn("js_ws_is_open", WS_REASON, None);
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_message_count(_handle: i64) -> f64 {
        perry_stub_warn("js_ws_message_count", WS_REASON, None);
        0.0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_receive(_handle: i64) -> *mut StringHeader {
        perry_stub_warn("js_ws_receive", WS_REASON, None);
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_wait_for_message(_handle: i64, _timeout_ms: f64) -> *mut Promise {
        perry_stub_warn("js_ws_wait_for_message", WS_REASON, None);
        ptr::null_mut()
    }

    #[no_mangle]
    pub extern "C" fn js_ws_on(
        _handle: i64,
        _event_name_ptr: *const StringHeader,
        _callback_ptr: i64,
    ) -> i64 {
        perry_stub_warn("js_ws_on", WS_REASON, None);
        0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_server_new(_opts_f64: f64) -> i64 {
        perry_stub_warn("js_ws_server_new", WS_REASON, None);
        0
    }

    #[no_mangle]
    pub extern "C" fn js_ws_server_close(_handle: i64) {
        perry_stub_warn("js_ws_server_close", WS_REASON, None);
    }

    #[no_mangle]
    pub extern "C" fn js_ws_process_pending() -> i32 {
        // Hot-loop: drained every event-loop tick, so a first-call
        // warning would be misleading even if the symbol IS the no-op
        // stub variant — skip the warning here.
        0
    }
}

// === Stdlib dispatch stubs ===
// On Android, perry-ui-android provides a real js_stdlib_process_pending
// that processes WebSocket promise resolves.
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_stdlib_process_pending() -> i32 {
    // Hot-loop drain — see js_ws_process_pending above. Silent stub.
    0
}

// === Cron scheduler stubs ===
// The generated event loop calls js_cron_timer_tick /
// js_cron_timer_has_pending unconditionally each iteration (cron jobs
// otherwise never fire — the CRON_TIMERS machinery existed but nothing
// drove it). Runtime-only links (no perry-stdlib) need these no-op
// fallbacks so the binary links; with perry-stdlib or perry-ext-cron
// linked, the real queue implementations win. Android's stdlib stubs
// cover that target independently.
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    // Hot-loop drain — silent stub, same as js_stdlib_process_pending.
    0
}

#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_cron_timer_has_pending() -> i32 {
    0
}

#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_stdlib_init_dispatch() {
    perry_stub_warn("js_stdlib_init_dispatch", STDLIB_DISPATCH_REASON, None);
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_fetch_with_options(
    _url_ptr: *const crate::string::StringHeader,
    _method_ptr: *const crate::string::StringHeader,
    _body_ptr: *const crate::string::StringHeader,
    _headers_json_ptr: *const crate::string::StringHeader,
) -> *mut crate::promise::Promise {
    perry_stub_warn("js_fetch_with_options", FETCH_REASON, None);
    std::ptr::null_mut()
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_blob_new(_parts: f64, _type_value: f64) -> f64 {
    perry_stub_warn("js_blob_new", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_headers_new() -> f64 {
    perry_stub_warn("js_headers_new", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_headers_init_from_value(_handle: f64, _init: f64) -> f64 {
    perry_stub_warn("js_headers_init_from_value", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_request_new(
    _url_ptr: *const crate::string::StringHeader,
    _method_ptr: *const crate::string::StringHeader,
    _body_ptr: *const crate::string::StringHeader,
    _headers_handle: f64,
    _referrer_ptr: *const crate::string::StringHeader,
    _referrer_policy_ptr: *const crate::string::StringHeader,
    _mode_ptr: *const crate::string::StringHeader,
    _credentials_ptr: *const crate::string::StringHeader,
    _cache_ptr: *const crate::string::StringHeader,
    _redirect_ptr: *const crate::string::StringHeader,
    _integrity_ptr: *const crate::string::StringHeader,
    _keepalive: f64,
    _duplex_ptr: *const crate::string::StringHeader,
    _signal: f64,
) -> f64 {
    perry_stub_warn("js_request_new", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_response_new(
    _body_ptr: *const crate::string::StringHeader,
    _status: f64,
    _status_text_ptr: *const crate::string::StringHeader,
    _headers_handle: f64,
) -> f64 {
    perry_stub_warn("js_response_new", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_response_static_json(
    _value: f64,
    _init_status: f64,
    _init_status_text_ptr: *const crate::string::StringHeader,
    _headers_handle: f64,
) -> f64 {
    perry_stub_warn("js_response_static_json", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_response_static_redirect(
    _url_ptr: *const crate::string::StringHeader,
    _status: f64,
) -> f64 {
    perry_stub_warn("js_response_static_redirect", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(not(feature = "external-fetch-symbols"))]
#[no_mangle]
pub extern "C" fn js_response_static_error() -> f64 {
    perry_stub_warn("js_response_static_error", FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// === readline (#347) stubs ===
// `process.stdin.setRawMode(...)` and `process.stdin.on(...)` always
// codegen direct extern calls to these symbols, even when the user's
// program doesn't `import 'readline'` and stdlib isn't linked. The
// stubs are no-ops so the program links cleanly; when stdlib IS
// linked, the real implementations from `perry-stdlib::readline`
// override these (linker picks stdlib over runtime). Android stdlib
// stubs cover those targets independently.
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_readline_set_raw_mode(_enabled: f64) -> f64 {
    perry_stub_warn("js_readline_set_raw_mode", READLINE_REASON, Some("#347"));
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_readline_stdin_on(_event_ptr: i64, _callback: i64) -> f64 {
    perry_stub_warn("js_readline_stdin_on", READLINE_REASON, Some("#347"));
    f64::from_bits(0x7FFC_0000_0000_0001)
}
#[cfg(not(target_os = "android"))]
#[no_mangle]
pub extern "C" fn js_readline_stdin_remove_listener(_event_ptr: i64, _callback: i64) -> f64 {
    perry_stub_warn(
        "js_readline_stdin_remove_listener",
        READLINE_REASON,
        Some("#3962"),
    );
    f64::from_bits(0x7FFC_0000_0000_0001)
}

macro_rules! readline_stdin_lifecycle_stub {
    ($name:ident) => {
        #[cfg(not(target_os = "android"))]
        #[no_mangle]
        pub extern "C" fn $name() -> f64 {
            perry_stub_warn(stringify!($name), READLINE_REASON, Some("#3962"));
            f64::from_bits(0x7FFC_0000_0000_0001)
        }
    };
}

readline_stdin_lifecycle_stub!(js_readline_stdin_pause);
readline_stdin_lifecycle_stub!(js_readline_stdin_resume);
readline_stdin_lifecycle_stub!(js_readline_stdin_unref);
readline_stdin_lifecycle_stub!(js_readline_stdin_ref);
readline_stdin_lifecycle_stub!(js_readline_stdin_destroy);
