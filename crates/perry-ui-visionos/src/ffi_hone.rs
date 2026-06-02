//! Hone-IDE specific FFI exports: documents directory and native iOS
//! WebSocket (bypasses tokio which doesn't work on iOS). Behavior is
//! unchanged from the pre-split `lib.rs`.

use super::*;

// =============================================================================
// iOS Documents directory (for persistent storage)
// =============================================================================

/// Returns the app's Documents directory path as a NaN-boxed string.
/// Used by hone-ide's paths.ts for persistent storage on iOS.
#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    unsafe {
        let file_manager: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSFileManager").unwrap(),
            defaultManager
        ];
        // NSDocumentDirectory = 9, NSUserDomainMask = 1
        let urls: objc2::rc::Retained<objc2_foundation::NSArray<objc2_foundation::NSURL>> =
            objc2::msg_send![file_manager, URLsForDirectory: 9u64, inDomains: 1u64];
        let count: usize = objc2::msg_send![&*urls, count];
        if count > 0 {
            let url: *const objc2::runtime::AnyObject =
                objc2::msg_send![&*urls, objectAtIndex: 0usize];
            let path: objc2::rc::Retained<objc2_foundation::NSString> = objc2::msg_send![url, path];
            let rust_str = path.to_string();
            let bytes = rust_str.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            // Return empty string
            let str_ptr = js_string_from_bytes(std::ptr::null(), 0);
            js_nanbox_string(str_ptr as i64)
        }
    }
}

/// Wrapper for Perry codegen (some declare functions use __wrapper_ prefix).
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    hone_get_documents_dir()
}

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
