use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static MENUS: RefCell<Vec<Retained<NSMenu>>> = RefCell::new(Vec::new());
    static MENU_ITEM_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static MENUBARS: RefCell<Vec<Retained<NSMenu>>> = RefCell::new(Vec::new());
    /// Pending user menu bar to install during app_run (set by menubar_attach).
    pub(crate) static PENDING_USER_MENUBAR: RefCell<Option<Retained<NSMenu>>> = RefCell::new(None);
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryMenuItemTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryMenuItemTarget"]
    #[ivars = PerryMenuItemTargetIvars]
    pub struct PerryMenuItemTarget;

    impl PerryMenuItemTarget {
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("menu callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                // Extract callback BEFORE calling it — the JS callback may re-enter
                // menuAddItem (e.g. building context menus), which needs borrow_mut().
                // Holding the borrow across js_closure_call0 would panic.
                let closure_f64 = MENU_ITEM_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(cf) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(cf) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryMenuItemTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryMenuItemTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a new context menu. Returns menu handle (1-based).
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let menu = NSMenu::new(mtm);
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        menus.push(menu);
        menus.len() as i64
    })
}

/// Get a menu by handle.
fn get_menu(handle: i64) -> Option<Retained<NSMenu>> {
    MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (handle - 1) as usize;
        menus.get(idx).cloned()
    })
}

/// Add an item to a menu with a title and callback.
pub fn add_item(menu_handle: i64, title_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr);
    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                Some(Sel::register(c"menuItemClicked:")),
                &empty_key,
            );

            let target = PerryMenuItemTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            MENU_ITEM_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            item.setTarget(Some(&target));
            std::mem::forget(target);

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                }
                perry_geisterhand_register(menu_handle, 5, 0, callback, title_ptr);
            }

            menu.addItem(&item);
        }
    }
}

/// Add an item to a menu with a title, callback, and keyboard shortcut.
pub fn add_item_with_shortcut(
    menu_handle: i64,
    title_ptr: *const u8,
    callback: f64,
    shortcut_ptr: *const u8,
) {
    let title = str_from_header(title_ptr);
    let shortcut_str = str_from_header(shortcut_ptr);
    let (key, flags) = parse_shortcut(shortcut_str);

    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let ns_key = NSString::from_str(&key);
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                Some(Sel::register(c"menuItemClicked:")),
                &ns_key,
            );
            item.setKeyEquivalentModifierMask(flags);

            let target = PerryMenuItemTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            MENU_ITEM_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            item.setTarget(Some(&target));
            std::mem::forget(target);

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register_with_shortcut(
                        h: i64,
                        wt: u8,
                        ck: u8,
                        cb: f64,
                        lbl: *const u8,
                        shortcut_ptr: *const u8,
                        shortcut_len: usize,
                    );
                }
                let shortcut_bytes = shortcut_str.as_bytes();
                perry_geisterhand_register_with_shortcut(
                    menu_handle,
                    5,
                    0,
                    callback,
                    title_ptr,
                    shortcut_bytes.as_ptr(),
                    shortcut_bytes.len(),
                );
            }

            menu.addItem(&item);
        }
    }
}

/// Remove all items from a menu.
pub fn clear(menu_handle: i64) {
    if let Some(menu) = get_menu(menu_handle) {
        menu.removeAllItems();
    }
}

/// Add a separator item to a menu.
pub fn add_separator(menu_handle: i64) {
    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let sep = NSMenuItem::separatorItem(mtm);
        menu.addItem(&sep);
    }
}

/// Add a submenu to a menu.
pub fn add_submenu(menu_handle: i64, title_ptr: *const u8, submenu_handle: i64) {
    let title = str_from_header(title_ptr);
    if let (Some(menu), Some(submenu)) = (get_menu(menu_handle), get_menu(submenu_handle)) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                None,
                &empty_key,
            );
            // Set the submenu's title to match
            submenu.setTitle(&ns_title);
            item.setSubmenu(Some(&submenu));
            menu.addItem(&item);
        }
    }
}

/// Create a new menu bar. Returns bar handle (1-based).
pub fn menubar_create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let menu = NSMenu::new(mtm);
    MENUBARS.with(|m| {
        let mut bars = m.borrow_mut();
        bars.push(menu);
        bars.len() as i64
    })
}

/// Get a menu bar by handle.
fn get_menubar(handle: i64) -> Option<Retained<NSMenu>> {
    MENUBARS.with(|m| {
        let bars = m.borrow();
        let idx = (handle - 1) as usize;
        bars.get(idx).cloned()
    })
}

/// Add a menu to the menu bar with a title.
pub fn menubar_add_menu(bar_handle: i64, title_ptr: *const u8, menu_handle: i64) {
    let title = str_from_header(title_ptr);
    if let (Some(bar), Some(menu)) = (get_menubar(bar_handle), get_menu(menu_handle)) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                None,
                &empty_key,
            );
            // Set the menu's title so it displays correctly in the bar
            menu.setTitle(&ns_title);
            item.setSubmenu(Some(&menu));
            bar.addItem(&item);
        }
    }
}

/// Attach a menu bar to the application, replacing the default menu bar.
/// Stores bar as pending so app_run() can install it instead of the default.
pub fn menubar_attach(bar_handle: i64) {
    if let Some(bar) = get_menubar(bar_handle) {
        PENDING_USER_MENUBAR.with(|p| {
            *p.borrow_mut() = Some(bar);
        });
    }
}

/// Add a menu item with a standard action selector and nil target.
/// Used for Edit menu items (Copy, Paste, Cut, Undo, Redo, Select All) so that
/// macOS routes them through the responder chain to the first responder.
pub fn add_standard_action(
    menu_handle: i64,
    title_ptr: *const u8,
    selector_ptr: *const u8,
    shortcut_ptr: *const u8,
) {
    let title = str_from_header(title_ptr);
    let selector_name = str_from_header(selector_ptr);
    let shortcut_str = str_from_header(shortcut_ptr);
    let (key, flags) = parse_shortcut(shortcut_str);

    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let ns_key = NSString::from_str(&key);
        // Create a CString for the selector name
        let sel_cstr = std::ffi::CString::new(selector_name).expect("invalid selector");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                Some(Sel::register(&sel_cstr)),
                &ns_key,
            );
            item.setKeyEquivalentModifierMask(flags);
            // target = nil → macOS sends action to first responder
            menu.addItem(&item);
        }
    }
}

/// Set a context menu on a widget. Right-click will show this menu.
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    if let (Some(view), Some(menu)) = (
        crate::widgets::get_widget(widget_handle),
        get_menu(menu_handle),
    ) {
        unsafe {
            let _: () = msg_send![&*view, setMenu: &*menu];
        }
    }
}

/// Parse a shortcut string like "Cmd+Shift+N" into (key, NSEventModifierFlags).
/// If only a key is given (e.g. "n"), defaults to Cmd modifier.
/// Uppercase single char (e.g. "S") means Cmd+Shift.
fn parse_shortcut(s: &str) -> (String, NSEventModifierFlags) {
    let mut flags = NSEventModifierFlags::empty();
    let parts: Vec<&str> = s.split('+').collect();
    let mut key = String::new();
    let mut has_explicit_modifier = false;

    for part in &parts {
        let trimmed = part.trim();
        match trimmed.to_lowercase().as_str() {
            "cmd" | "command" => {
                flags |= NSEventModifierFlags::Command;
                has_explicit_modifier = true;
            }
            "shift" => {
                flags |= NSEventModifierFlags::Shift;
                has_explicit_modifier = true;
            }
            "option" | "alt" => {
                flags |= NSEventModifierFlags::Option;
                has_explicit_modifier = true;
            }
            "ctrl" | "control" => {
                flags |= NSEventModifierFlags::Control;
                has_explicit_modifier = true;
            }
            _ => key = trimmed.to_string(),
        }
    }

    // If just a key with no explicit modifiers, default to Cmd
    // Uppercase single char (e.g. "S") means Cmd+Shift
    if !has_explicit_modifier && !key.is_empty() {
        flags |= NSEventModifierFlags::Command;
        if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if ch.is_ascii_uppercase() {
                flags |= NSEventModifierFlags::Shift;
                key = ch.to_lowercase().to_string();
            }
        }
    }

    (key.to_lowercase(), flags)
}
