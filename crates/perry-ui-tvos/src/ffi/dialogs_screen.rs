//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// QR Code
// =============================================================================

// QR Code — not available on tvOS (no camera for scanning)
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(_data_ptr: i64, _size: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(_handle: i64, _data_ptr: i64) {}

// =============================================================================
// Folder Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // iOS: UIDocumentPickerViewController for directories — stub for now
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Save File Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    _callback: f64,
    _default_name: i64,
    _allowed_types: i64,
) {
    // iOS: UIDocumentPickerViewController needed — stub for now
}

// =============================================================================
// Poll Open File (stub — iOS uses URL schemes / UIDocumentBrowser instead)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

// =============================================================================
// Overlay (stub — iOS uses different approach)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent_handle: i64, _child_handle: i64) {
    // Stub — iOS would use addSubview directly
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // Stub
}

// =============================================================================
// State TextField Binding
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Issue #708 — tvOS UIAlertController. Mirrors the iOS implementation
/// in `widgets::alert`; tvOS has UIAlertController with the same API.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) };
    widgets::alert::show(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet
// =============================================================================

// #1033: signature aligned with the perry-dispatch row
// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(_body: i64, _width: f64, _height: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}

// =============================================================================
// Screen Detection (iPad vs iPhone, orientation)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_screen_width() → logical width in points (e.g. 820 for iPad Air portrait)
#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        // UIScreen.bounds is orientation-aware since iOS 8
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.width
    }
}

/// perry_get_screen_height() → logical height in points
#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.height
    }
}

/// perry_get_scale_factor() → device pixel ratio (e.g. 2.0 for iPad, 3.0 for iPhone Pro)
#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let scale: f64 = objc2::msg_send![main_screen, scale];
        scale
    }
}

/// perry_get_orientation() → "landscape" or "portrait"
#[no_mangle]
pub extern "C" fn perry_get_orientation() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        if bounds.size.width > bounds.size.height {
            nanbox_static_str(b"landscape")
        } else {
            nanbox_static_str(b"portrait")
        }
    }
}

/// perry_get_device_idiom() → "tv" — this crate only ever runs on tvOS,
/// so the form factor is static. Returns a raw `*mut StringHeader` (i64);
/// the `ReturnKind::Str` dispatch row NaN-boxes it with STRING_TAG. (The
/// previous body was an iOS copy-paste that probed UIDevice.model for
/// iPad/iPhone and reported an Apple TV as the numeric phone code.)
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(b"tv".as_ptr(), 2) }
}
