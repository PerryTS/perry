//! Window chrome and appearance: dock icon, frameless mode, window level,
//! transparency, vibrancy and activation policy.
//!
//! Split out of `app.rs` to keep it under the 2000-line cap once the Servo
//! webview code landed. These are all plain setters over `APPS` and the
//! pending-state thread-locals, so they live as a child module and reach
//! app.rs's private helpers (`str_from_header`, the `PENDING_*` cells)
//! through `use super::*`.

use super::*;

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
                    fn class_addMethod(
                        cls: *mut std::ffi::c_void,
                        sel: *const std::ffi::c_void,
                        imp: extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i8,
                        types: *const i8,
                    ) -> i8;
                    fn object_setClass(
                        obj: *mut std::ffi::c_void,
                        cls: *mut std::ffi::c_void,
                    ) -> *mut std::ffi::c_void;
                    fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
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
                        class_addMethod(cls, sel, can_become_key, c"B@:".as_ptr());
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
