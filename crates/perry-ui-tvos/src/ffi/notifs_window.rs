//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// Notifications
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_notification_send(_title: i64, _body: i64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_register_remote(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_receive(_callback: f64) {}

/// Background-receive (#98) — no-op on tvOS. Apple TV doesn't deliver
/// silent push to backgrounded apps the way iOS does; the symbol exists so
/// cross-platform user code links cleanly.
#[no_mangle]
pub extern "C" fn perry_system_notification_on_background_receive(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_interval(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _seconds: f64,
    _repeats: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_calendar(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _timestamp_ms: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_schedule_location(
    _id_ptr: i64,
    _title_ptr: i64,
    _body_ptr: i64,
    _lat: f64,
    _lon: f64,
    _radius: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_system_notification_cancel(_id_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_system_notification_on_tap(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_system_get_locale() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe {
        let ns_locale: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSLocale").unwrap(),
            currentLocale
        ];
        let lang_code: *mut objc2::runtime::AnyObject = objc2::msg_send![ns_locale, languageCode];
        if lang_code.is_null() {
            let fallback = b"en";
            return js_string_from_bytes(fallback.as_ptr(), 2) as i64;
        }
        let utf8: *const u8 = objc2::msg_send![lang_code, UTF8String];
        let len = libc::strlen(utf8 as *const i8);
        let code_len = if len >= 2 { 2 } else { len };
        js_string_from_bytes(utf8, code_len as i64) as i64
    }
}

// =============================================================================
// Multi-Window
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(_title: i64, _width: f64, _height: f64) -> i64 {
    0 // stub — iOS uses UIScene for multi-window
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(_window: i64, _widget: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}

// =============================================================================
// LazyVStack
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(_count: i64, _render: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(_handle: i64, _count: i64) {}

// =============================================================================
// Table (stub — not yet implemented on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0 // stub
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_handle: i64, _col: i64, _title_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_handle: i64, _col: i64, _width: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_handle: i64) -> i64 {
    -1
}

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
