// FFI: context menus, menu bar, tray icons (#490).
use crate::{menu, tray};

// =============================================================================
// Menu
// =============================================================================

/// Create a context menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

/// Add an item to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

/// Add a menu item with a keyboard shortcut.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: i64,
    shortcut_ptr: i64,
    callback: f64,
) {
    // Arg order matches the TS-side API: `menuAddItemWithShortcut(menu, title, shortcut, callback)`.
    // Why: on Win64 ABI int and float positional slots share register indices —
    // `(i64, i64, f64, i64)` vs caller's `(i64, i64, i64, f64)` would put `callback`
    // in XMM2 (uninitialized) and `shortcut_ptr` in R9 (also uninitialized), causing
    // a deref-garbage ACCESS_VIOLATION inside `str_from_header(shortcut_ptr)`.
    menu::add_item_with_shortcut(
        menu_handle,
        title_ptr as *const u8,
        callback,
        shortcut_ptr as *const u8,
    );
}

/// Add a separator to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

/// Add a submenu to a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

/// Create a menu bar. Returns bar handle.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

/// Add a menu to a menu bar with a title.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

/// Attach a menu bar to the application.
#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

// =============================================================================
// Tray icon (issue #490)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_tray_create(icon_path_ptr: i64) -> i64 {
    tray::create(icon_path_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(tray_handle: i64, icon_path_ptr: i64) {
    tray::set_icon(tray_handle, icon_path_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(tray_handle: i64, tooltip_ptr: i64) {
    tray::set_tooltip(tray_handle, tooltip_ptr as *const u8);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(tray_handle: i64, menu_handle: i64) {
    tray::attach_menu(tray_handle, menu_handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(tray_handle: i64, callback: f64) {
    tray::on_click(tray_handle, callback);
}
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(tray_handle: i64) {
    tray::destroy(tray_handle);
}

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on Windows — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on Windows — standard actions (copy/paste/undo) are handled by
    // the system via WM_COMMAND and accelerator tables, not ObjC selectors.
}
