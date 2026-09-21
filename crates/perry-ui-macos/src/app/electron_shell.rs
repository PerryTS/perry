//! Electron-compat app shell (`packages/electron`): the body-less run loop
//! behind `app.whenReady()`, its top-level entry request, `app.quit()`, and
//! the full-window body pinning a `BrowserWindow` needs.

use super::*;
use objc2_app_kit::NSView;

/// Body-less event loop for the Electron-compat shell (`app.whenReady()`).
///
/// Unlike `app_run`, this does not expect any pre-built `App({...})` window.
/// It sets up `NSApplication`, the menu bar, the app delegate and the ~8ms
/// timer pump, then — once everything is in place — invokes the `on_ready` TS
/// closure (which resolves the shim's `app.whenReady()` promise) and blocks in
/// `NSApplication.run()`. The `.then(createWindow)` chain runs on the first pump
/// tick, so `new BrowserWindow()` / `Window()` create real windows while the
/// loop is live. Windows are created dynamically via the multi-window FFI
/// (`window_create` / `window_set_body` / `window_show`).
pub fn app_run_loop(on_ready: f64) {
    crate::crash_log::install_crash_hooks();
    register_cross_platform_text_handlers();

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    let policy = PENDING_ACTIVATION_POLICY.with(|p| p.borrow().clone());
    let activation_policy = match policy.as_deref() {
        Some("accessory") => NSApplicationActivationPolicy::Accessory,
        Some("background") => NSApplicationActivationPolicy::Prohibited,
        _ => NSApplicationActivationPolicy::Regular,
    };
    app.setActivationPolicy(activation_policy);

    PENDING_ICON_PATH.with(|p| {
        if let Some(path) = p.borrow().as_ref() {
            unsafe {
                let ns_path = NSString::from_str(path);
                let image: Option<Retained<NSImage>> =
                    msg_send![NSImage::alloc(), initWithContentsOfFile: &*ns_path];
                if let Some(img) = image {
                    let _: () = msg_send![&*app, setApplicationIconImage: &*img];
                }
            }
        }
    });

    setup_menu_bar(&app, mtm);
    flush_pending_shortcuts(mtm);

    unsafe {
        let delegate = PerryAppDelegate::new();
        let _: () = msg_send![&*app, setDelegate: &*delegate];
        crate::deeplinks::install_apple_event_handler(&*delegate as *const _ as *const AnyObject);
        std::mem::forget(delegate);
    }

    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    // ~8ms (120Hz) timer pump drives setTimeout/setInterval/microtasks/stdlib.
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
        std::mem::forget(target);
    }

    install_test_mode_exit_timer();

    // Resolve `app.whenReady()`. This only enqueues the `.then` microtask; it
    // runs on the first pump tick once `app.run()` is spinning below.
    // Reload the registered slot immediately before use: a moving collection
    // during setup may have rewritten it since `ui_loop_entry` first read it.
    let on_ready = PENDING_APP_LOOP_ON_READY.with(|slot| {
        let rooted = f64::from_bits(slot.get());
        if rooted == 0.0 {
            on_ready
        } else {
            rooted
        }
    });
    if on_ready != 0.0 {
        extern "C" {
            fn js_nanbox_get_pointer(value: f64) -> i64;
            fn js_closure_call0(closure: *const u8) -> f64;
        }
        crate::catch_callback_panic(
            "app whenReady",
            std::panic::AssertUnwindSafe(|| unsafe {
                let ptr = js_nanbox_get_pointer(on_ready) as *const u8;
                if !ptr.is_null() {
                    js_closure_call0(ptr);
                }
            }),
        );
    }

    app.run();
}

thread_local! {
    /// `on_ready` closure for the deferred top-level UI loop (Electron-compat).
    /// The slot is registered with the runtime GC before it receives a closure,
    /// so moving collections can both retain and rewrite the stored value.
    static PENDING_APP_LOOP_ON_READY: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static PENDING_APP_LOOP_ROOT_REGISTERED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Request the Electron-compat UI event loop be entered at the TOP LEVEL after
/// `main` (not from a microtask). The shim calls this during module init; it
/// stores `on_ready` and registers `ui_loop_entry` with the runtime so the
/// generated `main`'s `js_ui_loop_take_over()` hands control to `app_run_loop`
/// at the top level. Entering `[NSApp run]` from a microtask leaves windows
/// off-screen, so this top-level entry is required for windows to composite.
pub fn request_app_loop(on_ready: f64) {
    extern "C" {
        fn perry_runtime_register_ui_loop(f: extern "C" fn());
        fn js_gc_register_global_root(ptr: i64);
        fn js_write_barrier_root_nanbox(value_bits: u64);
    }
    PENDING_APP_LOOP_ON_READY.with(|slot| {
        PENDING_APP_LOOP_ROOT_REGISTERED.with(|registered| {
            if !registered.replace(true) {
                unsafe {
                    js_gc_register_global_root(slot.as_ptr() as i64);
                }
            }
        });
        slot.set(on_ready.to_bits());
        unsafe {
            js_write_barrier_root_nanbox(on_ready.to_bits());
        }
    });
    unsafe {
        perry_runtime_register_ui_loop(ui_loop_entry);
    }
}

extern "C" fn ui_loop_entry() {
    let on_ready = PENDING_APP_LOOP_ON_READY.with(|c| f64::from_bits(c.get()));
    app_run_loop(on_ready);
    // `app_run_loop` normally never returns. Clear the permanent root if the
    // application loop does terminate (for example under UI test mode).
    PENDING_APP_LOOP_ON_READY.with(|c| c.set(0));
}

/// `app.quit()` — terminate the application (Electron-compat).
pub fn app_quit() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        unsafe {
            let _: () = msg_send![&*app, terminate: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Pin `view` to `window`'s `contentLayoutGuide` with Auto Layout so a
/// full-window body (an Electron `BrowserWindow` whose entire content is the
/// page) fills and resizes with the window. Mirrors the pinning
/// `app_set_body` does for the main window.
pub(super) fn pin_to_content_layout_guide(window: &NSWindow, view: &NSView) {
    unsafe {
        let _: () = msg_send![view, setTranslatesAutoresizingMaskIntoConstraints: false];
        let guide: Retained<AnyObject> = msg_send![window, contentLayoutGuide];
        let guide_top: Retained<AnyObject> = msg_send![&*guide, topAnchor];
        let guide_bottom: Retained<AnyObject> = msg_send![&*guide, bottomAnchor];
        let guide_leading: Retained<AnyObject> = msg_send![&*guide, leadingAnchor];
        let guide_trailing: Retained<AnyObject> = msg_send![&*guide, trailingAnchor];

        let top_anchor = view.topAnchor();
        let bottom_anchor = view.bottomAnchor();
        let leading_anchor = view.leadingAnchor();
        let trailing_anchor = view.trailingAnchor();

        let c_top: Retained<AnyObject> =
            msg_send![&*top_anchor, constraintEqualToAnchor: &*guide_top];
        let c_bottom: Retained<AnyObject> =
            msg_send![&*bottom_anchor, constraintEqualToAnchor: &*guide_bottom];
        let c_leading: Retained<AnyObject> =
            msg_send![&*leading_anchor, constraintEqualToAnchor: &*guide_leading];
        let c_trailing: Retained<AnyObject> =
            msg_send![&*trailing_anchor, constraintEqualToAnchor: &*guide_trailing];

        let _: () = msg_send![&*c_top, setActive: true];
        let _: () = msg_send![&*c_bottom, setActive: true];
        let _: () = msg_send![&*c_leading, setActive: true];
        let _: () = msg_send![&*c_trailing, setActive: true];
    }
}
