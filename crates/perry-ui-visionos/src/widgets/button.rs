use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::{UIButton, UIView};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static BUTTON_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    // dispatch_get_main_queue() is a macro; the actual symbol is _dispatch_main_q
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

unsafe extern "C" fn button_callback_trampoline(context: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let closure_f64 = f64::from_bits(context as u64);
        let closure_ptr = js_nanbox_get_pointer(closure_f64);
        js_closure_call0(closure_ptr as *const u8);
    });
}

pub struct PerryButtonTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryButtonTarget"]
    #[ivars = PerryButtonTargetIvars]
    pub struct PerryButtonTarget;

    impl PerryButtonTarget {
        #[unsafe(method(buttonPressed:))]
        fn button_pressed(&self, _sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            BUTTON_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    // Dispatch async to avoid modifying the view hierarchy during
                    // UIKit touch event processing (crashes on iOS 26+).
                    unsafe {
                        dispatch_async_f(
                            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                            closure_f64.to_bits() as *mut std::ffi::c_void,
                            button_callback_trampoline,
                        );
                    }
                }
            });
        }
    }
);

impl PerryButtonTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryButtonTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
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

/// Create a UIButton with a label and closure callback.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);

    unsafe {
        // UIButton.buttonWithType: 0 = UIButtonTypeCustom, 1 = UIButtonTypeSystem
        let button: Retained<UIButton> = msg_send![
            objc2::runtime::AnyClass::get(c"UIButton").unwrap(),
            buttonWithType: 1i64  // UIButtonTypeSystem
        ];

        let ns_string = NSString::from_str(label);
        let _: () = msg_send![&*button, setTitle: &*ns_string, forState: 0u64]; // UIControlStateNormal = 0
        let _: () = msg_send![&*button, setAccessibilityLabel: &*ns_string];

        let _: () = msg_send![&*button, setTranslatesAutoresizingMaskIntoConstraints: false];

        let target = PerryButtonTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        BUTTON_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_press);
        });

        let sel = Sel::register(c"buttonPressed:");
        // addTarget:action:forControlEvents: UIControlEventTouchUpInside = 1 << 6 = 64
        let _: () = msg_send![&*button, addTarget: &*target, action: sel, forControlEvents: 64u64];

        std::mem::forget(target);

        let view: Retained<UIView> = Retained::cast_unchecked(button);
        let handle = super::register_widget(view);
        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            unsafe { perry_geisterhand_register(handle, 0, 0, on_press, label_ptr); }
        }
        handle
    }
}

/// Set whether a button has a border (approximated via layer).
pub fn set_bordered(handle: i64, bordered: bool) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let layer: *const AnyObject = msg_send![&*view, layer];
            if !layer.is_null() {
                if bordered {
                    let _: () = msg_send![layer, setBorderWidth: 1.0f64];
                    let color: *const AnyObject = msg_send![
                        objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
                        systemBlueColor
                    ];
                    let cg_color: *const AnyObject = msg_send![color, CGColor];
                    let _: () = msg_send![layer, setBorderColor: cg_color];
                    let _: () = msg_send![layer, setCornerRadius: 5.0f64];
                } else {
                    let _: () = msg_send![layer, setBorderWidth: 0.0f64];
                }
            }
        }
    }
}

/// Set the text color of a button.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color: Retained<AnyObject> = msg_send![
                objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r,
                green: g,
                blue: b,
                alpha: a
            ];
            // setTitleColor:forState: UIControlStateNormal = 0
            let _: () = msg_send![&*view, setTitleColor: &*color, forState: 0u64];
        }
    }
}

/// Set the title text of a button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_title = NSString::from_str(title);
        unsafe {
            let _: () = msg_send![&*view, setTitle: &*ns_title, forState: 0u64];
        }
    }
}

/// Set an SF Symbol image on a UIButton.
pub fn set_image(handle: i64, name_ptr: *const u8) {
    let name = str_from_header(name_ptr);
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let ns_name = NSString::from_str(name);
            // UIImage.systemImageNamed:
            let img_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
            let img: *mut AnyObject = msg_send![img_cls, systemImageNamed: &*ns_name];
            if !img.is_null() {
                // Apply large symbol configuration
                let config_cls = objc2::runtime::AnyClass::get(c"UIImageSymbolConfiguration");
                if let Some(config_cls) = config_cls {
                    // UIImageSymbolScale: 1=small, 2=medium, 3=large
                    let config: *mut AnyObject = msg_send![
                        config_cls, configurationWithScale: 3_i64
                    ];
                    if !config.is_null() {
                        let scaled_img: *mut AnyObject = msg_send![img, imageWithConfiguration: config];
                        if !scaled_img.is_null() {
                            let _: () = msg_send![&*view, setImage: scaled_img, forState: 0_u64];
                        } else {
                            let _: () = msg_send![&*view, setImage: img, forState: 0_u64];
                        }
                    } else {
                        let _: () = msg_send![&*view, setImage: img, forState: 0_u64];
                    }
                } else {
                    let _: () = msg_send![&*view, setImage: img, forState: 0_u64];
                }
            }
        }
    }
}

/// Set the image position of a UIButton (no-op on iOS — UIButton handles layout differently).
pub fn set_image_position(_handle: i64, _position: i64) {
    // iOS UIButton doesn't have NSImagePosition.
    // Image placement is controlled by configuration or content edge insets.
    // No-op for compatibility.
}

/// Set the tint color of a UIButton (affects SF Symbol icon color).
pub fn set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color: objc2::rc::Retained<AnyObject> = msg_send![
                objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r,
                green: g,
                blue: b,
                alpha: a
            ];
            let _: () = msg_send![&*view, setTintColor: &*color];
        }
    }
}

thread_local! {
    static TAP_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

pub struct PerryTapTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTapTarget"]
    #[ivars = PerryTapTargetIvars]
    pub struct PerryTapTarget;

    impl PerryTapTarget {
        #[unsafe(method(handleTap:))]
        fn handle_tap(&self, _sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            TAP_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    unsafe {
                        dispatch_async_f(
                            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
                            closure_f64.to_bits() as *mut std::ffi::c_void,
                            button_callback_trampoline,
                        );
                    }
                }
            });
        }
    }
);

impl PerryTapTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTapTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Attach a single-tap gesture recognizer to any widget view.
pub fn set_on_tap(handle: i64, callback: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let target = PerryTapTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            TAP_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            let sel = Sel::register(c"handleTap:");
            let gr_cls = objc2::runtime::AnyClass::get(c"UITapGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = msg_send![
                recognizer, initWithTarget: &*target, action: sel
            ];
            let _: () = msg_send![recognizer, setNumberOfTapsRequired: 1i64];
            let _: () = msg_send![&*view, setUserInteractionEnabled: true];
            let _: () = msg_send![&*view, addGestureRecognizer: recognizer];

            std::mem::forget(target);
        }
    }
}
