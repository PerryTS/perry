use crate::ffi::class_addMethod;
use crate::ffi::sel_registerName;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventModifierFlags,
    NSImage, NSMenu, NSMenuItem, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};

use std::cell::RefCell;
use std::collections::HashMap;

use crate::widgets;

thread_local! {
    pub(crate) static APPS: RefCell<Vec<AppEntry>> = const { RefCell::new(Vec::new()) };
    /// Buffered keyboard shortcuts registered before the menu bar exists.
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = const { RefCell::new(Vec::new()) };
}

struct PendingShortcut {
    key_ptr: *const u8,
    modifiers: f64,
    callback: f64,
}

thread_local! {
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    pub(crate) static WINDOWS: RefCell<Vec<WindowEntry>> = const { RefCell::new(Vec::new()) };
    static PENDING_ICON_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Files requested to be opened via macOS Open With / double-click.
    static PENDING_OPEN_FILES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Pending activation policy: "regular", "accessory", or "background".
    static PENDING_ACTIVATION_POLICY: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Whether the window needs rounded corners (set by frameless, applied in app_run).
    static PENDING_ROUNDED_CORNERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) struct WindowEntry {
    pub(crate) window: Retained<NSWindow>,
}

pub(crate) struct AppEntry {
    pub(crate) window: Retained<NSWindow>,
    pub(crate) _root_widget: Option<i64>,
    /// Issue #1280 — initial window state applied on `app_run`. `zoom:` /
    /// `toggleFullScreen:` need the window to be key+ordered front first.
    pub(crate) window_state: Option<WindowState>,
}

/// Issue #1280 — initial window state for the main app window.
#[derive(Copy, Clone)]
pub(crate) enum WindowState {
    Maximized,
    Fullscreen,
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

/// Create an app with title, width, height.
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App"
    } else {
        str_from_header(title_ptr)
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let frame = CGRect::new(CGPoint::new(200.0, 200.0), CGSize::new(w, h));

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );

        let ns_title = NSString::from_str(title);
        window.setTitle(&ns_title);

        // Match window appearance to the system setting so native controls
        // (NSTextField, NSPopUpButton, etc.) use the correct light/dark theme.
        // isDarkMode() is called here (after NSApp exists) to get the correct value.
        let is_dark = super::perry_system_is_dark_mode() != 0;
        let appearance_name = if is_dark {
            NSString::from_str("NSAppearanceNameDarkAqua")
        } else {
            NSString::from_str("NSAppearanceNameAqua")
        };
        let appearance_cls = objc2::runtime::AnyClass::get(c"NSAppearance").unwrap();
        let appearance: *mut objc2::runtime::AnyObject = objc2::msg_send![
            appearance_cls, appearanceNamed: &*appearance_name
        ];
        if !appearance.is_null() {
            let _: () = objc2::msg_send![&*window, setAppearance: appearance];
        }

        APPS.with(|a| {
            let mut apps = a.borrow_mut();
            apps.push(AppEntry {
                window,
                _root_widget: None,
                window_state: None,
            });
            apps.len() as i64 // 1-based handle
        })
    }
}

/// Set the root widget (body) of the app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx]._root_widget = Some(root_handle);

            if let Some(view) = widgets::get_widget(root_handle) {
                let window = &apps[idx].window;

                // Check if the current content view is an NSVisualEffectView (set by vibrancy).
                // If so, add the body as a subview of it instead of replacing it.
                let has_vibrancy = unsafe {
                    let content_view: *const objc2::runtime::AnyObject = msg_send![window, contentView];
                    if content_view.is_null() {
                        false
                    } else {
                        let cls = objc2::runtime::AnyClass::get(c"NSVisualEffectView");
                        match cls {
                            Some(effect_cls) => {
                                let is_effect: bool = msg_send![content_view, isKindOfClass: effect_cls];
                                is_effect
                            }
                            None => false,
                        }
                    }
                };

                if has_vibrancy {
                    // Add body as subview of the existing NSVisualEffectView
                    unsafe {
                        let content_view: *const objc2::runtime::AnyObject = msg_send![window, contentView];
                        let _: () = msg_send![content_view, addSubview: &*view];

                        // Pin body to fill the effect view using Auto Layout
                        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
                        let top_anchor = view.topAnchor();
                        let bottom_anchor = view.bottomAnchor();
                        let leading_anchor = view.leadingAnchor();
                        let trailing_anchor = view.trailingAnchor();

                        let parent_top: Retained<AnyObject> = msg_send![content_view, topAnchor];
                        let parent_bottom: Retained<AnyObject> = msg_send![content_view, bottomAnchor];
                        let parent_leading: Retained<AnyObject> = msg_send![content_view, leadingAnchor];
                        let parent_trailing: Retained<AnyObject> = msg_send![content_view, trailingAnchor];

                        let c_top: Retained<AnyObject> = msg_send![&*top_anchor, constraintEqualToAnchor: &*parent_top];
                        let c_bottom: Retained<AnyObject> = msg_send![&*bottom_anchor, constraintEqualToAnchor: &*parent_bottom];
                        let c_leading: Retained<AnyObject> = msg_send![&*leading_anchor, constraintEqualToAnchor: &*parent_leading];
                        let c_trailing: Retained<AnyObject> = msg_send![&*trailing_anchor, constraintEqualToAnchor: &*parent_trailing];

                        let _: () = msg_send![&*c_top, setActive: true];
                        let _: () = msg_send![&*c_bottom, setActive: true];
                        let _: () = msg_send![&*c_leading, setActive: true];
                        let _: () = msg_send![&*c_trailing, setActive: true];
                    }
                } else {
                    // Normal path: set body as the content view
                    window.setContentView(Some(&view));

                    // Pin the body view to the window's contentLayoutGuide using Auto Layout.
                    // contentLayoutGuide accounts for the title bar, so content starts below it.
                    unsafe {
                        let _: () = objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
                        let guide: Retained<AnyObject> = msg_send![window, contentLayoutGuide];
                        let guide_top: Retained<AnyObject> = msg_send![&*guide, topAnchor];
                        let guide_bottom: Retained<AnyObject> = msg_send![&*guide, bottomAnchor];
                        let guide_leading: Retained<AnyObject> = msg_send![&*guide, leadingAnchor];
                        let guide_trailing: Retained<AnyObject> = msg_send![&*guide, trailingAnchor];

                        let top_anchor = view.topAnchor();
                        let bottom_anchor = view.bottomAnchor();
                        let leading_anchor = view.leadingAnchor();
                        let trailing_anchor = view.trailingAnchor();

                        let c_top: Retained<AnyObject> = msg_send![&*top_anchor, constraintEqualToAnchor: &*guide_top];
                        let c_bottom: Retained<AnyObject> = msg_send![&*bottom_anchor, constraintEqualToAnchor: &*guide_bottom];
                        let c_leading: Retained<AnyObject> = msg_send![&*leading_anchor, constraintEqualToAnchor: &*guide_leading];
                        let c_trailing: Retained<AnyObject> = msg_send![&*trailing_anchor, constraintEqualToAnchor: &*guide_trailing];

                        let _: () = msg_send![&*c_top, setActive: true];
                        let _: () = msg_send![&*c_bottom, setActive: true];
                        let _: () = msg_send![&*c_leading, setActive: true];
                        let _: () = msg_send![&*c_trailing, setActive: true];
                    }
                }
            }
        }
    });
}

/// Create a standard macOS menu bar with Quit (Cmd+Q) support,
/// OR install the user-provided menu bar from menuBarAttach().
fn setup_menu_bar(app: &NSApplication, mtm: MainThreadMarker) {
    // Check if the user already attached a custom menu bar via menuBarAttach()
    let user_bar = crate::menu::PENDING_USER_MENUBAR.with(|p| p.borrow_mut().take());

    let has_user_bar = user_bar.is_some();

    unsafe {
        let menu_bar = if let Some(bar) = user_bar {
            // User provided a menu bar — prepend the app menu (with Quit) as the first item
            let app_menu_item = NSMenuItem::new(mtm);
            let app_menu = NSMenu::new(mtm);

            let quit_title = NSString::from_str("Quit");
            let quit_key = NSString::from_str("q");
            let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &quit_title,
                Some(Sel::register(c"terminate:")),
                &quit_key,
            );
            quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            app_menu.addItem(&quit_item);
            app_menu_item.setSubmenu(Some(&app_menu));

            // Insert app menu at position 0 (before user menus)
            bar.insertItem_atIndex(&app_menu_item, 0);
            bar
        } else {
            // No user menu bar — create default with just Quit
            let menu_bar = NSMenu::new(mtm);

            let app_menu_item = NSMenuItem::new(mtm);
            let app_menu = NSMenu::new(mtm);

            let quit_title = NSString::from_str("Quit");
            let quit_key = NSString::from_str("q");
            let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &quit_title,
                Some(Sel::register(c"terminate:")),
                &quit_key,
            );
            quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            app_menu.addItem(&quit_item);

            app_menu_item.setSubmenu(Some(&app_menu));
            menu_bar.addItem(&app_menu_item);
            menu_bar
        };

        // Only add the default Edit menu when the user hasn't provided a custom menu bar.
        // User-provided menus should include their own Edit menu with responder-chain actions.
        if !has_user_bar {
            let edit_menu_item = NSMenuItem::new(mtm);
            let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));

            let undo_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Undo"),
                Some(Sel::register(c"undo:")),
                &NSString::from_str("z"),
            );
            undo_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&undo_item);

            let redo_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Redo"),
                Some(Sel::register(c"redo:")),
                &NSString::from_str("z"),
            );
            redo_item.setKeyEquivalentModifierMask(
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
            );
            edit_menu.addItem(&redo_item);

            edit_menu.addItem(&NSMenuItem::separatorItem(mtm));

            let cut_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Cut"),
                Some(Sel::register(c"cut:")),
                &NSString::from_str("x"),
            );
            cut_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&cut_item);

            let copy_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Copy"),
                Some(Sel::register(c"copy:")),
                &NSString::from_str("c"),
            );
            copy_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&copy_item);

            let paste_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Paste"),
                Some(Sel::register(c"paste:")),
                &NSString::from_str("v"),
            );
            paste_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&paste_item);

            let select_all_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Select All"),
                Some(Sel::register(c"selectAll:")),
                &NSString::from_str("a"),
            );
            select_all_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&select_all_item);

            edit_menu_item.setSubmenu(Some(&edit_menu));
            menu_bar.addItem(&edit_menu_item);
        }

        // Always add a Window menu with a "Show Main Window" item so users can
        // re-open the main window after closing it (required by App Store guidelines).
        {
            let window_menu_item = NSMenuItem::new(mtm);
            let window_menu =
                NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Window"));

            let minimize_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Minimize"),
                Some(Sel::register(c"performMiniaturize:")),
                &NSString::from_str("m"),
            );
            minimize_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            window_menu.addItem(&minimize_item);

            let zoom_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Zoom"),
                Some(Sel::register(c"performZoom:")),
                &NSString::from_str(""),
            );
            window_menu.addItem(&zoom_item);

            window_menu.addItem(&NSMenuItem::separatorItem(mtm));

            let show_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Show Main Window"),
                Some(Sel::register(c"perryShowMainWindow:")),
                &NSString::from_str(""),
            );
            // Target is the delegate so our custom action fires
            show_item.setTarget(None);
            window_menu.addItem(&show_item);

            window_menu_item.setSubmenu(Some(&window_menu));
            menu_bar.addItem(&window_menu_item);

            // Tell NSApplication about the Window menu so macOS manages it
            app.setWindowsMenu(Some(&window_menu));
        }

        app.setMainMenu(Some(&menu_bar));
    }
}

/// Run the application event loop (blocks).
pub fn app_run(_app_handle: i64) {
    // Install crash reporting hooks before anything else
    crate::crash_log::install_crash_hooks();

    // #6184 (2026-07-09 GC audit): install a libdispatch memory-pressure
    // source on the main queue so the runtime can collect under OS memory
    // pressure (WARN → minor, CRITICAL → full collect). See memory_pressure.rs.
    crate::memory_pressure::install();

    // Phase 2 v3.3: register cross-platform showToast / setText handlers.
    // These are no-ops on harmonyos (where the ArkUI drain-queue path
    // owns the dispatch) but on macOS they wire `perry_arkts_show_toast`
    // and `perry_arkts_set_text` to our native NSPanel toast presenter
    // and NSTextField setStringValue: respectively.
    register_cross_platform_text_handlers();

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let app = NSApplication::sharedApplication(mtm);

    // Apply activation policy (default: Regular)
    let policy = PENDING_ACTIVATION_POLICY.with(|p| p.borrow().clone());
    let activation_policy = match policy.as_deref() {
        Some("accessory") => NSApplicationActivationPolicy::Accessory,
        Some("background") => NSApplicationActivationPolicy::Prohibited,
        _ => NSApplicationActivationPolicy::Regular,
    };
    app.setActivationPolicy(activation_policy);

    // Menu-bar / background apps (accessory / background activation policy)
    // don't auto-present their main window at launch — they're driven by a
    // status-item / tray and open windows on demand. Regular apps keep the
    // existing behaviour of showing the window immediately.
    let suppress_window = matches!(policy.as_deref(), Some("accessory") | Some("background"));

    // Apply pending dock icon
    PENDING_ICON_PATH.with(|p| {
        if let Some(path) = p.borrow().as_ref() {
            unsafe {
                let ns_path = NSString::from_str(path);
                let image: Option<Retained<NSImage>> = msg_send![
                    NSImage::alloc(), initWithContentsOfFile: &*ns_path
                ];
                if let Some(img) = image {
                    let _: () = msg_send![&*app, setApplicationIconImage: &*img];
                }
            }
        }
    });

    // Set up menu bar with Cmd+Q support
    setup_menu_bar(&app, mtm);

    // Install any keyboard shortcuts that were registered before the menu existed
    flush_pending_shortcuts(mtm);

    APPS.with(|a| {
        let apps = a.borrow();
        for entry in apps.iter() {
            entry.window.center();

            // Validate window is on a visible screen — if the position was
            // restored from a previous session with a different display setup,
            // the window could be completely off-screen.
            unsafe {
                let frame: CGRect = msg_send![&*entry.window, frame];
                let screens: Retained<AnyObject> = msg_send![objc2::class!(NSScreen), screens];
                let screen_count: usize = msg_send![&*screens, count];

                let mut on_screen = false;
                for i in 0..screen_count {
                    let screen: *const AnyObject = msg_send![&*screens, objectAtIndex: i];
                    let visible_frame: CGRect = msg_send![screen, visibleFrame];
                    if frame.origin.x >= visible_frame.origin.x - frame.size.width * 0.5
                        && frame.origin.x < visible_frame.origin.x + visible_frame.size.width
                        && frame.origin.y >= visible_frame.origin.y - frame.size.height * 0.5
                        && frame.origin.y < visible_frame.origin.y + visible_frame.size.height
                    {
                        on_screen = true;
                        break;
                    }
                }

                if !on_screen {
                    entry.window.center();
                }
            }

            // Apply deferred rounded corners on the final content view (after vibrancy/body setup)
            if PENDING_ROUNDED_CORNERS.with(|c| c.get()) {
                unsafe {
                    let content_view: *mut AnyObject = msg_send![&*entry.window, contentView];
                    if !content_view.is_null() {
                        let _: () = msg_send![content_view, setWantsLayer: true];
                        let layer: *mut AnyObject = msg_send![content_view, layer];
                        if !layer.is_null() {
                            let _: () = msg_send![layer, setCornerRadius: 12.0_f64];
                            let _: () = msg_send![layer, setMasksToBounds: true];
                        }
                    }
                }
            }

            // Accessory/background apps stay window-less at launch (the tray
            // owns presentation). Skip ordering the window front.
            if suppress_window {
                continue;
            }

            entry.window.makeKeyAndOrderFront(None);

            // Issue #1280 — apply initial window state. zoom: is the AppKit
            // "green button" maximize (respects dock + menu bar). The
            // toggleFullScreen: path enters native fullscreen on its own
            // Space. Both need the window to be key+ordered front, which is
            // why this runs here rather than in the setter.
            if let Some(state) = entry.window_state {
                unsafe {
                    match state {
                        WindowState::Maximized => {
                            let _: () =
                                msg_send![&*entry.window, zoom: std::ptr::null::<AnyObject>()];
                        }
                        WindowState::Fullscreen => {
                            let style_mask: usize = msg_send![&*entry.window, styleMask];
                            // Native fullscreen needs Resizable in the style
                            // mask; transient borderless / non-resizable
                            // windows can't enter it cleanly.
                            let resizable = NSWindowStyleMask::Resizable.0 as usize;
                            if style_mask & resizable != 0 {
                                let _: () = msg_send![
                                    &*entry.window,
                                    toggleFullScreen: std::ptr::null::<AnyObject>()
                                ];
                            } else {
                                // Fallback: zoom() to maximize within the screen.
                                let _: () = msg_send![
                                    &*entry.window,
                                    zoom: std::ptr::null::<AnyObject>()
                                ];
                            }
                        }
                    }
                }
            }
        }
    });

    // Set up app delegate for file open events
    unsafe {
        let delegate = PerryAppDelegate::new();
        let _: () = msg_send![&*app, setDelegate: &*delegate];
        // Issue #583: register the kAEGetURL handler so legacy URL-launch
        // pathways (older Spotlight, AppleScript `open location`) reach the
        // same dispatch as the AppKit application:openURLs: surface.
        crate::deeplinks::install_apple_event_handler(&*delegate as *const _ as *const AnyObject);
        std::mem::forget(delegate);
    }

    // Also check process argv for files passed on command line
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        PENDING_OPEN_FILES.with(|files| {
            let mut files = files.borrow_mut();
            for arg in args.iter().skip(1) {
                if !arg.starts_with('-') {
                    files.push(arg.clone());
                }
            }
        });
    }

    // Activate the app (bring to front)
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    // Set up a recurring timer pump to drive setTimeout/setInterval callbacks.
    // Without this, TypeScript timer-based loops (event polling, animation) never fire.
    // ~8ms interval (120Hz) ensures responsive callback delivery.
    unsafe {
        let target = PerryPumpTarget::new();
        let sel = Sel::register(c"pump:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.008f64,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];
        // Keep the target alive for the duration of the app
        std::mem::forget(target);
    }

    // Register UI function pointers for geisterhand dispatch
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register_state_set(f: extern "C" fn(i64, f64));
            fn perry_geisterhand_register_screenshot_capture(
                f: extern "C" fn(*mut usize) -> *mut u8,
            );
            fn perry_geisterhand_register_textfield_set_string(f: extern "C" fn(i64, i64));
            fn perry_geisterhand_register_scroll_set(f: extern "C" fn(i64, f64, f64));
            fn perry_geisterhand_register_read_value(f: extern "C" fn(i64, *mut usize) -> *mut u8);
            fn perry_geisterhand_register_query_tree(f: extern "C" fn(*mut usize) -> *mut u8);
            fn perry_geisterhand_register_apply_style(
                f: extern "C" fn(i64, u32, f64, f64, f64, f64),
            );
        }
        unsafe {
            perry_geisterhand_register_state_set(crate::perry_ui_state_set);
            perry_geisterhand_register_screenshot_capture(
                crate::screenshot::perry_ui_screenshot_capture,
            );
            perry_geisterhand_register_textfield_set_string(crate::perry_ui_textfield_set_string);
            perry_geisterhand_register_scroll_set(
                crate::widgets::scrollview::perry_ui_scroll_set_offset,
            );
            perry_geisterhand_register_read_value(crate::widgets::perry_ui_read_widget_value);
            perry_geisterhand_register_query_tree(crate::widgets::perry_ui_query_widget_tree);
            perry_geisterhand_register_apply_style(crate::geisterhand_style::apply_style);
        }
    }

    install_test_mode_exit_timer();

    app.run();
}

/// If `PERRY_UI_TEST_MODE=1`, schedule an NSTimer that captures a screenshot
/// (when `PERRY_UI_SCREENSHOT_PATH` is set) and exits the process cleanly.
/// This lets doc-example programs be verified in CI without a human.
fn install_test_mode_exit_timer() {
    if !perry_ui_testkit::is_test_mode() {
        return;
    }
    let delay_secs = perry_ui_testkit::exit_delay_ms() as f64 / 1000.0;
    unsafe {
        let target = PerryTestExitTarget::new();
        let sel = Sel::register(c"testExit:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: delay_secs,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: false
        ];
        std::mem::forget(target);
    }
}

pub struct PerryTestExitTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTestExitTarget"]
    #[ivars = PerryTestExitTargetIvars]
    pub struct PerryTestExitTarget;

    impl PerryTestExitTarget {
        #[unsafe(method(testExit:))]
        fn test_exit(&self, _sender: &AnyObject) {
            if let Some(path) = perry_ui_testkit::screenshot_path() {
                let mut len: usize = 0;
                let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
                if !ptr.is_null() && len > 0 {
                    perry_ui_testkit::write_screenshot_bytes(&path, ptr, len);
                    unsafe { libc::free(ptr as *mut libc::c_void); }
                }
            }
            // process::exit bypasses applicationWillTerminate:, so fire
            // the user's onTerminate hook manually to mirror a real quit.
            invoke_terminate_callback();
            use std::io::Write;
            let _ = std::io::stdout().flush();
            let _ = std::io::stderr().flush();
            std::process::exit(0);
        }
    }
);

impl PerryTestExitTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTestExitTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Set the minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window.setMinSize(CGSize::new(w, h));
        }
    });
}

/// Set the maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window.setMaxSize(CGSize::new(w, h));
        }
    });
}

/// Issue #1280 — record the requested initial window state. Applied in
/// `app_run` after the window is key+ordered front (zoom: / toggleFullScreen:
/// don't take effect on a window that hasn't been shown yet).
/// `value_ptr` is a StringHeader pointer to one of
/// "normal" | "maximized" | "fullscreen". Anything else is silently ignored.
pub fn set_window_state(app_handle: i64, value_ptr: *const u8) {
    let state_str = str_from_header(value_ptr);
    let state = match state_str {
        "maximized" => Some(WindowState::Maximized),
        "fullscreen" => Some(WindowState::Fullscreen),
        _ => None,
    };
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window_state = state;
        }
    });
}

/// Resize the main app window dynamically.
pub fn app_set_size(app_handle: i64, width: f64, height: f64) {
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            let window = &apps[idx].window;
            unsafe {
                let frame: CGRect = msg_send![window, frame];
                let new_frame = CGRect::new(frame.origin, CGSize::new(width, height));
                let _: () = msg_send![window, setFrame: new_frame, display: true, animate: true];
            }
        }
    });
}

// ============================================
// Keyboard Shortcuts
// ============================================

thread_local! {
    static SHORTCUT_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryShortcutTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryShortcutTarget"]
    #[ivars = PerryShortcutTargetIvars]
    pub struct PerryShortcutTarget;

    impl PerryShortcutTarget {
        #[unsafe(method(shortcutFired:))]
        fn shortcut_fired(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("shortcut callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = SHORTCUT_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryShortcutTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryShortcutTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set the application dock icon from a file path.
/// Stores the path; the icon is applied in app_run after activation policy is set.
pub fn app_set_icon(path_ptr: *const u8) {
    let path = str_from_header(path_ptr);
    if !path.is_empty() {
        PENDING_ICON_PATH.with(|p| {
            *p.borrow_mut() = Some(path.to_string());
        });
    }
}

/// Set frameless window mode (no titlebar).
/// `value` is a NaN-boxed boolean — TAG_TRUE = 0x7FFC_0000_0000_0004.
pub fn app_set_frameless(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            let window = &apps[idx].window;
            unsafe {
                // Remove all style masks for a borderless window
                let _: () = msg_send![window, setStyleMask: NSWindowStyleMask::Borderless.0];
                // Allow dragging by the window background
                let _: () = msg_send![window, setMovableByWindowBackground: true];
                // Borderless NSWindows don't become key by default.
                // Force the window to accept key status so text fields work.
                // Use raw ObjC runtime C calls to create a subclass.
                extern "C" {
                    fn objc_allocateClassPair(
                        superclass: *const std::ffi::c_void,
                        name: *const i8,
                        extra: usize,
                    ) -> *mut std::ffi::c_void;
                    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
                    fn object_setClass(
                        obj: *mut std::ffi::c_void,
                        cls: *mut std::ffi::c_void,
                    ) -> *mut std::ffi::c_void;
                    fn object_getClass(obj: *const std::ffi::c_void) -> *mut std::ffi::c_void;
                }
                extern "C" fn can_become_key(
                    _this: *mut std::ffi::c_void,
                    _sel: *mut std::ffi::c_void,
                ) -> i8 {
                    1
                }
                let window_ptr = &**window as *const NSWindow as *mut std::ffi::c_void;
                let parent_class = object_getClass(window_ptr);
                let subclass_name =
                    std::ffi::CString::new(format!("PerryKeyableWindow_{}", app_handle)).unwrap();
                let existing = objc2::runtime::AnyClass::get(&subclass_name);
                let new_class = if existing.is_some() {
                    existing.unwrap() as *const _ as *mut std::ffi::c_void
                } else {
                    let cls = objc_allocateClassPair(parent_class, subclass_name.as_ptr(), 0);
                    if !cls.is_null() {
                        let sel = sel_registerName(c"canBecomeKeyWindow".as_ptr());
                        class_addMethod(
                            cls,
                            sel,
                            can_become_key as *const std::ffi::c_void,
                            c"B@:".as_ptr(),
                        );
                        objc_registerClassPair(cls);
                    }
                    cls
                };
                if !new_class.is_null() {
                    object_setClass(window_ptr, new_class);
                    let _: () =
                        msg_send![window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
                }
                // Make window transparent so rounded corners show through
                let _: () = msg_send![window, setOpaque: false];
                let clear_color: *const AnyObject = msg_send![objc2::class!(NSColor), clearColor];
                let _: () = msg_send![window, setBackgroundColor: clear_color];
                let _: () = msg_send![window, setHasShadow: true];
                // Defer rounded corners to app_run, after vibrancy/body are set up
                PENDING_ROUNDED_CORNERS.with(|c| c.set(true));
            }
        }
    });
}

/// Set window level: "floating", "statusBar", "modal", or "normal".
pub fn app_set_level(app_handle: i64, value_ptr: *const u8) {
    let level_str = str_from_header(value_ptr);
    if level_str.is_empty() {
        return;
    }
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            let window = &apps[idx].window;
            unsafe {
                // NSWindowLevel values:
                // normal = 0, floating = 3, statusBar = 25, modalPanel = 8
                let level: isize = match level_str {
                    "floating" => 3,   // NSFloatingWindowLevel
                    "statusBar" => 25, // NSStatusWindowLevel
                    "modal" => 8,      // NSModalPanelWindowLevel
                    _ => 0,            // NSNormalWindowLevel
                };
                let _: () = msg_send![window, setLevel: level];
            }
        }
    });
}

/// Set window transparency (clear background).
/// `value` is a NaN-boxed boolean.
pub fn app_set_transparent(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            let window = &apps[idx].window;
            unsafe {
                let _: () = msg_send![window, setOpaque: false];
                // NSColor.clearColor
                let clear_color: *const objc2::runtime::AnyObject =
                    msg_send![objc2::class!(NSColor), clearColor];
                let _: () = msg_send![window, setBackgroundColor: clear_color];
            }
        }
    });
}

/// Set vibrancy material: "sidebar", "headerView", "sheet", "titlebar",
/// "tooltip", "underWindowBackground", "contentBackground", "behindWindow",
/// "menu", "popover", "selection".
///
/// Called BEFORE app_set_body: sets an NSVisualEffectView as the window's
/// content view. app_set_body then adds the body widget as a subview of it.
pub fn app_set_vibrancy(app_handle: i64, value_ptr: *const u8) {
    let material_str = str_from_header(value_ptr);
    if material_str.is_empty() {
        return;
    }
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            let window = &apps[idx].window;
            unsafe {
                // NSVisualEffectMaterial values
                let material: isize = match material_str {
                    "titlebar" => 3,
                    "selection" => 4,
                    "menu" => 5,
                    "popover" => 6,
                    "sidebar" => 7,
                    "headerView" => 10,
                    "sheet" => 11,
                    "windowBackground" => 12,
                    "hudWindow" => 13,
                    "fullScreenUI" => 15,
                    "tooltip" => 17,
                    "contentBackground" => 18,
                    "underWindowBackground" => 21,
                    "underPageBackground" => 22,
                    _ => 7, // default to sidebar
                };

                // Make window transparent so vibrancy shows through
                let _: () = msg_send![window, setOpaque: false];
                let clear_color: *const objc2::runtime::AnyObject =
                    msg_send![objc2::class!(NSColor), clearColor];
                let _: () = msg_send![window, setBackgroundColor: clear_color];

                // Create NSVisualEffectView sized to the window
                let effect_cls = objc2::runtime::AnyClass::get(c"NSVisualEffectView").unwrap();
                let effect_view: *mut objc2::runtime::AnyObject = msg_send![effect_cls, alloc];
                let frame = CGRect::new(
                    CGPoint::new(0.0, 0.0),
                    CGSize::new(window.frame().size.width, window.frame().size.height),
                );
                let effect_view: *mut objc2::runtime::AnyObject = msg_send![
                    effect_view, initWithFrame: frame
                ];
                let _: () = msg_send![effect_view, setMaterial: material];
                // NSVisualEffectBlendingMode.behindWindow = 0
                let _: () = msg_send![effect_view, setBlendingMode: 0isize];
                // NSVisualEffectState.active = 1 (always show vibrancy)
                let _: () = msg_send![effect_view, setState: 1isize];
                // Auto-resize with window
                // NSViewWidthSizable | NSViewHeightSizable = 0x12 = 18
                let _: () = msg_send![effect_view, setAutoresizingMask: 18u64];

                // Set the effect view as the window's content view.
                // app_set_body (called next) will add the body widget as a subview of this.
                let _: () = msg_send![window, setContentView: effect_view];
            }
        }
    });
}

/// Set the activation policy: "regular", "accessory", or "background".
/// Stored and applied in app_run() since NSApp policy must be set before the event loop starts.
pub fn app_set_activation_policy(_app_handle: i64, value_ptr: *const u8) {
    let policy_str = str_from_header(value_ptr);
    if !policy_str.is_empty() {
        PENDING_ACTIVATION_POLICY.with(|p| {
            *p.borrow_mut() = Some(policy_str.to_string());
        });
    }
}

/// Register a system-wide global hotkey that fires even when the app is in the background.
/// Also registers a local event monitor for when the app is focused.
/// `key_ptr` is a StringHeader pointer to the key character (e.g., "q").
/// `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
/// `callback` is a NaN-boxed closure pointer.
pub fn register_global_hotkey(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let key_str = str_from_header(key_ptr);
    if key_str.is_empty() {
        return;
    }

    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as usize;
    let mod_bits = modifiers as u64;
    let target_key = key_str.to_lowercase();

    // Map Perry modifier bits to NSEventModifierFlags raw values
    // Cmd = 1<<20, Shift = 1<<17, Option = 1<<19, Control = 1<<18
    let mut ns_mods: u64 = 0;
    if mod_bits & 1 != 0 {
        ns_mods |= 1 << 20;
    } // Cmd
    if mod_bits & 2 != 0 {
        ns_mods |= 1 << 17;
    } // Shift
    if mod_bits & 4 != 0 {
        ns_mods |= 1 << 19;
    } // Option
    if mod_bits & 8 != 0 {
        ns_mods |= 1 << 18;
    } // Control

    unsafe {
        // NSEventMask for keyDown = 1 << 10
        let key_down_mask: u64 = 1 << 10;

        // Global monitor (fires when app is NOT focused)
        let target_key_global = target_key.clone();
        let global_block = block2::RcBlock::new(move |event: *const AnyObject| {
            if event.is_null() {
                return;
            }
            let chars: *const AnyObject = msg_send![event, charactersIgnoringModifiers];
            if !chars.is_null() {
                let utf8: *const std::os::raw::c_char = msg_send![chars, UTF8String];
                if !utf8.is_null() {
                    let event_key = std::ffi::CStr::from_ptr(utf8)
                        .to_str()
                        .unwrap_or("")
                        .to_lowercase();
                    let event_mods: u64 = msg_send![event, modifierFlags];
                    // Mask to device-independent modifier flags only
                    let relevant_mods = event_mods & 0xFFFF0000;
                    if event_key == target_key_global && relevant_mods == ns_mods {
                        js_closure_call0(callback_ptr as *const u8);
                    }
                }
            }
        });

        let ns_event_cls = objc2::class!(NSEvent);
        let _: *const AnyObject = msg_send![
            ns_event_cls,
            addGlobalMonitorForEventsMatchingMask: key_down_mask,
            handler: &*global_block
        ];
        std::mem::forget(global_block);

        // Local monitor (fires when app IS focused)
        let target_key_local = target_key;
        let local_block =
            block2::RcBlock::new(move |event: *const AnyObject| -> *const AnyObject {
                if event.is_null() {
                    return event;
                }
                let chars: *const AnyObject = msg_send![event, charactersIgnoringModifiers];
                if !chars.is_null() {
                    let utf8: *const std::os::raw::c_char = msg_send![chars, UTF8String];
                    if !utf8.is_null() {
                        let event_key = std::ffi::CStr::from_ptr(utf8)
                            .to_str()
                            .unwrap_or("")
                            .to_lowercase();
                        let event_mods: u64 = msg_send![event, modifierFlags];
                        let relevant_mods = event_mods & 0xFFFF0000;
                        if event_key == target_key_local && relevant_mods == ns_mods {
                            js_closure_call0(callback_ptr as *const u8);
                        }
                    }
                }
                event // Return the event (don't consume it)
            });

        let _: *const AnyObject = msg_send![
            ns_event_cls,
            addLocalMonitorForEventsMatchingMask: key_down_mask,
            handler: &*local_block
        ];
        std::mem::forget(local_block);
    }
}

/// Add a keyboard shortcut to the app menu.
/// `key_ptr` is a StringHeader pointer to the key character (e.g., "s" for Cmd+S).
/// `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
/// `callback` is a NaN-boxed closure pointer.
///
/// If the menu bar doesn't exist yet (called before `app_run`), the shortcut
/// is buffered and will be installed once the menu is created.
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    // If the menu bar exists, install immediately; otherwise buffer for later.
    let has_menu = unsafe { app.mainMenu().is_some() };
    if has_menu {
        install_keyboard_shortcut(key_ptr, modifiers, callback, mtm);
    } else {
        PENDING_SHORTCUTS.with(|ps| {
            ps.borrow_mut().push(PendingShortcut {
                key_ptr,
                modifiers,
                callback,
            });
        });
    }
}

/// Install a single keyboard shortcut into the existing app menu.
fn install_keyboard_shortcut(
    key_ptr: *const u8,
    modifiers: f64,
    callback: f64,
    mtm: MainThreadMarker,
) {
    let key_str = str_from_header(key_ptr);
    let app = NSApplication::sharedApplication(mtm);

    unsafe {
        let target = PerryShortcutTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        SHORTCUT_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, callback);
        });

        let ns_key = NSString::from_str(key_str);
        let title = NSString::from_str(&format!("Shortcut {}", key_str));
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &title,
            Some(Sel::register(c"shortcutFired:")),
            &ns_key,
        );

        // Build modifier flags
        let mod_bits = modifiers as u64;
        let mut flags = NSEventModifierFlags::empty();
        if mod_bits & 1 != 0 {
            flags |= NSEventModifierFlags::Command;
        }
        if mod_bits & 2 != 0 {
            flags |= NSEventModifierFlags::Shift;
        }
        if mod_bits & 4 != 0 {
            flags |= NSEventModifierFlags::Option;
        }
        if mod_bits & 8 != 0 {
            flags |= NSEventModifierFlags::Control;
        }
        item.setKeyEquivalentModifierMask(flags);

        item.setTarget(Some(&target));
        std::mem::forget(target);

        // Add to the app menu (first menu item's submenu)
        if let Some(main_menu) = app.mainMenu() {
            if main_menu.numberOfItems() > 0 {
                if let Some(app_menu_item) = main_menu.itemAtIndex(0) {
                    if let Some(submenu) = app_menu_item.submenu() {
                        submenu.addItem(&item);
                    }
                }
            }
        }
    }
}

/// Flush any keyboard shortcuts that were registered before the menu bar existed.
fn flush_pending_shortcuts(mtm: MainThreadMarker) {
    flush_pending_shortcuts_inner(mtm);
}

/// Public entry point for flushing pending shortcuts (called from menu::menubar_attach).
pub fn flush_pending_shortcuts_pub(mtm: MainThreadMarker) {
    flush_pending_shortcuts_inner(mtm);
}

fn flush_pending_shortcuts_inner(mtm: MainThreadMarker) {
    PENDING_SHORTCUTS.with(|ps| {
        let pending: Vec<PendingShortcut> = ps.borrow_mut().drain(..).collect();
        for shortcut in pending {
            install_keyboard_shortcut(shortcut.key_ptr, shortcut.modifiers, shortcut.callback, mtm);
        }
    });
}

// ============================================
// Timer
// ============================================

thread_local! {
    static TIMER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
    fn js_frame_pump_default() -> i32;
}

pub struct PerryTimerTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTimerTarget"]
    #[ivars = PerryTimerTargetIvars]
    pub struct PerryTimerTarget;

    impl PerryTimerTarget {
        #[unsafe(method(timerFired:))]
        fn timer_fired(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("timer callback", std::panic::AssertUnwindSafe(|| {
                unsafe {
                    js_run_stdlib_pump();
                    js_promise_run_microtasks();
                }

                let key = self.ivars().callback_key.get();
                let closure_f64 = TIMER_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

// ============================================
// Timer Pump — drives setTimeout/setInterval callbacks
// ============================================

// Separate target class for the run loop pump timer.
// Calls timer tick functions from perry-runtime (always linked) and optionally
// runs the stdlib pump (registered at init if perry-stdlib is linked).
pub struct PerryPumpTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPumpTarget"]
    #[ivars = PerryPumpTargetIvars]
    pub struct PerryPumpTarget;

    impl PerryPumpTarget {
        #[unsafe(method(pump:))]
        fn pump(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("pump", std::panic::AssertUnwindSafe(|| {
                extern "C" {
                    fn js_run_stdlib_pump();
                    // perry-runtime's embedder GC stepper: spends up to
                    // `budget_us` advancing an ACTIVE budgeted collection
                    // in bounded work units; a cheap status probe when no
                    // cycle is active (out=null is allowed).
                    fn js_gc_step_us(budget_us: u64, out: *mut u8) -> u32;
                }
                unsafe {
                    js_callback_timer_tick();
                    js_interval_timer_tick();
                    // Issue #1865: perry/ui `onFrame` one-shot display-link
                    // callbacks. Driven off the same ~8ms pump for now;
                    // CADisplayLink-quality vsync alignment is a follow-up.
                    js_frame_pump_default();
                    js_promise_run_microtasks();
                    // Process deferred promise resolutions from perry-stdlib tokio workers.
                    // No-op if perry-stdlib is not linked (function pointer not registered).
                    js_run_stdlib_pump();
                    // #6183 (2026-07-09 GC audit): spend a bounded idle
                    // budget on any active budgeted GC cycle at the pump
                    // boundary — the JS stack is fully unwound here, so
                    // this is a precise-root safepoint. 2 ms of the ~8 ms
                    // tick drains collection debt in slices instead of
                    // letting an alloc-point collection land unbounded on
                    // this (main/UI) thread mid-gesture.
                    js_gc_step_us(2_000, std::ptr::null_mut());
                    #[cfg(feature = "geisterhand")]
                    {
                        extern "C" { fn perry_geisterhand_pump(); }
                        perry_geisterhand_pump();
                    }
                }
            }));
        }
    }
);

impl PerryPumpTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPumpTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

impl PerryTimerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTimerTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ============================================
// Application Delegate — handles file open events
// ============================================

pub struct PerryAppDelegateIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryAppDelegate"]
    #[ivars = PerryAppDelegateIvars]
    pub struct PerryAppDelegate;

    impl PerryAppDelegate {
        #[unsafe(method(application:openFile:))]
        fn application_open_file(&self, _app: &AnyObject, filename: &NSString) -> bool {
            let path = filename.to_string();
            PENDING_OPEN_FILES.with(|files| {
                files.borrow_mut().push(path);
            });
            true
        }

        /// Called when the user clicks the dock icon. Re-show the main window
        /// if no windows are visible (required by macOS App Store guidelines).
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(&self, _app: &AnyObject, has_visible_windows: bool) -> bool {
            if !has_visible_windows {
                APPS.with(|a| {
                    let apps = a.borrow();
                    if let Some(entry) = apps.first() {
                        entry.window.makeKeyAndOrderFront(None);
                    }
                });
            }
            true
        }

        /// Action handler for the "Show Main Window" menu item.
        #[unsafe(method(perryShowMainWindow:))]
        fn perry_show_main_window(&self, _sender: &AnyObject) {
            APPS.with(|a| {
                let apps = a.borrow();
                if let Some(entry) = apps.first() {
                    entry.window.makeKeyAndOrderFront(None);
                }
            });
        }

        /// Called by NSApplication just before the app exits. Invokes the
        /// user's `onTerminate` callback (if registered) so state/DB cleanup
        /// can run.
        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &AnyObject) {
            invoke_terminate_callback();
        }

        /// Called when the app becomes the frontmost app (launch, dock click,
        /// cmd-tab). Invokes the user's `onActivate` callback if registered.
        #[unsafe(method(applicationDidBecomeActive:))]
        fn application_did_become_active(&self, _notification: &AnyObject) {
            invoke_activate_callback();
        }

        /// APNs handed us a device token (#95). Hex-format it and call the
        /// closure passed to `notificationRegisterRemote`.
        #[unsafe(method(application:didRegisterForRemoteNotificationsWithDeviceToken:))]
        fn did_register_for_remote_notifications(
            &self,
            _app: &AnyObject,
            device_token: &AnyObject,
        ) {
            unsafe {
                crate::notifications::dispatch_device_token(
                    device_token as *const _ as *mut AnyObject,
                );
            }
        }

        /// APNs rejected the registration (missing entitlement, network
        /// error, …). Logged to stderr.
        #[unsafe(method(application:didFailToRegisterForRemoteNotificationsWithError:))]
        fn did_fail_to_register_for_remote_notifications(
            &self,
            _app: &AnyObject,
            error: &AnyObject,
        ) {
            unsafe {
                crate::notifications::dispatch_registration_failure(
                    error as *const _ as *mut AnyObject,
                );
            }
        }

        /// Foreground remote-notification payload. Background delivery is
        /// issue #98 (needs `fetchCompletionHandler:` variant).
        #[unsafe(method(application:didReceiveRemoteNotification:))]
        fn did_receive_remote_notification(
            &self,
            _app: &AnyObject,
            user_info: &AnyObject,
        ) {
            unsafe {
                crate::notifications::dispatch_remote_payload(
                    user_info as *const _ as *mut AnyObject,
                );
            }
        }

        /// Issue #583: AppKit URL delivery. AppKit calls this on launch
        /// (cold-start) and while the app is running (foreground). The
        /// dispatch helper inspects `mark_launched` to label the source.
        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _app: &AnyObject, urls: &AnyObject) {
            unsafe {
                crate::deeplinks::dispatch_open_urls(urls as *const AnyObject);
            }
        }

        /// Issue #583: legacy `kAEGetURL` Apple Event. Some launchers
        /// (older Spotlight, AppleScript `open location`) deliver custom
        /// schemes through this surface instead of the AppKit method.
        #[unsafe(method(apple_event_get_url:withReplyEvent:))]
        fn apple_event_get_url(&self, event: &AnyObject, _reply: &AnyObject) {
            unsafe {
                crate::deeplinks::dispatch_apple_event_url(event as *const AnyObject);
            }
        }

        /// Issue #583: NSApplication finished launching — flip the
        /// cold-start gate. Subsequent URL deliveries get
        /// `source = "foreground"`.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &AnyObject) {
            crate::deeplinks::mark_launched();
        }
    }
);

fn invoke_terminate_callback() {
    extern "C" {
        fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let cb = ON_TERMINATE_CALLBACK.with(|c| *c.borrow());
    if let Some(callback) = cb {
        unsafe {
            let ptr = js_nanbox_get_pointer(callback) as *const u8;
            if !ptr.is_null() {
                js_closure_call1(ptr, f64::from_bits(0x7FFC_0000_0000_0001)); // undefined
            }
        }
    }
}

fn invoke_activate_callback() {
    extern "C" {
        fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let cb = ON_ACTIVATE_CALLBACK.with(|c| *c.borrow());
    if let Some(callback) = cb {
        unsafe {
            let ptr = js_nanbox_get_pointer(callback) as *const u8;
            if !ptr.is_null() {
                js_closure_call1(ptr, f64::from_bits(0x7FFC_0000_0000_0001)); // undefined
            }
        }
    }
}

impl PerryAppDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryAppDelegateIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Poll for pending open-file requests. Returns the next file path or empty string.
pub fn poll_open_file() -> String {
    PENDING_OPEN_FILES.with(|files| {
        let mut files = files.borrow_mut();
        if files.is_empty() {
            String::new()
        } else {
            files.remove(0)
        }
    })
}

/// Set a recurring timer. interval_ms is in milliseconds.
/// The timer calls js_run_stdlib_pump() then invokes the callback.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let interval_secs = interval_ms / 1000.0;

    unsafe {
        let target = PerryTimerTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        TIMER_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, callback);
        });

        // NSTimer.scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:
        let sel = Sel::register(c"timerFired:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: interval_secs,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];

        // Keep the target alive
        std::mem::forget(target);
    }
}

// ============================================
// Lifecycle Hooks
// ============================================

/// Register an onTerminate callback.
pub fn register_on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Register an onActivate callback.
pub fn register_on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

// ============================================
// Multi-Window
// ============================================

/// Create a new window. Returns 1-based handle.
pub fn window_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Window"
    } else {
        str_from_header(title_ptr)
    };
    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let frame = CGRect::new(CGPoint::new(200.0, 200.0), CGSize::new(w, h));

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );

        let ns_title = NSString::from_str(title);
        window.setTitle(&ns_title);

        WINDOWS.with(|w| {
            let mut windows = w.borrow_mut();
            windows.push(WindowEntry { window });
            windows.len() as i64
        })
    }
}

/// Set the root widget of a window.
pub fn window_set_body(window_handle: i64, widget_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            if let Some(view) = crate::widgets::get_widget(widget_handle) {
                windows[idx].window.setContentView(Some(&view));
            }
        }
    });
}

/// Show a window (make key and order front).
pub fn window_show(window_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            windows[idx].window.center();
            windows[idx].window.makeKeyAndOrderFront(None);
        }
    });
}

/// Close a window.
pub fn window_close(window_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            windows[idx].window.close();
        }
    });
}

/// Hide a window without destroying it.
pub fn window_hide(window_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            windows[idx].window.orderOut(None);
        }
    });
}

/// Set window size. handle=0 targets the main app window.
pub fn window_set_size(window_handle: i64, width: f64, height: f64) {
    // Handle 0 = main app window
    if window_handle == 0 {
        APPS.with(|a| {
            let apps = a.borrow();
            if !apps.is_empty() {
                let window = &apps[0].window;
                unsafe {
                    let frame: CGRect = objc2::msg_send![window, frame];
                    // Keep top-left anchored: adjust origin.y by height difference
                    let dy = frame.size.height - height;
                    let new_frame = CGRect::new(
                        CGPoint::new(frame.origin.x, frame.origin.y + dy),
                        CGSize::new(width, height),
                    );
                    let _: () =
                        objc2::msg_send![window, setFrame: new_frame, display: true, animate: false];
                }
            }
        });
        return;
    }
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            let window = &windows[idx].window;
            unsafe {
                let frame: CGRect = objc2::msg_send![window, frame];
                let dy = frame.size.height - height;
                let new_frame = CGRect::new(
                    CGPoint::new(frame.origin.x, frame.origin.y + dy),
                    CGSize::new(width, height),
                );
                let _: () =
                    objc2::msg_send![window, setFrame: new_frame, display: true, animate: true];
            }
        }
    });
}

thread_local! {
    static WINDOW_FOCUS_LOST_CBS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

/// Register a callback for when the window loses focus.
pub fn window_on_focus_lost(window_handle: i64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
        fn js_closure_call0(closure: *const u8) -> f64;
    }
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            let window = &windows[idx].window;
            WINDOW_FOCUS_LOST_CBS.with(|cbs| {
                cbs.borrow_mut().insert(window_handle, callback);
            });
            unsafe {
                let center: Retained<AnyObject> =
                    objc2::msg_send![objc2::class!(NSNotificationCenter), defaultCenter];
                let name = NSString::from_str("NSWindowDidResignKeyNotification");
                let callback_copy = callback;
                let block = block2::RcBlock::new(move |_notif: *const AnyObject| {
                    let ptr = js_nanbox_get_pointer(callback_copy) as *const u8;
                    js_closure_call0(ptr);
                });
                let _: Retained<AnyObject> = objc2::msg_send![
                    &*center,
                    addObserverForName: &*name,
                    object: &**window,
                    queue: std::ptr::null::<AnyObject>(),
                    usingBlock: &*block
                ];
                std::mem::forget(block);
            }
        }
    });
}

// ============================================
// App Icon Retrieval
// ============================================

/// Get the icon for a file/application at the given path, returned as an NSImageView widget handle.
/// Uses NSWorkspace.iconForFile: to retrieve the system icon.
///
/// Safe to call during UI callbacks — wrapped in an autorelease pool and retains all
/// intermediate objects to prevent use-after-free during AppKit event dispatch.
pub fn get_app_icon(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    if path.is_empty() {
        return 0;
    }

    // Wrap in autorelease pool — required when called during callbacks where
    // AppKit may not have an active pool (e.g. TextField onChange).
    objc2::rc::autoreleasepool(|_pool| {
        unsafe {
            let ns_path = NSString::from_str(path);

            // NSWorkspace.sharedWorkspace is a singleton — always valid on the main thread.
            let workspace: *const AnyObject =
                msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return 0;
            }

            // iconForFile: returns an autoreleased NSImage.
            // Retain it immediately so it survives pool drain.
            let icon_raw: *const AnyObject = msg_send![
                workspace, iconForFile: &*ns_path
            ];
            if icon_raw.is_null() {
                return 0;
            }
            let icon: Retained<AnyObject> = match Retained::retain(icon_raw as *mut AnyObject) {
                Some(r) => r,
                None => return 0,
            };

            // Set icon size to 32x32 (reasonable default)
            let size = CGSize::new(32.0, 32.0);
            let _: () = msg_send![&*icon, setSize: size];

            // Create an NSImageView with the retained icon
            let image_view: Retained<AnyObject> = msg_send![
                objc2::class!(NSImageView), imageViewWithImage: &*icon
            ];

            // Set a default frame size
            let frame_size = CGSize::new(32.0, 32.0);
            let _: () = msg_send![&*image_view, setFrameSize: frame_size];

            // Cast to NSView and register as widget
            let view: Retained<objc2_app_kit::NSView> = Retained::cast_unchecked(image_view);
            widgets::register_widget(view)
        }
    })
}

// ============================================================================
// Phase 2 v3.3: cross-platform showToast / setText wiring.
// ============================================================================

extern "C" {
    /// Defined in `perry-runtime/src/ui_text_registry.rs`. Stores the
    /// passed handler in an AtomicPtr that
    /// `perry_arkts_show_toast`/`perry_arkts_set_text`/`perry_arkts_register_text_id`
    /// consult on each call. No-op when `ohos-napi` is on (the drain-queue
    /// path owns dispatch there).
    fn js_register_show_toast_handler(f: extern "C" fn(msg_ptr: *const u8, msg_len: usize));
    fn js_register_set_text_handler(
        f: extern "C" fn(id_ptr: *const u8, id_len: usize, val_ptr: *const u8, val_len: usize),
    );
    fn js_register_text_id_handler(
        f: extern "C" fn(widget_handle: i64, id_ptr: *const u8, id_len: usize),
    );
    /// Issue #535 Layer 2 — `js_state_set` calls this for every NavStack
    /// route bound to the changed state's synth id. Defined in
    /// `perry-runtime/src/ui_text_registry.rs`'s `NAVSTACK_REGISTRY` block.
    fn js_register_widget_hidden_handler(f: extern "C" fn(widget_handle: i64, hidden: i32));
    /// Issue #610 — `js_state_set` calls this for every ForEach binding
    /// when the bound `State<number>`'s value changes. The handler clears
    /// the host's children, calls `render_closure(i)` for each
    /// `i in [0..count)`, and adds each returned widget. Defined in
    /// `perry-runtime/src/ui_text_registry.rs`'s `FOREACH_REGISTRY` block.
    fn js_register_foreach_render_handler(
        f: extern "C" fn(container_handle: i64, render_closure: f64, count: f64),
    );
}

extern "C" fn navstack_set_widget_hidden(widget_handle: i64, hidden: i32) {
    widgets::set_hidden(widget_handle, hidden != 0);
}

fn register_cross_platform_text_handlers() {
    unsafe {
        js_register_show_toast_handler(widgets::toast::show_toast_handler);
        js_register_set_text_handler(widgets::text_registry::set_text_handler);
        js_register_text_id_handler(widgets::text_registry::register_text_id_handler);
        js_register_widget_hidden_handler(navstack_set_widget_hidden);
        js_register_foreach_render_handler(widgets::foreach_registry::render_handler);
    }
}
