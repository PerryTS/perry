//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// Phase A.2: ScrollView, Clipboard & Keyboard Shortcuts
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(_key: i64, _mods: f64, _cb: f64) {}

// Continuous keyboard events (issue #1864) — UIPress/UIKey impl.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_down(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_down(handle, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_up(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_up(handle, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_down(cb: f64) {
    crate::keyboard::set_on_key_down(0, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_up(cb: f64) {
    crate::keyboard::set_on_key_up(0, cb);
    crate::keyboard::make_first_responder();
}
#[no_mangle]
pub extern "C" fn perry_ui_focus_widget(handle: i64) {
    crate::keyboard::focus_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_blur_widget(handle: i64) {
    crate::keyboard::blur_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_is_key_down(code: f64) -> i32 {
    let raw = code as i32;
    if !(0..=u16::MAX as i32).contains(&raw) {
        return 0;
    }
    if crate::keyboard::is_key_down(raw as u16) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_current_modifiers() -> i32 {
    crate::keyboard::current_modifiers() as i32
}

#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 {
    0
}
