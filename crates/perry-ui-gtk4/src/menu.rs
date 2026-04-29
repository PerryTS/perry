use gtk4::gio;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// Stored menu item — regular item, separator, or submenu.
enum MenuItemEntry {
    Item {
        title: String,
        callback: f64,
        shortcut: Option<String>,
    },
    Separator,
    Submenu {
        title: String,
        submenu_handle: i64,
    },
}

thread_local! {
    /// Menu entries: each is a list of MenuItemEntry
    static MENUS: RefCell<Vec<Vec<MenuItemEntry>>> = RefCell::new(Vec::new());
    /// Map from action name to closure pointer (f64 NaN-boxed)
    static MENU_ITEM_CALLBACKS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
    /// Counter for generating unique action names
    static NEXT_ACTION_ID: RefCell<usize> = RefCell::new(1);
    /// Menu bars: each is a list of (title, menu_handle)
    static MENUBARS: RefCell<Vec<Vec<(String, i64)>>> = RefCell::new(Vec::new());
    /// Pending menu bar to attach (set by menubar_attach, consumed during app activate)
    pub(crate) static PENDING_MENUBAR: RefCell<Option<i64>> = RefCell::new(None);
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Extract a &str from a *const StringHeader pointer.
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

/// Create a new context menu. Returns menu handle (1-based).
pub fn create() -> i64 {
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        menus.push(Vec::new());
        menus.len() as i64
    })
}

/// Add an item to a menu with a title and callback.
pub fn add_item(menu_handle: i64, title_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr).to_string();
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].push(MenuItemEntry::Item {
                title,
                callback,
                shortcut: None,
            });
        }
    });
}

/// Add an item with a keyboard shortcut.
pub fn add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: *const u8,
    callback: f64,
    shortcut_ptr: *const u8,
) {
    let title = str_from_header(title_ptr).to_string();
    let shortcut = str_from_header(shortcut_ptr).to_string();
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].push(MenuItemEntry::Item {
                title,
                callback,
                shortcut: Some(shortcut),
            });
        }
    });
}

/// Remove all items from a menu.
pub fn clear(menu_handle: i64) {
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].clear();
        }
    });
}

/// Add a separator to a menu.
pub fn add_separator(menu_handle: i64) {
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].push(MenuItemEntry::Separator);
        }
    });
}

/// Add a submenu to a menu.
pub fn add_submenu(menu_handle: i64, title_ptr: *const u8, submenu_handle: i64) {
    let title = str_from_header(title_ptr).to_string();
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].push(MenuItemEntry::Submenu {
                title,
                submenu_handle,
            });
        }
    });
}

/// Create a menu bar. Returns bar handle (1-based).
pub fn menubar_create() -> i64 {
    MENUBARS.with(|m| {
        let mut bars = m.borrow_mut();
        bars.push(Vec::new());
        bars.len() as i64
    })
}

/// Add a menu to the menu bar with a title.
pub fn menubar_add_menu(bar_handle: i64, title_ptr: *const u8, menu_handle: i64) {
    let title = str_from_header(title_ptr).to_string();
    MENUBARS.with(|m| {
        let mut bars = m.borrow_mut();
        let idx = (bar_handle - 1) as usize;
        if idx < bars.len() {
            bars[idx].push((title, menu_handle));
        }
    });
}

/// Attach a menu bar to the application. Stores it; actual attachment happens
/// when the GTK application activates (or immediately if already active).
pub fn menubar_attach(bar_handle: i64) {
    PENDING_MENUBAR.with(|p| {
        *p.borrow_mut() = Some(bar_handle);
    });

    // If the app is already running, attach immediately
    crate::app::GTK_APP.with(|ga| {
        if let Some(ref app) = *ga.borrow() {
            install_menubar_on_app(app, bar_handle);
        }
    });
}

/// Build a gio::Menu from a stored menu handle. Registers actions on the app.
fn build_gio_menu(menu_handle: i64, app: &gtk4::Application) -> gio::Menu {
    let gio_menu = gio::Menu::new();

    let items: Vec<_> = MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            // Clone the data we need without holding the borrow
            menus[idx]
                .iter()
                .map(|entry| match entry {
                    MenuItemEntry::Item {
                        title,
                        callback,
                        shortcut,
                    } => (0, title.clone(), *callback, shortcut.clone(), 0),
                    MenuItemEntry::Separator => (1, String::new(), 0.0, None, 0),
                    MenuItemEntry::Submenu {
                        title,
                        submenu_handle,
                    } => (2, title.clone(), 0.0, None, *submenu_handle),
                })
                .collect()
        } else {
            Vec::new()
        }
    });

    // GMenu uses sections for separators: items between separators go into sections
    let mut current_section = gio::Menu::new();

    for (kind, title, callback, shortcut, submenu_handle) in items {
        match kind {
            0 => {
                // Regular item
                let action_name = NEXT_ACTION_ID.with(|id| {
                    let mut id = id.borrow_mut();
                    let name = format!("app.menu{}", *id);
                    let short_name = format!("menu{}", *id);
                    *id += 1;
                    (name, short_name)
                });

                MENU_ITEM_CALLBACKS.with(|cbs| {
                    cbs.borrow_mut().insert(action_name.1.clone(), callback);
                });

                let action = gio::SimpleAction::new(&action_name.1, None);
                let action_name_clone = action_name.1.clone();
                action.connect_activate(move |_action, _param| {
                    let closure_f64 = MENU_ITEM_CALLBACKS
                        .with(|cbs| cbs.borrow().get(&action_name_clone).copied());
                    if let Some(closure_f64) = closure_f64 {
                        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                        unsafe {
                            js_closure_call0(closure_ptr as *const u8);
                        }
                    }
                });

                app.add_action(&action);

                let menu_item = gio::MenuItem::new(Some(&title), Some(&action_name.0));

                // Set accelerator if shortcut is provided
                if let Some(ref shortcut_str) = shortcut {
                    let accel = shortcut_to_gtk_accel(shortcut_str);
                    app.set_accels_for_action(&action_name.0, &[&accel]);
                }

                current_section.append_item(&menu_item);
            }
            1 => {
                // Separator — close the current section and start a new one
                gio_menu.append_section(None, &current_section);
                current_section = gio::Menu::new();
            }
            2 => {
                // Submenu
                let sub_gio = build_gio_menu(submenu_handle, app);
                current_section.append_submenu(Some(&title), &sub_gio);
            }
            _ => {}
        }
    }

    // Append the final section
    gio_menu.append_section(None, &current_section);
    gio_menu
}

/// Install a menu bar on a GTK Application.
pub fn install_menubar_on_app(app: &gtk4::Application, bar_handle: i64) {
    let bar_menus: Vec<(String, i64)> = MENUBARS.with(|m| {
        let bars = m.borrow();
        let idx = (bar_handle - 1) as usize;
        if idx < bars.len() {
            bars[idx].clone()
        } else {
            Vec::new()
        }
    });

    let menubar_model = gio::Menu::new();

    for (title, menu_handle) in bar_menus {
        let gio_menu = build_gio_menu(menu_handle, app);
        menubar_model.append_submenu(Some(&title), &gio_menu);
    }

    app.set_menubar(Some(&menubar_model));

    // Enable menubar on existing windows
    if let Some(window_list) = app.windows().first() {
        if let Ok(app_window) = window_list.clone().downcast::<gtk4::ApplicationWindow>() {
            app_window.set_show_menubar(true);
        }
    }
}

/// Convert a shortcut string like "Cmd+N" to GTK accelerator format "<Control>n".
fn shortcut_to_gtk_accel(s: &str) -> String {
    let parts: Vec<&str> = s.split('+').collect();
    let mut accel = String::new();
    let mut key = String::new();

    for part in &parts {
        let trimmed = part.trim();
        match trimmed.to_lowercase().as_str() {
            "cmd" | "command" | "ctrl" | "control" => accel.push_str("<Control>"),
            "shift" => accel.push_str("<Shift>"),
            "option" | "alt" => accel.push_str("<Alt>"),
            _ => key = trimmed.to_lowercase(),
        }
    }

    accel.push_str(&key);
    accel
}

/// Set a context menu on a widget. Right-click will show this menu.
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    if let Some(widget) = crate::widgets::get_widget(widget_handle) {
        // Build a GIO menu model from our stored menu items
        let gio_menu = gio::Menu::new();

        // Only use simple items for context menus (old path)
        let items: Vec<(String, f64)> = MENUS.with(|m| {
            let menus = m.borrow();
            let idx = (menu_handle - 1) as usize;
            if idx < menus.len() {
                menus[idx]
                    .iter()
                    .filter_map(|entry| {
                        if let MenuItemEntry::Item {
                            title, callback, ..
                        } = entry
                        {
                            Some((title.clone(), *callback))
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            }
        });

        // Create an action group on the widget
        let action_group = gio::SimpleActionGroup::new();

        for (title, callback) in items {
            let action_name = NEXT_ACTION_ID.with(|id| {
                let mut id = id.borrow_mut();
                let name = format!("ctx{}", *id);
                *id += 1;
                name
            });

            MENU_ITEM_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(action_name.clone(), callback);
            });

            let action = gio::SimpleAction::new(&action_name, None);
            let action_name_clone = action_name.clone();
            action.connect_activate(move |_action, _param| {
                let closure_f64 =
                    MENU_ITEM_CALLBACKS.with(|cbs| cbs.borrow().get(&action_name_clone).copied());
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            });

            action_group.add_action(&action);
            gio_menu.append(Some(&title), Some(&format!("ctx.{}", action_name)));
        }

        widget.insert_action_group("ctx", Some(&action_group));

        // Create a PopoverMenu from the GIO menu and attach it
        let popover = gtk4::PopoverMenu::from_model(Some(&gio_menu));
        popover.set_parent(&widget);
        popover.set_has_arrow(false);

        // Attach a right-click gesture controller
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3); // Right-click
        gesture.connect_pressed(move |gesture, _n_press, x, y| {
            let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
            popover.set_pointing_to(Some(&rect));
            popover.popup();
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        widget.add_controller(gesture);
    }
}
