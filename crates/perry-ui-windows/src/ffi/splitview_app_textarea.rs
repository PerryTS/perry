// FFI: SplitView/VBox stubs, app icon/level/transparency, file-open polling,
// TextArea (Win32 EDIT control with ES_MULTILINE | WS_VSCROLL).
use crate::{app, widgets};

// =============================================================================
// Splitview / VBox stubs (iOS-only layout containers)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_splitview_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(_handle: i64, _child: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_create(_spacing: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(_handle: i64, _child: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_handle: i64, _child: i64) {}

// =============================================================================
// App icon & file open polling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(app_handle: i64, value: f64) {
    app::app_set_frameless(app_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(app_handle: i64, value_ptr: i64) {
    app::app_set_level(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(app_handle: i64, value: f64) {
    app::app_set_transparent(app_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(app_handle: i64, value_ptr: i64) {
    app::app_set_vibrancy(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(app_handle: i64, value_ptr: i64) {
    app::app_set_activation_policy(app_handle, value_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    0
}

// =============================================================================
// TextArea — Win32 EDIT control with ES_MULTILINE | WS_VSCROLL
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textarea_create(on_change: f64) -> i64 {
    widgets::textarea::create(on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_set_string(handle: i64, text_ptr: i64) {
    widgets::textarea::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_textarea_get_string(handle: i64) -> i64 {
    widgets::textarea::get_string(handle)
}
