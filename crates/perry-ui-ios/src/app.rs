use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, ClassType, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::{UIView, UIViewController, UIWindow};

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;

use crate::menu;
use crate::widgets;

thread_local! {
    static PENDING_CONFIG: RefCell<Option<AppConfig>> = RefCell::new(None);
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
    pub(crate) static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    /// The bottom constraint of the root widget, adjusted when keyboard appears/disappears
    static ROOT_BOTTOM_CONSTRAINT: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
}

struct AppConfig {
    title: String,
    _width: f64,
    _height: f64,
}

pub(crate) struct AppEntry {
    pub(crate) window: Retained<UIWindow>,
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

/// Create an app. Stores config in thread-local for deferred creation.
/// Returns app handle (i64).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    PENDING_CONFIG.with(|c| {
        *c.borrow_mut() = Some(AppConfig {
            title,
            _width: w,
            _height: h,
        });
    });

    1 // Single app handle
}

/// Set the root widget (body) of the app.
pub fn app_set_body(_app_handle: i64, root_handle: i64) {
    PENDING_BODY.with(|b| {
        *b.borrow_mut() = Some(root_handle);
    });
}

/// Define the PerryAppDelegate class for UIApplicationDelegate protocol.
pub struct PerryAppDelegateIvars {}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryAppDelegate"]
    #[ivars = PerryAppDelegateIvars]
    pub struct PerryAppDelegate;

    impl PerryAppDelegate {
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        fn did_finish_launching(&self, _application: &AnyObject, _options: *const AnyObject) -> bool {
            // Window creation is handled by PerrySceneDelegate.
            // Fire notification auth prompt once here so notificationSend() doesn't
            // re-prompt on every call (per #94).
            crate::notifications::request_authorization();
            // Drain pending BGTaskScheduler registrations from `registerTask`
            // calls during module init (#538). BGTaskScheduler enforces a
            // "register all handlers during launch" contract — if we miss
            // this window, late-registered identifiers won't be reachable
            // for the OS's first wake-up.
            crate::background::flush_pending_registrations();
            true
        }

        /// UIKit memory-warning delivery (#6184). This is the last notice
        /// before jetsam terminates the app, so treat it as critical: force a
        /// full collect and clamp the GC trigger. Runs on the main thread,
        /// which owns the JS arena, so calling into the runtime here is safe.
        #[unsafe(method(applicationDidReceiveMemoryWarning:))]
        fn application_did_receive_memory_warning(&self, _application: &AnyObject) {
            unsafe {
                js_gc_memory_pressure(2);
            }
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

        /// APNs rejected the registration. Logged to stderr.
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

        /// Foreground remote-notification payload.
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

        /// Background remote-notification payload (#98). UIKit always passes
        /// a non-null completion handler block; we forward it to the
        /// notifications module so the user's returned Promise can gate the
        /// `UIBackgroundFetchResult` signal.
        #[unsafe(method(application:didReceiveRemoteNotification:fetchCompletionHandler:))]
        fn did_receive_remote_notification_with_completion(
            &self,
            _app: &AnyObject,
            user_info: &AnyObject,
            completion: *mut AnyObject,
        ) {
            unsafe {
                crate::notifications::dispatch_remote_payload_with_completion(
                    user_info as *const _ as *mut AnyObject,
                    completion,
                );
            }
        }

        /// Custom URL scheme delivery (issue #583). Fires when the app is
        /// already running and the OS hands us a `myapp://…` URL.
        ///
        /// On scene-based apps the equivalent path is
        /// `scene(_:openURLContexts:)`; UIKit calls one or the other,
        /// never both, so wiring both surfaces is safe.
        #[unsafe(method(application:openURL:options:))]
        fn application_open_url(
            &self,
            _app: &AnyObject,
            url: &AnyObject,
            _options: &AnyObject,
        ) -> bool {
            unsafe {
                crate::deeplinks::dispatch_app_open_url(url as *const AnyObject);
            }
            true
        }

        /// Universal Link delivery (issue #583). The `userActivity` is an
        /// `NSUserActivity` whose `webpageURL` is the
        /// `https://yourdomain.com/path?…` link.
        #[unsafe(method(application:continueUserActivity:restorationHandler:))]
        fn application_continue_user_activity(
            &self,
            _app: &AnyObject,
            user_activity: &AnyObject,
            _restoration_handler: *const AnyObject,
        ) -> bool {
            unsafe {
                crate::deeplinks::dispatch_continue_user_activity(
                    user_activity as *const AnyObject,
                );
            }
            true
        }
    }
);

/// Scene delegate callback: creates the UIWindow and attaches the root widget.
/// Called by UIKit when the scene connects.
unsafe extern "C" fn scene_will_connect(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    scene: *mut AnyObject,
    _session: *mut AnyObject,
    options: *mut AnyObject,
) {
    // Issue #583: drain any cold-start URLs / Universal-Link activities
    // out of the scene-connection options BEFORE the JS module's
    // `appOnOpenUrl(...)` call has had a chance to register a handler.
    // The deeplinks module caches the URL until a handler arrives.
    crate::deeplinks::dispatch_scene_connection_options(options as *const AnyObject);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    // Create UIWindow attached to the scene (UIWindowScene)
    let window_cls = AnyClass::get(c"UIWindow").unwrap();
    let window_alloc: *mut AnyObject = msg_send![window_cls, alloc];
    let window_raw: *mut AnyObject = msg_send![window_alloc, initWithWindowScene: scene];
    let window: Retained<UIWindow> =
        Retained::cast_unchecked(Retained::retain(window_raw as *mut AnyObject).unwrap());

    // Create root PerryViewController (custom subclass with menu bar support).
    // Falls back to UIViewController if PerryViewController isn't registered yet.
    let vc_cls = AnyClass::get(c"PerryViewController")
        .unwrap_or_else(|| AnyClass::get(c"UIViewController").unwrap());
    let vc: Retained<UIViewController> = msg_send![vc_cls, new];

    // Set white background
    let white: Retained<AnyObject> = msg_send![AnyClass::get(c"UIColor").unwrap(), whiteColor];
    let vc_view: Retained<UIView> = msg_send![&*vc, view];
    let _: () = msg_send![&*vc_view, setBackgroundColor: &*white];

    // Attach the root widget if set
    PENDING_BODY.with(|b| {
        if let Some(root_handle) = b.borrow().as_ref() {
            if let Some(root_view) = widgets::get_widget(*root_handle) {
                let _: () =
                    msg_send![&*root_view, setTranslatesAutoresizingMaskIntoConstraints: false];

                vc_view.addSubview(&root_view);

                // Pin root widget to view edges (not safe area) for edge-to-edge layout
                let root_leading: *const AnyObject = msg_send![&*root_view, leadingAnchor];
                let root_trailing: *const AnyObject = msg_send![&*root_view, trailingAnchor];
                let root_top: *const AnyObject = msg_send![&*root_view, topAnchor];
                let root_bottom: *const AnyObject = msg_send![&*root_view, bottomAnchor];

                let view_leading: *const AnyObject = msg_send![&*vc_view, leadingAnchor];
                let view_trailing: *const AnyObject = msg_send![&*vc_view, trailingAnchor];
                let view_top: *const AnyObject = msg_send![&*vc_view, topAnchor];
                let view_bottom: *const AnyObject = msg_send![&*vc_view, bottomAnchor];

                let c1: Retained<AnyObject> =
                    msg_send![root_leading, constraintEqualToAnchor: view_leading];
                let c2: Retained<AnyObject> =
                    msg_send![root_trailing, constraintEqualToAnchor: view_trailing];
                let c3: Retained<AnyObject> =
                    msg_send![root_top, constraintEqualToAnchor: view_top];
                let c4: Retained<AnyObject> =
                    msg_send![root_bottom, constraintEqualToAnchor: view_bottom];

                let _: () = msg_send![&*c1, setActive: true];
                let _: () = msg_send![&*c2, setActive: true];
                let _: () = msg_send![&*c3, setActive: true];
                let _: () = msg_send![&*c4, setActive: true];

                // Store the bottom constraint so we can adjust it for keyboard avoidance
                ROOT_BOTTOM_CONSTRAINT.with(|rc| {
                    *rc.borrow_mut() = Some(c4);
                });
            }
        }
    });

    window.setRootViewController(Some(&vc));
    window.makeKeyAndVisible();

    // Store the view for keyboard avoidance
    let vc_view_ptr = Retained::as_ptr(&vc_view) as usize;
    KEYBOARD_VIEW.with(|kv| {
        *kv.borrow_mut() = Some(vc_view_ptr);
    });

    APPS.with(|a| {
        a.borrow_mut().push(AppEntry { window });
    });

    // Register for keyboard notifications
    register_keyboard_observers();

    // Start the timer pump to drive setInterval/setTimeout callbacks (8ms ≈ 120Hz).
    // Without this, js_interval_timer_tick() is never called and setInterval never fires.
    let pump_target = PerryPumpTarget::new();
    let pump_sel = Sel::register(c"pump:");
    let _: Retained<AnyObject> = msg_send![
        objc2::class!(NSTimer),
        scheduledTimerWithTimeInterval: 0.008f64,
        target: &*pump_target,
        selector: pump_sel,
        userInfo: std::ptr::null::<AnyObject>(),
        repeats: true
    ];
    std::mem::forget(pump_target);

    install_test_mode_exit_timer();
}

/// If `PERRY_UI_TEST_MODE=1`, schedule an NSTimer that writes a screenshot
/// (when `PERRY_UI_SCREENSHOT_PATH` is set) and exits cleanly. Mirrors the
/// hook perry-ui-macos installs; lets iOS doc-example programs be
/// verified under `xcrun simctl` in CI without a human.
unsafe fn install_test_mode_exit_timer() {
    if !perry_ui_testkit::is_test_mode() {
        return;
    }
    let delay_secs = perry_ui_testkit::exit_delay_ms() as f64 / 1000.0;
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

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(
        superclass: *const std::ffi::c_void,
        name: *const i8,
        extra_bytes: usize,
    ) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(
        cls: *mut std::ffi::c_void,
        sel: *const std::ffi::c_void,
        imp: *const std::ffi::c_void,
        types: *const i8,
    ) -> bool;
    fn class_addProtocol(cls: *mut std::ffi::c_void, protocol: *const std::ffi::c_void) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getProtocol(name: *const i8) -> *const std::ffi::c_void;
}

/// SceneDelegate `scene(_:openURLContexts:)` — custom-scheme delivery
/// while the app is running (issue #583).
unsafe extern "C" fn scene_open_url_contexts(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _scene: *mut AnyObject,
    contexts: *mut AnyObject,
) {
    crate::deeplinks::dispatch_scene_open_url_contexts(contexts as *const AnyObject);
}

/// SceneDelegate `scene(_:continueUserActivity:)` — Universal Link
/// delivery while the app is running (issue #583).
unsafe extern "C" fn scene_continue_user_activity(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    _scene: *mut AnyObject,
    activity: *mut AnyObject,
) {
    crate::deeplinks::dispatch_continue_user_activity(activity as *const AnyObject);
}

/// Register the PerrySceneDelegate class dynamically at runtime.
fn register_scene_delegate() {
    unsafe {
        let superclass = objc_getClass(c"UIResponder".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerrySceneDelegate".as_ptr(), 0);
        if cls.is_null() {
            // Class already registered
            return;
        }

        // Declare conformance to UIWindowSceneDelegate (which inherits UISceneDelegate).
        // iOS 26+ requires formal protocol conformance, not just method presence.
        let proto = objc_getProtocol(c"UIWindowSceneDelegate".as_ptr());
        if !proto.is_null() {
            class_addProtocol(cls, proto);
        }
        let proto2 = objc_getProtocol(c"UISceneDelegate".as_ptr());
        if !proto2.is_null() {
            class_addProtocol(cls, proto2);
        }

        // Add scene:willConnectToSession:options: method
        // ObjC type encoding: v@:@@@ (void, self, _cmd, scene, session, options)
        let sel = sel_registerName(c"scene:willConnectToSession:options:".as_ptr());
        class_addMethod(
            cls,
            sel,
            scene_will_connect as *const std::ffi::c_void,
            c"v@:@@@".as_ptr(),
        );

        // Issue #583: scene(_:openURLContexts:) — custom-scheme delivery
        // when the scene is already connected. Type encoding v@:@@.
        let sel_open = sel_registerName(c"scene:openURLContexts:".as_ptr());
        class_addMethod(
            cls,
            sel_open,
            scene_open_url_contexts as *const std::ffi::c_void,
            c"v@:@@".as_ptr(),
        );

        // Issue #583: scene(_:continueUserActivity:) — Universal Link
        // delivery when the scene is already connected. Type encoding v@:@@.
        let sel_continue = sel_registerName(c"scene:continueUserActivity:".as_ptr());
        class_addMethod(
            cls,
            sel_continue,
            scene_continue_user_activity as *const std::ffi::c_void,
            c"v@:@@".as_ptr(),
        );

        objc_registerClassPair(cls);
    }
}

// ─── PerryViewController ─────────────────────────────────────────────────────
// A custom UIViewController subclass that overrides:
//   - buildMenuWithBuilder: — populates the iPadOS menu bar via UIMenuBuilder
//   - perryMenuAction:      — dispatches UICommand/UIKeyCommand taps to JS callbacks

/// buildMenuWithBuilder: callback — forwards to menu::build_menubar_for_builder.
unsafe extern "C" fn vc_build_menu(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    builder: *mut AnyObject,
) {
    // Call super first so UIKit fills in system menus
    // (we can't easily call [super buildMenuWithBuilder:] from raw FFI,
    //  but UIViewController's default impl is a no-op, so skipping is fine.)
    menu::build_menubar_for_builder(builder);
}

/// perryMenuAction: callback — dispatches to menu::dispatch_menu_action.
unsafe extern "C" fn vc_perry_menu_action(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    sender: *mut AnyObject,
) {
    menu::dispatch_menu_action(sender);
}

/// canPerformAction:withSender: — return YES for perryMenuAction: so UIKit routes commands here.
unsafe extern "C" fn vc_can_perform_action(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    action: *const std::ffi::c_void,
    _sender: *mut AnyObject,
) -> bool {
    let perry_sel = sel_registerName(c"perryMenuAction:".as_ptr());
    action == perry_sel
}

/// Register the PerryViewController class dynamically at runtime.
fn register_view_controller() {
    unsafe {
        let superclass = objc_getClass(c"UIViewController".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerryViewController".as_ptr(), 0);
        if cls.is_null() {
            // Already registered
            return;
        }

        // buildMenuWithBuilder: — type encoding: v@:@ (void, self, _cmd, builder)
        let sel_build = sel_registerName(c"buildMenuWithBuilder:".as_ptr());
        class_addMethod(
            cls,
            sel_build,
            vc_build_menu as *const std::ffi::c_void,
            c"v@:@".as_ptr(),
        );

        // perryMenuAction: — type encoding: v@:@ (void, self, _cmd, sender)
        let sel_action = sel_registerName(c"perryMenuAction:".as_ptr());
        class_addMethod(
            cls,
            sel_action,
            vc_perry_menu_action as *const std::ffi::c_void,
            c"v@:@".as_ptr(),
        );

        // canPerformAction:withSender: — type encoding: B@::@ (BOOL, self, _cmd, action, sender)
        let sel_can = sel_registerName(c"canPerformAction:withSender:".as_ptr());
        class_addMethod(
            cls,
            sel_can,
            vc_can_perform_action as *const std::ffi::c_void,
            c"B@::@".as_ptr(),
        );

        objc_registerClassPair(cls);
    }
}

/// Run the iOS app event loop (calls UIApplicationMain, blocks forever).
pub fn app_run(_app_handle: i64) {
    // Install crash reporting hooks before anything else
    crate::crash_log::install_crash_hooks();

    // Phase 2 v3.3: register cross-platform showToast / setText handlers
    // so `showToast("…")` and `setText(id, val)` produce visible UI on iOS.
    register_cross_platform_text_handlers();

    // Force PerryAppDelegate class registration (define_class! registers it lazily)
    let _ = PerryAppDelegate::class();

    // Register PerrySceneDelegate dynamically before UIApplicationMain
    register_scene_delegate();

    // Register PerryViewController (UIViewController + menu bar support)
    register_view_controller();

    // Issue #1864: install continuous keyboard event hooks
    // (`pressesBegan:`/`pressesEnded:`/`canBecomeFirstResponder`) on the
    // PerryViewController class. The first onKeyDown/onKeyUp registration
    // also calls `make_first_responder` so events actually route here.
    crate::keyboard::install_view_controller_overrides();

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

    unsafe {
        let argc = 0i32;
        let argv: *const *const u8 = std::ptr::null();
        let principal = std::ptr::null::<NSString>();
        let delegate_class_name = NSString::from_str("PerryAppDelegate");

        // UIApplicationMain(argc, argv, nil, @"PerryAppDelegate")
        extern "C" {
            fn UIApplicationMain(
                argc: i32,
                argv: *const *const u8,
                principalClassName: *const NSString,
                delegateClassName: *const NSString,
            ) -> i32;
        }

        UIApplicationMain(argc, argv, principal, &*delegate_class_name);
    }
}

/// Set minimum window size (no-op on iOS — windows are always full-screen).
pub fn set_min_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on iOS
}

/// Set maximum window size (no-op on iOS — windows are always full-screen).
pub fn set_max_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on iOS
}

/// Add a keyboard shortcut (stub on iOS — UIKeyCommand not yet implemented).
pub fn add_keyboard_shortcut(_key_ptr: *const u8, _modifiers: f64, _callback: f64) {
    // No-op on iOS for now
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
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
    fn js_frame_pump_default() -> i32;
    // perry-runtime's embedder GC stepper: spends up to `budget_us`
    // advancing an ACTIVE budgeted collection in bounded work units; a
    // cheap status probe when no cycle is active (out=null is allowed).
    fn js_gc_step_us(budget_us: u64, out: *mut u8) -> u32;
    // perry-runtime's OS-memory-pressure entry point (#6184): level 1 =
    // collect-if-safe (minor), level 2+ = full collect + clamp trigger.
    // Always lowers+arms the arena trigger so an unsafe delivery still
    // collects at the next allocation.
    fn js_gc_memory_pressure(level: u32) -> u32;
}

// ============================================
// Timer Pump — drives setTimeout/setInterval callbacks
// ============================================

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
                unsafe {
                    js_callback_timer_tick();
                    js_interval_timer_tick();
                    // Issue #1865: perry/ui `onFrame` display-link callbacks.
                    js_frame_pump_default();
                    js_promise_run_microtasks();
                    // Drain deferred promise resolutions from perry-stdlib
                    // tokio workers (async fetch/network completions). Without
                    // this, `await fetch(...)` never resolves on iOS — the
                    // macOS pump has always called it; iOS omitted it.
                    js_run_stdlib_pump();
                    // #6183 (2026-07-09 GC audit): spend a bounded idle
                    // budget on any active budgeted GC cycle at the pump
                    // boundary — the JS stack is fully unwound here, so this
                    // is a precise-root safepoint. A 2 ms slice per tick
                    // drains collection debt incrementally instead of letting
                    // an alloc-point collection land unbounded on this
                    // (main/UI) thread mid-interaction.
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

// ============================================
// Test Mode Exit — PERRY_UI_TEST_MODE auto-exit hook
// ============================================

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
            // Drain resolved promises, then run microtasks (.then callbacks)
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
        }
    }
);

impl PerryTimerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTimerTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ============================================
// Keyboard Avoidance
// ============================================

thread_local! {
    static KEYBOARD_VIEW: RefCell<Option<usize>> = RefCell::new(None);
}

/// Register for UIKeyboard notifications to adjust the root view when the keyboard appears.
fn register_keyboard_observers() {
    unsafe {
        let nc: *const AnyObject = msg_send![
            AnyClass::get(c"NSNotificationCenter").unwrap(),
            defaultCenter
        ];

        // Register the PerryKeyboardObserver class
        let observer = PerryKeyboardObserver::new();

        // UIKeyboardWillChangeFrameNotification
        let notif_name = NSString::from_str("UIKeyboardWillChangeFrameNotification");
        let sel = Sel::register(c"keyboardWillChangeFrame:");
        let _: () = msg_send![
            nc,
            addObserver: &*observer,
            selector: sel,
            name: &*notif_name,
            object: std::ptr::null::<AnyObject>()
        ];

        std::mem::forget(observer); // keep alive
    }
}

pub struct PerryKeyboardObserverIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryKeyboardObserver"]
    #[ivars = PerryKeyboardObserverIvars]
    pub struct PerryKeyboardObserver;

    impl PerryKeyboardObserver {
        #[unsafe(method(keyboardWillChangeFrame:))]
        fn keyboard_will_change_frame(&self, notification: &AnyObject) {
            unsafe {
                // Get keyboard frame from notification userInfo
                let user_info: *const AnyObject = msg_send![notification, userInfo];
                if user_info.is_null() { return; }

                let frame_key = NSString::from_str("UIKeyboardFrameEndUserInfoKey");
                let frame_value: *const AnyObject = msg_send![user_info, objectForKey: &*frame_key];
                if frame_value.is_null() { return; }

                let kbd_frame: objc2_core_foundation::CGRect = msg_send![frame_value, CGRectValue];

                // Get animation duration
                let duration_key = NSString::from_str("UIKeyboardAnimationDurationUserInfoKey");
                let duration_value: *const AnyObject = msg_send![user_info, objectForKey: &*duration_key];
                let duration: f64 = if !duration_value.is_null() {
                    msg_send![duration_value, doubleValue]
                } else {
                    0.25
                };

                // Get the screen height to determine if keyboard is showing or hiding
                let screen: *const AnyObject = msg_send![
                    AnyClass::get(c"UIScreen").unwrap(),
                    mainScreen
                ];
                let screen_bounds: objc2_core_foundation::CGRect = msg_send![screen, bounds];
                let screen_height = screen_bounds.size.height;

                // Keyboard height: if kbd_frame.origin.y >= screen_height, keyboard is hidden
                let keyboard_height = if kbd_frame.origin.y >= screen_height {
                    0.0
                } else {
                    screen_height - kbd_frame.origin.y
                };

                // Adjust the root bottom constraint
                ROOT_BOTTOM_CONSTRAINT.with(|rc| {
                    if let Some(ref constraint) = *rc.borrow() {
                        // Negative constant because bottom constraint is root.bottom == view.bottom + constant
                        let _: () = msg_send![&**constraint, setConstant: -keyboard_height];
                    }
                });

                // Animate the layout change
                KEYBOARD_VIEW.with(|kv| {
                    if let Some(view_ptr) = *kv.borrow() {
                        let view = view_ptr as *const AnyObject;

                        // Get animation curve
                        let curve_key = NSString::from_str("UIKeyboardAnimationCurveUserInfoKey");
                        let curve_value: *const AnyObject = msg_send![user_info, objectForKey: &*curve_key];
                        let curve: u64 = if !curve_value.is_null() {
                            msg_send![curve_value, unsignedIntegerValue]
                        } else {
                            7 // UIViewAnimationCurveKeyboard (undocumented but standard)
                        };

                        // UIView.animateWithDuration:delay:options:animations:completion:
                        // options: curve << 16 to convert UIViewAnimationCurve to UIViewAnimationOptions
                        let options = curve << 16;
                        let view_copy = view;

                        // Use block-based animation
                        let animation_block = block2::RcBlock::new(move || {
                            let _: () = msg_send![view_copy, layoutIfNeeded];
                        });

                        let _: () = msg_send![
                            AnyClass::get(c"UIView").unwrap(),
                            animateWithDuration: duration,
                            delay: 0.0f64,
                            options: options,
                            animations: &*animation_block,
                            completion: std::ptr::null::<AnyObject>()
                        ];

                        // Also scroll the focused text field into view
                        scroll_focused_field_into_view(keyboard_height);
                    }
                });
            }
        }
    }
);

impl PerryKeyboardObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryKeyboardObserverIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Find the currently focused UITextField/UISecureTextField and scroll its
/// parent UIScrollView so the field is visible above the keyboard.
unsafe fn scroll_focused_field_into_view(keyboard_height: f64) {
    if keyboard_height <= 0.0 {
        return;
    }

    // Find the first responder
    let app: *const AnyObject =
        msg_send![AnyClass::get(c"UIApplication").unwrap(), sharedApplication];
    let key_window: *const AnyObject = msg_send![app, keyWindow];
    if key_window.is_null() {
        return;
    }

    // Use a private but widely-used method to find first responder
    // Alternatively, walk the view hierarchy
    let first_responder: *const AnyObject = find_first_responder(key_window as *const AnyObject);
    if first_responder.is_null() {
        return;
    }

    // Check if it's a text field
    let tf_cls = AnyClass::get(c"UITextField").unwrap();
    let is_tf: bool = msg_send![first_responder, isKindOfClass: tf_cls];
    if !is_tf {
        return;
    }

    // Find parent UIScrollView
    let mut parent: *const AnyObject = msg_send![first_responder, superview];
    let scroll_cls = AnyClass::get(c"UIScrollView").unwrap();
    while !parent.is_null() {
        let is_scroll: bool = msg_send![parent, isKindOfClass: scroll_cls];
        if is_scroll {
            // Convert the text field's frame to the scroll view's coordinate space
            let tf_frame: objc2_core_foundation::CGRect = msg_send![first_responder, frame];
            let tf_superview: *const AnyObject = msg_send![first_responder, superview];
            let converted: objc2_core_foundation::CGRect = msg_send![
                parent,
                convertRect: tf_frame,
                fromView: tf_superview
            ];

            // Calculate visible area (scroll view height minus keyboard)
            let scroll_bounds: objc2_core_foundation::CGRect = msg_send![parent, bounds];
            let visible_height = scroll_bounds.size.height - keyboard_height;

            // If the text field is below the visible area, scroll to it
            let tf_bottom = converted.origin.y + converted.size.height + 20.0; // 20px padding
            let scroll_offset: objc2_core_foundation::CGPoint = msg_send![parent, contentOffset];

            if tf_bottom > scroll_offset.y + visible_height {
                let new_offset = tf_bottom - visible_height;
                let point = objc2_core_foundation::CGPoint::new(0.0, new_offset);
                let _: () = msg_send![parent, setContentOffset: point, animated: true];
            }

            break;
        }
        parent = msg_send![parent, superview];
    }
}

/// Recursively find the first responder in the view hierarchy.
unsafe fn find_first_responder(view: *const AnyObject) -> *const AnyObject {
    let is_first: bool = msg_send![view, isFirstResponder];
    if is_first {
        return view;
    }

    let subviews: *const AnyObject = msg_send![view, subviews];
    let count: usize = msg_send![subviews, count];
    for i in 0..count {
        let subview: *const AnyObject = msg_send![subviews, objectAtIndex: i];
        let result = find_first_responder(subview);
        if !result.is_null() {
            return result;
        }
    }

    std::ptr::null()
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

// =============================================================================
// Cross-platform showToast / setText handler registration (Phase 2 v3.3)
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
        js_register_show_toast_handler(widgets::toast::show_toast_handler);
        js_register_set_text_handler(widgets::text_registry::set_text_handler);
        js_register_text_id_handler(widgets::text_registry::register_text_id_handler);
        js_register_widget_hidden_handler(navstack_set_widget_hidden);
    }
}
