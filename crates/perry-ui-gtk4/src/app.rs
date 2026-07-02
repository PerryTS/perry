use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, EventControllerKey};

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

use crate::widgets;

static GTK_INIT: Once = Once::new();

/// Ensure GTK is initialized (safe to call multiple times).
pub(crate) fn ensure_gtk_init() {
    GTK_INIT.call_once(|| {
        gtk4::init().expect("Failed to initialize GTK4");
    });
}

thread_local! {
    static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    /// Activation policy set (via `appSetActivationPolicy`) before `App()`
    /// builds the AppEntry. Handle-agnostic, mirroring the macOS backend, so
    /// the synthetic 0 app-handle from the 1-arg TS form still works.
    static PENDING_ACTIVATION_POLICY: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Buffered keyboard shortcuts registered before the app is running.
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = RefCell::new(Vec::new());
    /// Callback map for keyboard shortcuts.
    static SHORTCUT_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Counter for generating unique callback keys.
    static NEXT_CALLBACK_KEY: RefCell<usize> = RefCell::new(1);
    /// Reference to the GTK Application (public for sheet/toolbar/window/system access).
    pub(crate) static GTK_APP: RefCell<Option<Application>> = RefCell::new(None);
    /// Reference to the main ApplicationWindow (public for screenshot capture).
    pub(crate) static APP_WINDOW: RefCell<Option<ApplicationWindow>> = RefCell::new(None);
    /// Timer callbacks queued before the app activates (interval_ms, callback f64).
    /// After activation, set_timer schedules directly via install_timer().
    static TIMER_CALLBACKS: RefCell<Vec<(f64, f64)>> = RefCell::new(Vec::new());
    /// True once connect_activate has fired — set_timer can then call glib::timeout_add_local
    /// directly instead of buffering.
    static APP_ACTIVATED: RefCell<bool> = RefCell::new(false);
    /// App lifecycle callbacks
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
}

struct PendingShortcut {
    key_ptr: *const u8,
    modifiers: f64,
    callback: f64,
}

// SAFETY: PendingShortcut contains a raw pointer that is only dereferenced on the main thread
// where it was created. We need Send to store it in a thread_local RefCell.
unsafe impl Send for PendingShortcut {}

struct AppEntry {
    title: String,
    width: f64,
    height: f64,
    root_handle: Option<i64>,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
    frameless: bool,
    level: Option<String>,
    transparent: bool,
    vibrancy: Option<String>,
    activation_policy: Option<String>,
    /// Issue #1280 — "maximized" | "fullscreen" | None (= "normal").
    window_state: Option<String>,
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
    fn js_run_ext_pump();
    fn js_frame_pump_default() -> i32;
}

/// Extract a &str from a *const StringHeader pointer.
pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Create an app with title, width, height.
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    ensure_gtk_init();

    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        apps.push(AppEntry {
            title,
            width: w,
            height: h,
            root_handle: None,
            min_size: None,
            max_size: None,
            frameless: false,
            level: None,
            transparent: false,
            vibrancy: None,
            activation_policy: PENDING_ACTIVATION_POLICY.with(|p| p.borrow().clone()),
            window_state: None,
        });
        apps.len() as i64 // 1-based handle
    })
}

/// Set the root widget (body) of the app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_handle = Some(root_handle);
        }
    });
}

/// Run the application event loop (blocks).
pub fn app_run(_app_handle: i64) {
    let app = Application::builder()
        .application_id("com.perry.app")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    GTK_APP.with(|ga| {
        *ga.borrow_mut() = Some(app.clone());
    });

    app.connect_activate(move |app| {
        // Load global CSS to tighten Adwaita button/widget padding so controls
        // match the compact sizing of macOS/AppKit rather than the oversized
        // GNOME defaults.  Use USER priority (800) to override Adwaita theme (200).
        let global_css = gtk4::CssProvider::new();
        global_css.load_from_data(
            "button { padding: 2px 8px; min-height: 0; min-width: 0; }\n\
             button.flat { padding: 2px 8px; min-height: 0; min-width: 0; }\n\
             button label { padding: 0; margin: 0; }\n\
             .perry-mini button, button.perry-mini { padding: 1px 4px; font-size: 11px; }\n\
             .perry-small button, button.perry-small { padding: 2px 6px; font-size: 12px; }\n\
             .perry-regular button, button.perry-regular { padding: 2px 8px; }\n\
             .perry-large button, button.perry-large { padding: 6px 12px; font-size: 15px; }\n\
             entry { min-height: 0; padding: 2px 6px; }\n",
        );
        if let Some(display) = gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &global_css,
                gtk4::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
        // Install the menu bar model on the app BEFORE creating windows,
        // so that show_menubar(true) takes effect when the window is first presented.
        let has_menubar = crate::menu::PENDING_MENUBAR.with(|p| p.borrow().is_some());
        if has_menubar {
            crate::menu::PENDING_MENUBAR.with(|p| {
                if let Some(bar_handle) = *p.borrow() {
                    crate::menu::install_menubar_on_app(app, bar_handle);
                }
            });
        }

        APPS.with(|a| {
            let apps = a.borrow();
            for entry in apps.iter() {
                let window = ApplicationWindow::builder()
                    .application(app)
                    .title(&entry.title)
                    .default_width(entry.width as i32)
                    .default_height(entry.height as i32)
                    .decorated(!entry.frameless)
                    .build();

                // Store the window for screenshot capture (first window wins).
                APP_WINDOW.with(|aw| {
                    if aw.borrow().is_none() {
                        *aw.borrow_mut() = Some(window.clone());
                    }
                });

                // Set min/max size hints via GDK geometry
                if let Some((min_w, min_h)) = entry.min_size {
                    window.set_size_request(min_w as i32, min_h as i32);
                }

                // Note: GTK4 doesn't have a direct max-size API. Apps typically
                // use set_resizable(false) or handle size constraints differently.
                // We store it but GTK4 relies on window manager for max size.

                // Apply window level (always on top).
                // GTK4 removed set_keep_above; best-effort via focus-on-map
                // and modal behavior which most compositors keep above.
                if let Some(ref level) = entry.level {
                    match level.as_str() {
                        "floating" | "statusBar" | "modal" => {
                            window.set_modal(true);
                        }
                        _ => {}
                    }
                }

                // Apply transparency via CSS
                if entry.transparent {
                    let css_provider = gtk4::CssProvider::new();
                    css_provider.load_from_data("window { background-color: transparent; }");
                    gtk4::style_context_add_provider_for_display(
                        &gdk::Display::default().expect("display"),
                        &css_provider,
                        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                }

                // Apply vibrancy (best-effort: semi-transparent background via CSS)
                if let Some(ref _vibrancy) = entry.vibrancy {
                    let css_provider = gtk4::CssProvider::new();
                    css_provider.load_from_data(
                        "window { background-color: alpha(@window_bg_color, 0.85); }",
                    );
                    gtk4::style_context_add_provider_for_display(
                        &gdk::Display::default().expect("display"),
                        &css_provider,
                        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
                    );
                }

                // Accessory/background apps skip the taskbar. GTK4 has no
                // skip_taskbar_hint; best-effort via deletable=false, which
                // some compositors interpret as a utility window.
                let suppress_window = matches!(
                    entry.activation_policy.as_deref(),
                    Some("accessory") | Some("background")
                );
                if suppress_window {
                    window.set_deletable(false);
                }

                if let Some(root_handle) = entry.root_handle {
                    if let Some(widget) = widgets::get_widget(root_handle) {
                        // Ensure root widget fills the window
                        widget.set_hexpand(true);
                        widget.set_vexpand(true);
                        widget.set_halign(gtk4::Align::Fill);
                        widget.set_valign(gtk4::Align::Fill);
                        // Wrap in an Overlay so toast banners can be layered on
                        // top without altering the user's widget tree.
                        let overlay = gtk4::Overlay::new();
                        overlay.set_child(Some(&widget));
                        overlay.set_hexpand(true);
                        overlay.set_vexpand(true);
                        overlay.set_halign(gtk4::Align::Fill);
                        overlay.set_valign(gtk4::Align::Fill);
                        window.set_child(Some(&overlay));
                        crate::widgets::toast::set_toast_overlay(&overlay);
                    }
                }

                // Install keyboard shortcuts on this window
                install_shortcuts_on_window(&window);

                // Issue #1864: continuous onKeyDown/onKeyUp via a second
                // EventControllerKey routed through the shared dispatcher.
                crate::keyboard::install_on_window(&window);

                // Enable the menu bar on this window before presenting it
                if has_menubar {
                    window.set_show_menubar(true);
                }

                // Issue #1280 — initial window state. GTK4 needs maximize() /
                // fullscreen() called before `present()` so the window appears
                // already in the requested state rather than flickering.
                if let Some(ref state) = entry.window_state {
                    match state.as_str() {
                        "maximized" => window.maximize(),
                        "fullscreen" => window.fullscreen(),
                        _ => {}
                    }
                }

                // Accessory/background apps stay window-less at launch (the
                // tray / status notifier owns presentation); open windows on
                // demand instead. Mirrors the macOS backend.
                if !suppress_window {
                    window.present();
                }
            }
        });

        // Mark the app as activated so set_timer can schedule directly from here on,
        // then install any timers that were queued before activate fired.
        APP_ACTIVATED.with(|a| *a.borrow_mut() = true);
        TIMER_CALLBACKS.with(|tc| {
            let queued: Vec<(f64, f64)> = tc.borrow_mut().drain(..).collect();
            for (interval_ms, callback) in queued {
                install_timer(interval_ms, callback);
            }
        });

        // Start the timer pump to drive setInterval/setTimeout callbacks (~120Hz)
        // This mirrors the implementation on other platforms (macOS, Windows, etc.)
        glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
            unsafe {
                js_callback_timer_tick();
                js_interval_timer_tick();
                // Issue #1865: perry/ui `onFrame` display-link callbacks.
                // Real gtk_widget_add_tick_callback wiring is a follow-up.
                js_frame_pump_default();
                js_run_ext_pump();
                js_run_stdlib_pump();
                js_promise_run_microtasks();
            }
            glib::ControlFlow::Continue
        });

        // Call on_activate callback
        ON_ACTIVATE_CALLBACK.with(|cb| {
            if let Some(callback) = *cb.borrow() {
                let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                unsafe {
                    js_closure_call0(ptr);
                }
            }
        });

        // If PERRY_UI_TEST_MODE is set, schedule an automatic exit so doc-example
        // programs can be verified in CI without a human.
        if perry_ui_testkit::is_test_mode() {
            let delay = std::time::Duration::from_millis(perry_ui_testkit::exit_delay_ms() as u64);
            let app_clone = app.clone();
            glib::timeout_add_local_once(delay, move || {
                if let Some(path) = perry_ui_testkit::screenshot_path() {
                    let mut len: usize = 0;
                    let ptr =
                        crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
                    if !ptr.is_null() && len > 0 {
                        perry_ui_testkit::write_screenshot_bytes(&path, ptr, len);
                        unsafe {
                            libc::free(ptr as *mut libc::c_void);
                        }
                    }
                }
                app_clone.quit();
            });
        }
    });

    // Install shutdown handler for on_terminate
    app.connect_shutdown(move |_app| {
        ON_TERMINATE_CALLBACK.with(|cb| {
            if let Some(callback) = *cb.borrow() {
                let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                unsafe {
                    js_closure_call0(ptr);
                }
            }
        });
    });

    // Register UI function pointers for geisterhand dispatch
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register_state_set(f: extern "C" fn(i64, f64));
            fn perry_geisterhand_register_screenshot_capture(
                f: extern "C" fn(*mut usize) -> *mut u8,
            );
            fn perry_geisterhand_register_textfield_set_string(f: extern "C" fn(i64, i64));
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
            perry_geisterhand_register_apply_style(crate::geisterhand_style::apply_style);
        }
    }

    register_cross_platform_text_handlers();

    // GTK Application::run() blocks like NSApplication.run()
    // Pass empty args since we handle our own argument parsing
    let empty: Vec<String> = vec![];
    app.run_with_args(&empty);
}

// =============================================================================
// Cross-platform showToast / setText / NavStack-hidden handler registration.
// Issue #535: routes runtime-side state writes (`js_state_set`,
// `perry_arkts_set_text`, `perry_arkts_show_toast`) to GTK4 widget mutations.
// =============================================================================

extern "C" {
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
}

extern "C" fn navstack_set_widget_hidden(widget_handle: i64, hidden: i32) {
    crate::widgets::set_hidden(widget_handle, hidden != 0);
}

fn register_cross_platform_text_handlers() {
    unsafe {
        js_register_show_toast_handler(crate::widgets::toast::show_toast_handler);
        js_register_set_text_handler(crate::widgets::text_registry::set_text_handler);
        js_register_text_id_handler(crate::widgets::text_registry::register_text_id_handler);
        js_register_widget_hidden_handler(navstack_set_widget_hidden);
    }
}

/// Set the minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].min_size = Some((w, h));
        }
    });
}

/// Set the maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].max_size = Some((w, h));
        }
    });
}

/// Resize the main app window dynamically.
/// Stores the size and applies if the window exists (realized via connect_activate).
pub fn app_set_size(app_handle: i64, width: f64, height: f64) {
    // On GTK4, the window is realized during app_run. If we're called after that,
    // we need to resize the actual window. Try the stored APP_WINDOW first.
    APP_WINDOW.with(|aw| {
        if let Some(ref window) = *aw.borrow() {
            window.set_default_size(width as i32, height as i32);
            // queue_resize ensures the window manager processes the change
            window.queue_resize();
            return;
        }
    });
    // If window not yet realized, update the stored dimensions
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].width = width;
            apps[idx].height = height;
        }
    });
}

/// Set frameless window mode (no titlebar/decorations).
/// `value` is a NaN-boxed boolean — TAG_TRUE = 0x7FFC_0000_0000_0004.
pub fn app_set_frameless(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].frameless = true;
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
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].level = Some(level_str.to_string());
        }
    });
}

/// Set window transparency.
/// `value` is a NaN-boxed boolean.
pub fn app_set_transparent(app_handle: i64, value: f64) {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    if value.to_bits() != TAG_TRUE {
        return;
    }
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].transparent = true;
        }
    });
}

/// Set vibrancy material. On GTK4 this is a best-effort CSS opacity effect
/// since true vibrancy depends on the compositor.
pub fn app_set_vibrancy(app_handle: i64, value_ptr: *const u8) {
    let material_str = str_from_header(value_ptr);
    if material_str.is_empty() {
        return;
    }
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].vibrancy = Some(material_str.to_string());
        }
    });
}

/// Set activation policy: "regular", "accessory", or "background".
/// On Linux: "accessory"/"background" skips the taskbar.
pub fn app_set_activation_policy(app_handle: i64, value_ptr: *const u8) {
    let policy_str = str_from_header(value_ptr);
    if policy_str.is_empty() {
        return;
    }
    // Stash handle-agnostically (mirrors macOS): `App()` reads this when it
    // builds the AppEntry. Also set it on an existing entry if one is already
    // registered, so a post-create call still takes effect.
    PENDING_ACTIVATION_POLICY.with(|p| *p.borrow_mut() = Some(policy_str.to_string()));
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].activation_policy = Some(policy_str.to_string());
        }
    });
}

/// Issue #1280 — initial window state. value_ptr points at a StringHeader
/// for one of "normal" | "maximized" | "fullscreen". Anything else is
/// silently ignored; the state is applied just before `window.present()`.
pub fn app_set_window_state(app_handle: i64, value_ptr: *const u8) {
    let state_str = str_from_header(value_ptr);
    if state_str.is_empty() {
        return;
    }
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window_state = match state_str {
                "maximized" | "fullscreen" => Some(state_str.to_string()),
                _ => None,
            };
        }
    });
}

/// Install keyboard shortcuts on a window using EventControllerKey.
fn install_shortcuts_on_window(window: &ApplicationWindow) {
    let controller = EventControllerKey::new();

    controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();

        // Find matching shortcut
        let matched = PENDING_SHORTCUTS.with(|ps| {
            let shortcuts = ps.borrow();
            for shortcut in shortcuts.iter() {
                let shortcut_key = str_from_header(shortcut.key_ptr);

                // Convert Perry modifier bits to GDK modifier state
                let mod_bits = shortcut.modifiers as u64;
                // Perry: 1=Cmd, 2=Shift, 4=Option, 8=Control
                // On Linux: Cmd maps to Ctrl
                let mut expected = gdk::ModifierType::empty();
                if mod_bits & 1 != 0 {
                    expected |= gdk::ModifierType::CONTROL_MASK;
                } // Cmd -> Ctrl
                if mod_bits & 2 != 0 {
                    expected |= gdk::ModifierType::SHIFT_MASK;
                }
                if mod_bits & 4 != 0 {
                    expected |= gdk::ModifierType::ALT_MASK;
                } // Option -> Alt
                if mod_bits & 8 != 0 {
                    expected |= gdk::ModifierType::CONTROL_MASK;
                }

                // Check key match (case-insensitive single char)
                let key_matches = key_name.eq_ignore_ascii_case(shortcut_key);

                // Check modifier match (mask out irrelevant bits)
                let relevant = gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK;
                let mod_matches = (modifier & relevant) == (expected & relevant);

                if key_matches && mod_matches {
                    return Some(shortcut.callback);
                }
            }
            None
        });

        if let Some(callback) = matched {
            let closure_ptr = unsafe { js_nanbox_get_pointer(callback) };
            unsafe {
                js_closure_call0(closure_ptr as *const u8);
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window.add_controller(controller);
}

/// Add a keyboard shortcut.
/// `key_ptr` is a StringHeader pointer to the key character (e.g., "s" for Cmd+S).
/// `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
/// `callback` is a NaN-boxed closure pointer.
///
/// On Linux, Cmd (modifier 1) is transparently remapped to Ctrl.
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    PENDING_SHORTCUTS.with(|ps| {
        ps.borrow_mut().push(PendingShortcut {
            key_ptr,
            modifiers,
            callback,
        });
    });
}

/// Install a recurring glib timeout that drains the stdlib pump + microtasks, invokes
/// the JS callback, and (with `geisterhand`) ticks the geisterhand pump. Must run on
/// the GTK main thread after the GLib MainContext is active.
fn install_timer(interval_ms: f64, callback: f64) {
    let ms = interval_ms as u64;
    glib::timeout_add_local(std::time::Duration::from_millis(ms), move || {
        unsafe {
            js_run_stdlib_pump();
            js_promise_run_microtasks();
        }
        let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
        unsafe {
            js_closure_call0(ptr);
        }
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_pump();
            }
            unsafe {
                perry_geisterhand_pump();
            }
        }
        glib::ControlFlow::Continue
    });
}

/// Set a repeating timer. interval_ms = milliseconds between ticks.
/// Queues the timer until `connect_activate` fires; after that, schedules immediately
/// so callers from button handlers or other timer callbacks (#429) actually run.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let activated = APP_ACTIVATED.with(|a| *a.borrow());
    if activated {
        install_timer(interval_ms, callback);
    } else {
        TIMER_CALLBACKS.with(|tc| {
            tc.borrow_mut().push((interval_ms, callback));
        });
    }
}

/// Register an on_activate callback (called when the app becomes active).
pub fn on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Register an on_terminate callback (called when the app is shutting down).
pub fn on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Register a system-wide global hotkey.
/// On Linux this is not yet supported (requires X11-specific code or Wayland portals).
pub fn register_global_hotkey(key_ptr: *const u8, _modifiers: f64, _callback: f64) {
    let key_str = str_from_header(key_ptr);
    eprintln!("[perry/ui] registerGlobalHotkey('{}') is not yet supported on Linux (requires X11/Wayland portal)", key_str);
}

/// Get the icon for an application at the given path.
/// Supports .desktop files (Icon= field lookup via GTK icon theme) and direct image paths.
pub fn get_app_icon(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    if path.is_empty() {
        return 0;
    }

    // .desktop file: parse for Icon= field
    if path.ends_with(".desktop") {
        if let Ok(content) = std::fs::read_to_string(path) {
            for line in content.lines() {
                if let Some(icon_name) = line.strip_prefix("Icon=") {
                    let icon_name = icon_name.trim();
                    // Try icon theme lookup
                    let display = gtk4::gdk::Display::default();
                    if let Some(display) = display {
                        let theme = gtk4::IconTheme::for_display(&display);
                        if theme.has_icon(icon_name) {
                            let image = gtk4::Image::from_icon_name(icon_name);
                            image.set_pixel_size(32);
                            return widgets::register_widget(image.upcast());
                        }
                    }
                    // Fallback: try as absolute path
                    if std::path::Path::new(icon_name).exists() {
                        let image = gtk4::Image::from_file(icon_name);
                        image.set_pixel_size(32);
                        return widgets::register_widget(image.upcast());
                    }
                }
            }
        }
    }

    // Direct file path — try loading as image
    if std::path::Path::new(path).exists() {
        let image = gtk4::Image::from_file(path);
        image.set_pixel_size(32);
        return widgets::register_widget(image.upcast());
    }

    0
}
