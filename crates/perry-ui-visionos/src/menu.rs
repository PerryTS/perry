//! iOS menu system — UIMenu / UIAction / UIKeyCommand for iPadOS menu bar and context menus.
//!
//! On iPadOS 26+, menus appear as a native menu bar. On iPhone / older iPad,
//! keyboard shortcuts still work via UIKeyCommand discoverability.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use std::cell::RefCell;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// A stored menu item — either a regular item, separator, or submenu.
#[derive(Clone)]
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

/// A menu bar: ordered list of (title, menu_handle).
#[derive(Clone)]
struct MenuBarEntry {
    menus: Vec<(String, i64)>,
}

thread_local! {
    static MENUS: RefCell<Vec<Vec<MenuItemEntry>>> = RefCell::new(Vec::new());
    static MENUBARS: RefCell<Vec<MenuBarEntry>> = RefCell::new(Vec::new());
    /// The attached menu bar handle (if any). Read by buildMenuWithBuilder:.
    pub(crate) static ATTACHED_MENUBAR: RefCell<Option<i64>> = RefCell::new(None);
    /// Global action counter for unique selector names.
    static NEXT_ACTION_ID: RefCell<usize> = RefCell::new(1);
    /// Map from action tag (usize) to callback f64.
    static ACTION_CALLBACKS: RefCell<Vec<(usize, f64)>> = RefCell::new(Vec::new());
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

// ─── Public API ──────────────────────────────────────────────────────────────

/// Create a new menu. Returns menu handle (1-based).
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
    #[cfg(feature = "geisterhand")]
    {
        extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
        unsafe { perry_geisterhand_register(menu_handle, 5, 0, callback, title_ptr); }
    }
}

/// Add an item with a keyboard shortcut (e.g. "Cmd+N").
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
                shortcut: Some(shortcut.clone()),
            });
        }
    });
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register_with_shortcut(
                h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8,
                shortcut_ptr: *const u8, shortcut_len: usize,
            );
        }
        let shortcut_bytes = shortcut.as_bytes();
        unsafe {
            perry_geisterhand_register_with_shortcut(
                menu_handle, 5, 0, callback, title_ptr,
                shortcut_bytes.as_ptr(), shortcut_bytes.len(),
            );
        }
    }
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
        bars.push(MenuBarEntry { menus: Vec::new() });
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
            bars[idx].menus.push((title, menu_handle));
        }
    });
}

/// Attach a menu bar. Stores handle and triggers UIMenuSystem rebuild.
pub fn menubar_attach(bar_handle: i64) {
    ATTACHED_MENUBAR.with(|a| {
        *a.borrow_mut() = Some(bar_handle);
    });

    // Tell UIKit to rebuild the menu bar
    unsafe {
        let menu_system_cls = AnyClass::get(c"UIMenuSystem");
        if let Some(cls) = menu_system_cls {
            let main_system: *mut AnyObject = msg_send![cls, mainSystem];
            if !main_system.is_null() {
                let _: () = msg_send![main_system, setNeedsRebuild];
            }
        }
    }
}

/// Set a context menu on a widget (stub — UIContextMenuInteraction not yet implemented).
pub fn set_context_menu(_widget_handle: i64, _menu_handle: i64) {
    // TODO: UIContextMenuInteraction
}

// ─── UIMenu Construction ─────────────────────────────────────────────────────

/// Build the UIMenu hierarchy for a menu handle.
/// Returns a Retained<AnyObject> pointing to a UIMenu, or None.
pub(crate) unsafe fn build_uimenu(menu_handle: i64) -> Option<Retained<AnyObject>> {
    let items: Vec<MenuItemEntry> = MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (menu_handle - 1) as usize;
        if idx < menus.len() {
            menus[idx].clone()
        } else {
            Vec::new()
        }
    });

    if items.is_empty() {
        return None;
    }

    // Build an NSMutableArray of UIMenuElement objects
    let arr_cls = AnyClass::get(c"NSMutableArray").unwrap();
    let children: Retained<AnyObject> = msg_send![arr_cls, new];

    // Group items by separator — each group becomes a section (UIMenu with .displayInline)
    let mut current_group: Vec<Retained<AnyObject>> = Vec::new();
    let mut groups: Vec<Vec<Retained<AnyObject>>> = Vec::new();

    for item in &items {
        match item {
            MenuItemEntry::Item {
                title,
                callback,
                shortcut,
            } => {
                let element = build_uiaction(title, *callback, shortcut.as_deref());
                current_group.push(element);
            }
            MenuItemEntry::Separator => {
                if !current_group.is_empty() {
                    groups.push(std::mem::take(&mut current_group));
                }
            }
            MenuItemEntry::Submenu {
                title,
                submenu_handle,
            } => {
                if let Some(sub) = build_uimenu(*submenu_handle) {
                    // Set title on the submenu
                    let ns_title = objc2_foundation::NSString::from_str(title);
                    let sub_with_title: Retained<AnyObject> = build_uimenu_with_title(&ns_title, &sub);
                    current_group.push(sub_with_title);
                }
            }
        }
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }

    // If there's only one group (no separators), put all items directly in children
    if groups.len() <= 1 {
        for group in &groups {
            for element in group {
                let _: () = msg_send![&*children, addObject: &**element];
            }
        }
    } else {
        // Multiple groups — wrap each in a displayInline UIMenu for separator rendering
        for group in &groups {
            let section_arr: Retained<AnyObject> = msg_send![arr_cls, new];
            for element in group {
                let _: () = msg_send![&*section_arr, addObject: &**element];
            }
            let empty_title = objc2_foundation::NSString::from_str("");
            let menu_cls = AnyClass::get(c"UIMenu").unwrap();
            // UIMenuOptions.displayInline = 1 << 0 = 1
            let section: Retained<AnyObject> = msg_send![
                menu_cls,
                menuWithTitle: &*empty_title,
                image: std::ptr::null::<AnyObject>(),
                identifier: std::ptr::null::<AnyObject>(),
                options: 1u64,
                children: &*section_arr
            ];
            let _: () = msg_send![&*children, addObject: &*section];
        }
    }

    // Create the top-level UIMenu (no title — the bar item title comes from menuBarAddMenu)
    let empty_title = objc2_foundation::NSString::from_str("");
    let menu_cls = AnyClass::get(c"UIMenu").unwrap();
    let menu: Retained<AnyObject> = msg_send![
        menu_cls,
        menuWithTitle: &*empty_title,
        children: &*children
    ];
    Some(menu)
}

/// Wrap a UIMenu with a new title (for submenus).
unsafe fn build_uimenu_with_title(
    title: &objc2_foundation::NSString,
    source_menu: &AnyObject,
) -> Retained<AnyObject> {
    // Get the children array from source_menu
    let source_children: Retained<AnyObject> = msg_send![source_menu, children];
    let menu_cls = AnyClass::get(c"UIMenu").unwrap();
    let menu: Retained<AnyObject> = msg_send![
        menu_cls,
        menuWithTitle: title,
        children: &*source_children
    ];
    menu
}

/// Build a UIAction for a menu item.
unsafe fn build_uiaction(
    title: &str,
    callback: f64,
    shortcut: Option<&str>,
) -> Retained<AnyObject> {
    let ns_title = objc2_foundation::NSString::from_str(title);

    // Allocate a unique tag for this action
    let tag = NEXT_ACTION_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    // Store callback
    ACTION_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().push((tag, callback));
    });

    if let Some(shortcut_str) = shortcut {
        // Create a UIKeyCommand with keyboard shortcut
        let (key, mods) = parse_shortcut(shortcut_str);
        let ns_key = objc2_foundation::NSString::from_str(&key);

        let cmd_cls = AnyClass::get(c"UIKeyCommand").unwrap();
        let sel = Sel::register(c"perryMenuAction:");
        let cmd: Retained<AnyObject> = msg_send![
            cmd_cls,
            keyCommandWithInput: &*ns_key,
            modifierFlags: mods,
            action: sel
        ];

        // Set title for menu bar display
        let _: () = msg_send![&*cmd, setTitle: &*ns_title];

        // Store the tag in the property number so we can look up the callback
        // Use the wantsPriorityOverSystemBehavior property tag approach —
        // actually we'll use the command's hash as key, but simpler: use a
        // global counter and store tag on the object via associated object or
        // by encoding in the property text. Simplest: store in discoverabilityTitle.
        let tag_str = format!("__perry_tag_{}", tag);
        let ns_tag = objc2_foundation::NSString::from_str(&tag_str);
        let _: () = msg_send![&*cmd, setDiscoverabilityTitle: &*ns_tag];

        cmd
    } else {
        // Create a UIAction (closure-based, no shortcut)
        // UIAction requires a block/closure — use UICommand with a selector instead
        let cmd_cls = AnyClass::get(c"UICommand").unwrap();
        let sel = Sel::register(c"perryMenuAction:");
        let cmd: Retained<AnyObject> = msg_send![
            cmd_cls,
            commandWithTitle: &*ns_title,
            image: std::ptr::null::<AnyObject>(),
            action: sel,
            propertyList: std::ptr::null::<AnyObject>()
        ];

        // Store tag in discoverabilityTitle for lookup
        let tag_str = format!("__perry_tag_{}", tag);
        let ns_tag = objc2_foundation::NSString::from_str(&tag_str);
        let _: () = msg_send![&*cmd, setDiscoverabilityTitle: &*ns_tag];

        cmd
    }
}

/// Called from the perryMenuAction: selector on the responder.
/// Extracts the tag from the sender's discoverabilityTitle and invokes the callback.
pub(crate) unsafe fn dispatch_menu_action(sender: *mut AnyObject) {
    if sender.is_null() {
        return;
    }
    let disc_title: *mut AnyObject = msg_send![sender, discoverabilityTitle];
    if disc_title.is_null() {
        return;
    }
    let ns_str: &objc2_foundation::NSString =
        &*(disc_title as *const objc2_foundation::NSString);
    let rust_str = ns_str.to_string();

    if let Some(tag_str) = rust_str.strip_prefix("__perry_tag_") {
        if let Ok(tag) = tag_str.parse::<usize>() {
            let callback = ACTION_CALLBACKS.with(|cbs| {
                let cbs = cbs.borrow();
                cbs.iter()
                    .find(|&&(t, _)| t == tag)
                    .map(|&(_, cb)| cb)
            });
            if let Some(cb) = callback {
                let ptr = js_nanbox_get_pointer(cb) as *const u8;
                js_closure_call0(ptr);
            }
        }
    }
}

/// Parse a shortcut string like "Cmd+Shift+N" into (key, UIKeyModifierFlags bitmask).
/// UIKeyModifierFlags: Command=1<<20, Shift=1<<17, Alternate(Option)=1<<19, Control=1<<18
fn parse_shortcut(s: &str) -> (String, u64) {
    let mut flags: u64 = 0;
    let mut key = String::new();

    for part in s.split('+') {
        let trimmed = part.trim();
        match trimmed.to_lowercase().as_str() {
            "cmd" | "command" => flags |= 1 << 20, // UIKeyModifierCommand
            "shift" => flags |= 1 << 17,           // UIKeyModifierShift
            "option" | "alt" => flags |= 1 << 19,  // UIKeyModifierAlternate
            "ctrl" | "control" => flags |= 1 << 18, // UIKeyModifierControl
            _ => key = trimmed.to_lowercase(),
        }
    }

    (key, flags)
}

/// Build the complete menu bar for the UIMenuBuilder.
/// Called from the `buildMenuWithBuilder:` override on PerryViewController.
pub(crate) unsafe fn build_menubar_for_builder(builder: *mut AnyObject) {
    let bar_handle = ATTACHED_MENUBAR.with(|a| *a.borrow());
    let bar_handle = match bar_handle {
        Some(h) => h,
        None => return,
    };

    let bar_menus: Vec<(String, i64)> = MENUBARS.with(|m| {
        let bars = m.borrow();
        let idx = (bar_handle - 1) as usize;
        if idx < bars.len() {
            bars[idx].menus.clone()
        } else {
            Vec::new()
        }
    });

    for (title, menu_handle) in bar_menus {
        if let Some(uimenu) = build_uimenu(menu_handle) {
            // Re-wrap with the correct title
            let ns_title = objc2_foundation::NSString::from_str(&title);
            let titled_menu = build_uimenu_with_title(&ns_title, &uimenu);

            // Insert at end of the main menu via UIMenuBuilder
            // builder.insertChild(_:atEndOfMenu:) with identifier .root
            let root_id = objc2_foundation::NSString::from_str("UIMenuRoot");
            let _: () = msg_send![
                builder,
                insertSiblingMenu: &*titled_menu,
                afterMenuForIdentifier: &*root_id
            ];
        }
    }
}
