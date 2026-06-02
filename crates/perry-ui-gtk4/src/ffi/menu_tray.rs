// FFI: Menu (context + bar) + Tray icon (issue #490, Linux-only body).
use crate::menu;
#[cfg(target_os = "linux")]
use crate::tray;

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
// Tray icon (issue #490) — KSNI / StatusNotifierItem on Linux.
// FFI symbols stay defined unconditionally so the link surface is
// stable; only the body branches on cfg.
// =============================================================================

/// Create a tray icon with an initial PNG path. Returns 0 on systems
/// without StatusNotifierItem support.
#[no_mangle]
pub extern "C" fn perry_ui_tray_create(icon_path_ptr: i64) -> i64 {
    #[cfg(target_os = "linux")]
    {
        return tray::create(icon_path_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = icon_path_ptr;
        eprintln!(
            "[perry] warning: tray icons require Linux + StatusNotifierItem \
            (KDE / GNOME-with-extension / XFCE) — gtk4 build on this host \
            doesn't support them (#490)"
        );
        0
    }
}

/// Hot-update the tray icon (no service re-creation).
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_icon(tray_handle: i64, icon_path_ptr: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::set_icon(tray_handle, icon_path_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, icon_path_ptr);
    }
}

/// Hot-update the tray tooltip.
#[no_mangle]
pub extern "C" fn perry_ui_tray_set_tooltip(tray_handle: i64, tooltip_ptr: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::set_tooltip(tray_handle, tooltip_ptr as *const u8);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, tooltip_ptr);
    }
}

/// Attach a menu (handle from `menuCreate` / `menuAddItem` / etc.) so
/// right-click pops it up. Re-uses the existing menu storage in
/// `menu.rs` rather than building a parallel system.
#[no_mangle]
pub extern "C" fn perry_ui_tray_attach_menu(tray_handle: i64, menu_handle: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::attach_menu(tray_handle, menu_handle);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, menu_handle);
    }
}

/// Register a JS click callback (NaN-boxed closure pointer). Fires on
/// SNI's primary `Activate` action — usually left-click on the tray
/// icon. Right-click pops the attached menu instead.
#[no_mangle]
pub extern "C" fn perry_ui_tray_on_click(tray_handle: i64, callback: f64) {
    #[cfg(target_os = "linux")]
    {
        tray::on_click(tray_handle, callback);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (tray_handle, callback);
    }
}

/// Destroy the tray icon. Subsequent calls on the same handle no-op.
#[no_mangle]
pub extern "C" fn perry_ui_tray_destroy(tray_handle: i64) {
    #[cfg(target_os = "linux")]
    {
        tray::destroy(tray_handle);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = tray_handle;
    }
}

/// Remove all items from a menu.
#[no_mangle]
pub extern "C" fn perry_ui_menu_clear(menu_handle: i64) {
    menu::clear(menu_handle);
}

/// Add a menu item with a standard action (no-op on GTK4 — macOS responder chain concept).
#[no_mangle]
pub extern "C" fn perry_ui_menu_add_standard_action(
    _menu_handle: i64,
    _title_ptr: i64,
    _selector_ptr: i64,
    _shortcut_ptr: i64,
) {
    // No-op on GTK4 — standard actions are handled by GtkTextView built-in bindings
}
