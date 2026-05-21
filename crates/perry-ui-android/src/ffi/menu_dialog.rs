//! Context menus, file/folder dialogs, tray-icon stubs, window sizing,
//! app lifecycle, state bindings (new), text styling (new) and widget
//! creation (new). Originally `lib.rs` lines 565-790 (Phase A.5 through
//! the "Widget Creation (new)" section).

use crate::{app, file_dialog, menu, state, widgets};

// =============================================================================
// Phase A.5: Context Menus, File Dialog & Window Sizing
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(
    _menu_handle: i64,
    _title_ptr: i64,
    _shortcut_ptr: i64,
    _callback: f64,
) {
    // No-op on Android — no menu bar on mobile
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(_menu_handle: i64) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(
    _menu_handle: i64,
    _title_ptr: i64,
    _submenu_handle: i64,
) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    0 // Stub — no menu bar on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(_bar_handle: i64, _title_ptr: i64, _menu_handle: i64) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(_bar_handle: i64) {
    // No-op on Android
}

// =============================================================================
// Tray icon (issue #490) — no-op on Android (no system tray concept).
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tray_create(_icon_path_ptr: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(_tray_handle: i64, _icon_path_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(_tray_handle: i64, _tooltip_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(_tray_handle: i64, _menu_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(_tray_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(_tray_handle: i64) {}

/// Remove all items from a menu (no-op on Android).
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(_menu_handle: i64) {
    // No-op on Android
}

/// Add a menu item with a standard action (no-op on Android — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on Android
}

#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}

// =============================================================================
// App Lifecycle (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(callback: f64) {
    app::on_activate(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(callback: f64) {
    app::on_terminate(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(_app_handle: i64, interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

// =============================================================================
// State Bindings (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::on_change(state_handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Text Styling (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    widgets::text::set_font_family(handle, family_ptr as *const u8);
}
