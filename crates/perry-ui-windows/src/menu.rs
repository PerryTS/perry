//! Context menus and menu bars — CreatePopupMenu, CreateMenu, TrackPopupMenu, SetMenu

use std::cell::RefCell;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

struct MenuItem {
    id: u16,
    callback_ptr: *const u8,
}

struct MenuEntry {
    #[cfg(target_os = "windows")]
    hmenu: HMENU,
    #[cfg(not(target_os = "windows"))]
    hmenu: isize,
    items: Vec<MenuItem>,
}

// Maps widget handle -> menu handle for context menus
struct ContextMenuBinding {
    widget_handle: i64,
    menu_handle: i64,
}

struct MenuBarEntry {
    #[cfg(target_os = "windows")]
    hmenu: HMENU,
    #[cfg(not(target_os = "windows"))]
    hmenu: isize,
}

thread_local! {
    static MENUS: RefCell<Vec<MenuEntry>> = RefCell::new(Vec::new());
    static NEXT_MENU_ITEM_ID: RefCell<u16> = RefCell::new(40_000); // Start high to avoid conflicts
    static CONTEXT_MENU_BINDINGS: RefCell<Vec<ContextMenuBinding>> = RefCell::new(Vec::new());
    // Global map from menu item ID -> callback pointer
    static MENU_CALLBACKS: RefCell<Vec<(u16, *const u8)>> = RefCell::new(Vec::new());
    static MENUBARS: RefCell<Vec<MenuBarEntry>> = RefCell::new(Vec::new());
    /// Pending menubar handle to attach when window is created
    static PENDING_MENUBAR: RefCell<Option<i64>> = RefCell::new(None);
}

/// Create a context menu. Returns menu handle (1-based).
pub fn create() -> i64 {
    #[cfg(target_os = "windows")]
    {
        let hmenu = unsafe { CreatePopupMenu().unwrap() };
        MENUS.with(|menus| {
            let mut menus = menus.borrow_mut();
            menus.push(MenuEntry {
                hmenu,
                items: Vec::new(),
            });
            menus.len() as i64
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        MENUS.with(|menus| {
            let mut menus = menus.borrow_mut();
            menus.push(MenuEntry {
                hmenu: 0,
                items: Vec::new(),
            });
            menus.len() as i64
        })
    }
}

/// Add an item to a context menu with title and callback.
pub fn add_item(menu_handle: i64, title_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    let id = NEXT_MENU_ITEM_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    MENUS.with(|menus| {
        let mut menus = menus.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            #[cfg(target_os = "windows")]
            {
                let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                unsafe {
                    let _ = AppendMenuW(
                        menus[idx].hmenu,
                        MF_STRING,
                        id as usize,
                        windows::core::PCWSTR(wide.as_ptr()),
                    );
                }
            }
            menus[idx].items.push(MenuItem { id, callback_ptr });
        }
    });

    MENU_CALLBACKS.with(|cb| {
        cb.borrow_mut().push((id, callback_ptr));
    });

    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(
                handle: i64,
                widget_type: u8,
                callback_kind: u8,
                closure_f64: f64,
                label_ptr: *const u8,
            );
        }
        unsafe {
            perry_geisterhand_register(menu_handle, 5, 0, callback, title_ptr);
        }
    }
}

/// Add an item to a menu with title, callback, and keyboard shortcut.
/// Shortcut is formatted as tab-separated display text (e.g., "New\tCtrl+N").
pub fn add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: *const u8,
    callback: f64,
    shortcut_ptr: *const u8,
) {
    let title = str_from_header(title_ptr);
    let shortcut = str_from_header(shortcut_ptr);
    // Convert "Cmd+N" to "Ctrl+N" for Windows display
    let display_shortcut = shortcut.replace("Cmd", "Ctrl").replace("Option", "Alt");
    let display_title = format!("{}\t{}", title, display_shortcut);
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    let id = NEXT_MENU_ITEM_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    MENUS.with(|menus| {
        let mut menus = menus.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            #[cfg(target_os = "windows")]
            {
                let wide: Vec<u16> = display_title
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    let _ = AppendMenuW(
                        menus[idx].hmenu,
                        MF_STRING,
                        id as usize,
                        windows::core::PCWSTR(wide.as_ptr()),
                    );
                }
            }
            menus[idx].items.push(MenuItem { id, callback_ptr });
        }
    });

    MENU_CALLBACKS.with(|cb| {
        cb.borrow_mut().push((id, callback_ptr));
    });

    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(
                handle: i64,
                widget_type: u8,
                callback_kind: u8,
                closure_f64: f64,
                label_ptr: *const u8,
            );
        }
        unsafe {
            perry_geisterhand_register(menu_handle, 5, 0, callback, title_ptr);
        }
    }
}

/// Add a separator to a menu.
pub fn add_separator(menu_handle: i64) {
    MENUS.with(|menus| {
        let mut menus = menus.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            #[cfg(target_os = "windows")]
            {
                unsafe {
                    let _ = AppendMenuW(
                        menus[idx].hmenu,
                        MF_SEPARATOR,
                        0,
                        windows::core::PCWSTR::null(),
                    );
                }
            }
        }
    });
}

/// Add a submenu to a menu.
pub fn add_submenu(menu_handle: i64, title_ptr: *const u8, submenu_handle: i64) {
    let title = str_from_header(title_ptr);
    MENUS.with(|menus| {
        let menus = menus.borrow();
        let idx = (menu_handle - 1) as usize;
        let sub_idx = (submenu_handle - 1) as usize;
        if idx < menus.len() && sub_idx < menus.len() {
            #[cfg(target_os = "windows")]
            {
                let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
                unsafe {
                    let _ = AppendMenuW(
                        menus[idx].hmenu,
                        MF_POPUP,
                        menus[sub_idx].hmenu.0 as usize,
                        windows::core::PCWSTR(wide.as_ptr()),
                    );
                }
            }
        }
    });
}

/// Create a menu bar. Returns bar handle (1-based).
pub fn menubar_create() -> i64 {
    #[cfg(target_os = "windows")]
    {
        let hmenu = unsafe { CreateMenu().unwrap() };
        MENUBARS.with(|bars| {
            let mut bars = bars.borrow_mut();
            bars.push(MenuBarEntry { hmenu });
            bars.len() as i64
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        MENUBARS.with(|bars| {
            let mut bars = bars.borrow_mut();
            bars.push(MenuBarEntry { hmenu: 0 });
            bars.len() as i64
        })
    }
}

/// Add a menu to the menu bar with a title.
pub fn menubar_add_menu(bar_handle: i64, title_ptr: *const u8, menu_handle: i64) {
    let title = str_from_header(title_ptr);
    MENUBARS.with(|bars| {
        let bars = bars.borrow();
        let bar_idx = (bar_handle - 1) as usize;
        if bar_idx < bars.len() {
            MENUS.with(|menus| {
                let menus = menus.borrow();
                let menu_idx = (menu_handle - 1) as usize;
                if menu_idx < menus.len() {
                    #[cfg(target_os = "windows")]
                    {
                        let wide: Vec<u16> =
                            title.encode_utf16().chain(std::iter::once(0)).collect();
                        unsafe {
                            let _ = AppendMenuW(
                                bars[bar_idx].hmenu,
                                MF_POPUP,
                                menus[menu_idx].hmenu.0 as usize,
                                windows::core::PCWSTR(wide.as_ptr()),
                            );
                        }
                    }
                }
            });
        }
    });
}

/// Attach a menu bar to the main application window.
/// If the window doesn't exist yet, stores the handle as pending.
pub fn menubar_attach(bar_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = crate::app::get_main_hwnd() {
            attach_menubar_to_hwnd(bar_handle, hwnd);
        } else {
            // Window not created yet — store for later
            PENDING_MENUBAR.with(|p| *p.borrow_mut() = Some(bar_handle));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = bar_handle;
    }
}

/// Actually attach a menubar HMENU to a window HWND.
#[cfg(target_os = "windows")]
fn attach_menubar_to_hwnd(bar_handle: i64, hwnd: HWND) {
    MENUBARS.with(|bars| {
        let bars = bars.borrow();
        let bar_idx = (bar_handle - 1) as usize;
        if bar_idx < bars.len() {
            unsafe {
                let _ = SetMenu(hwnd, bars[bar_idx].hmenu);
                let _ = DrawMenuBar(hwnd);
            }
        }
    });
}

/// Called from app_create after the window is created — attaches any pending menu bar.
#[cfg(target_os = "windows")]
pub fn attach_pending_menubar(hwnd: HWND) {
    let pending = PENDING_MENUBAR.with(|p| p.borrow_mut().take());
    if let Some(bar_handle) = pending {
        attach_menubar_to_hwnd(bar_handle, hwnd);
    }
}

/// Set a context menu on a widget (right-click menu).
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    CONTEXT_MENU_BINDINGS.with(|bindings| {
        bindings.borrow_mut().push(ContextMenuBinding {
            widget_handle,
            menu_handle,
        });
    });
}

/// Handle WM_CONTEXTMENU — find the menu bound to the widget and show it.
#[cfg(target_os = "windows")]
pub fn handle_context_menu(parent_hwnd: HWND, child_hwnd: HWND, x: i32, y: i32) {
    // Find which widget this HWND belongs to
    let widget_handle = crate::widgets::find_handle_by_hwnd(child_hwnd);
    if widget_handle == 0 {
        return;
    }

    // Find menu binding for this widget
    let menu_handle = CONTEXT_MENU_BINDINGS.with(|bindings| {
        let bindings = bindings.borrow();
        for b in bindings.iter() {
            if b.widget_handle == widget_handle {
                return b.menu_handle;
            }
        }
        0i64
    });

    if menu_handle == 0 {
        return;
    }

    MENUS.with(|menus| {
        let menus = menus.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            unsafe {
                let result = TrackPopupMenu(
                    menus[idx].hmenu,
                    TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN,
                    x,
                    y,
                    0,
                    parent_hwnd,
                    None,
                );
                if result.as_bool() {
                    let selected_id = result.0 as u16;
                    dispatch_menu_item(selected_id);
                }
            }
        }
    });
}

#[cfg(not(target_os = "windows"))]
pub fn handle_context_menu(_parent_hwnd: isize, _child_hwnd: isize, _x: i32, _y: i32) {}

/// Remove all items from a menu.
pub fn clear(menu_handle: i64) {
    // Collect item IDs before clearing so we can remove their callbacks
    let item_ids: Vec<u16> = MENUS.with(|menus| {
        let mut menus = menus.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            let ids: Vec<u16> = menus[idx].items.iter().map(|i| i.id).collect();
            #[cfg(target_os = "windows")]
            {
                let hmenu = menus[idx].hmenu;
                unsafe {
                    while GetMenuItemCount(hmenu) > 0 {
                        let _ = RemoveMenu(hmenu, 0, MF_BYPOSITION);
                    }
                }
            }
            menus[idx].items.clear();
            ids
        } else {
            Vec::new()
        }
    });
    if !item_ids.is_empty() {
        MENU_CALLBACKS.with(|cb| {
            cb.borrow_mut().retain(|&(id, _)| !item_ids.contains(&id));
        });
    }
}

/// Dispatch a menu item click to its callback.
pub fn dispatch_menu_item(id: u16) {
    // Extract callback BEFORE calling it — the JS callback may re-enter
    // menuAddItem (e.g. building context menus), which needs borrow_mut().
    // Holding the borrow across js_closure_call0 would panic.
    let callback_ptr = MENU_CALLBACKS.with(|cb| {
        let callbacks = cb.borrow();
        for &(item_id, ptr) in callbacks.iter() {
            if item_id == id {
                return Some(ptr);
            }
        }
        None
    });
    if let Some(ptr) = callback_ptr {
        unsafe { js_closure_call0(ptr) };
    }
}
