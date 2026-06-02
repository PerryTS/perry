//! Focus, scroll-to, context menus, tray stubs, file-dialog, app
//! min/max size, textfield extras, and `widget_add_child_at`. Behavior
//! is unchanged from the pre-split `lib.rs`.

use super::*;

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(scroll_handle: i64, callback: f64) {
    widgets::scrollview::set_refresh_control(scroll_handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(scroll_handle: i64) {
    widgets::scrollview::end_refreshing(scroll_handle);
}

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
    menu_handle: i64,
    title_ptr: i64,
    shortcut_ptr: i64,
    callback: f64,
) {
    // Arg order matches the TS-side API: `menuAddItemWithShortcut(menu, title, shortcut, callback)`.
    menu::add_item_with_shortcut(
        menu_handle,
        title_ptr as *const u8,
        callback,
        shortcut_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

// =============================================================================
// Tray icon (issue #490) — no-op on visionOS (no system tray concept).
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

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on iOS — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on iOS — standard Edit menu actions are handled by UIResponder chain natively
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
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string_value(handle) as i64
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    widgets::textfield::set_on_submit(handle, on_submit);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64) {
    // TODO: implement iOS textfield focus observer
    let _ = (handle, on_focus);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    // TODO: implement iOS blur
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // iOS handles tab navigation automatically
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}
