//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// Native iOS WebSocket (bypasses tokio which doesn't work on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn hone_ws_connect(url_ptr: i64) -> f64 {
    // Log to file for debugging (Perry GUI apps don't show stderr)
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hone-ws-debug.log")
    {
        let _ = writeln!(f, "hone_ws_connect called, url_ptr={}", url_ptr);
        let ptr = url_ptr as *const u8;
        if !ptr.is_null() && url_ptr > 0x1000 {
            let header = ptr as *const perry_runtime::string::StringHeader;
            unsafe {
                let len = (*header).byte_len as usize;
                let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
                if let Ok(s) = std::str::from_utf8(std::slice::from_raw_parts(data, len.min(200))) {
                    let _ = writeln!(f, "  url_str={}", s);
                }
            }
        }
    }
    websocket::connect(url_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_connect(url_nanboxed: f64) -> f64 {
    // Wrapper called with f64 NaN-boxed string — extract pointer
    let ptr = perry_runtime::js_get_string_pointer_unified(url_nanboxed);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hone-ws-debug.log")
    {
        use std::io::Write;
        let _ = writeln!(
            f,
            "__wrapper_hone_ws_connect called, nanboxed={}, extracted_ptr={}",
            url_nanboxed, ptr
        );
    }
    hone_ws_connect(ptr)
}

#[no_mangle]
pub extern "C" fn hone_ws_is_open(handle: f64) -> f64 {
    websocket::is_open(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_is_open(handle: f64) -> f64 {
    websocket::is_open(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_send(handle: f64, msg_ptr: i64) {
    websocket::send(handle, msg_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_send(handle: f64, msg_nanboxed: f64) {
    let ptr = perry_runtime::js_get_string_pointer_unified(msg_nanboxed);
    hone_ws_send(handle, ptr)
}

#[no_mangle]
pub extern "C" fn hone_ws_receive(handle: f64) -> f64 {
    websocket::receive(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_receive(handle: f64) -> f64 {
    websocket::receive(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_message_count(handle: f64) -> f64 {
    websocket::message_count(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_message_count(handle: f64) -> f64 {
    websocket::message_count(handle)
}

#[no_mangle]
pub extern "C" fn hone_ws_close(handle: f64) {
    websocket::close(handle)
}
#[no_mangle]
pub extern "C" fn __wrapper_hone_ws_close(handle: f64) {
    websocket::close(handle)
}

// --- Cross-platform toast + reactive setText stubs (Phase 2 v3.3) ---
// Full GTK4 implementation in perry-ui-gtk4. Present here so cross-platform
// code that calls showToast / setText links on tvOS targets.

#[no_mangle]
pub extern "C" fn perry_ui_show_toast(_msg_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_text_create_with_id(text_ptr: i64, _id_ptr: i64) -> i64 {
    crate::ffi::core_widgets::perry_ui_text_create(text_ptr)
}

#[no_mangle]
pub extern "C" fn perry_ui_set_text(_id_ptr: i64, _value_ptr: i64) {}
