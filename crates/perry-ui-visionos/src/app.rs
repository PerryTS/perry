use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_foundation::{NSObject, NSString};
use objc2_ui_kit::UIView;

use std::cell::RefCell;
use std::collections::HashMap;

use crate::widgets;
use crate::menu;

thread_local! {
    static PENDING_CONFIG: RefCell<Option<AppConfig>> = RefCell::new(None);
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
    /// The bottom constraint of the root widget, adjusted when keyboard appears/disappears
    static ROOT_BOTTOM_CONSTRAINT: RefCell<Option<Retained<AnyObject>>> = RefCell::new(None);
    static RUNTIME_STARTED: RefCell<bool> = const { RefCell::new(false) };
}

struct AppConfig {
    title: String,
    _width: f64,
    _height: f64,
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

fn ensure_runtime_services_started() {
    let already_started = RUNTIME_STARTED.with(|started| {
        let mut started = started.borrow_mut();
        if *started {
            true
        } else {
            *started = true;
            false
        }
    });
    if already_started {
        return;
    }

    register_keyboard_observers();

    let pump_target = PerryPumpTarget::new();
    let pump_sel = Sel::register(c"pump:");
    let _: Retained<AnyObject> = unsafe {
        msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.008f64,
            target: &*pump_target,
            selector: pump_sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ]
    };
    std::mem::forget(pump_target);
}

fn make_fallback_root_view() -> Retained<UIView> {
    unsafe {
        let label_cls = AnyClass::get(c"UILabel").unwrap();
        let label_obj: Retained<AnyObject> = msg_send![label_cls, new];
        let label: Retained<UIView> = Retained::cast_unchecked(label_obj);
        let title = PENDING_CONFIG.with(|c| {
            c.borrow()
                .as_ref()
                .map(|cfg| cfg.title.clone())
                .unwrap_or_else(|| "Perry visionOS App".to_string())
        });
        let ns_title = NSString::from_str(&title);
        let _: () = msg_send![&*label, setText: &*ns_title];
        label
    }
}

#[no_mangle]
pub extern "C" fn perry_visionos_root_view() -> i64 {
    ensure_runtime_services_started();

    let view = PENDING_BODY.with(|b| {
        b.borrow()
            .as_ref()
            .and_then(|root_handle| widgets::get_widget(*root_handle))
    }).unwrap_or_else(make_fallback_root_view);

    KEYBOARD_VIEW.with(|kv| {
        *kv.borrow_mut() = Some(Retained::as_ptr(&view) as usize);
    });

    let ptr = Retained::as_ptr(&view) as i64;
    std::mem::forget(view);
    ptr
}

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(superclass: *const std::ffi::c_void, name: *const i8, extra_bytes: usize) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(cls: *mut std::ffi::c_void, sel: *const std::ffi::c_void, imp: *const std::ffi::c_void, types: *const i8) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
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

/// visionOS uses a SwiftUI host app; `perry_main_init` should not enter
/// UIApplicationMain itself. The Swift host calls back into
/// `perry_visionos_root_view()` to embed the UIKit tree.
pub fn app_run(_app_handle: i64) {
    crate::crash_log::install_crash_hooks();
    register_view_controller();
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
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
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
                    js_promise_run_microtasks();
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
                js_stdlib_process_pending();
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
    if keyboard_height <= 0.0 { return; }

    // Find the first responder
    let app: *const AnyObject = msg_send![
        AnyClass::get(c"UIApplication").unwrap(),
        sharedApplication
    ];
    let key_window: *const AnyObject = msg_send![app, keyWindow];
    if key_window.is_null() { return; }

    // Use a private but widely-used method to find first responder
    // Alternatively, walk the view hierarchy
    let first_responder: *const AnyObject = find_first_responder(key_window as *const AnyObject);
    if first_responder.is_null() { return; }

    // Check if it's a text field
    let tf_cls = AnyClass::get(c"UITextField").unwrap();
    let is_tf: bool = msg_send![first_responder, isKindOfClass: tf_cls];
    if !is_tf { return; }

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
    if is_first { return view; }

    let subviews: *const AnyObject = msg_send![view, subviews];
    let count: usize = msg_send![subviews, count];
    for i in 0..count {
        let subview: *const AnyObject = msg_send![subviews, objectAtIndex: i];
        let result = find_first_responder(subview);
        if !result.is_null() { return result; }
    }

    std::ptr::null()
}

/// Set a recurring timer. interval_ms is in milliseconds.
/// The timer calls js_stdlib_process_pending() then invokes the callback.
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
