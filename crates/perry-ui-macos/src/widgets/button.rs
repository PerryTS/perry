use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSButton, NSView};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from button target object address to closure pointer (f64 NaN-boxed)
    static BUTTON_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Internal state for our button target
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
            crate::catch_callback_panic("button callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = BUTTON_CALLBACKS.with(|cbs| {
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

impl PerryButtonTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryButtonTargetIvars {
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

/// Create an NSButton with a label and closure callback.
/// `label_ptr` is a StringHeader pointer, `on_press` is a NaN-boxed closure pointer.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_string = NSString::from_str(label);

    unsafe {
        let button = NSButton::buttonWithTitle_target_action(&ns_string, None, None, mtm);
        let _: () = msg_send![&*button, setAccessibilityLabel: &*ns_string];

        // Create our target object and wire it up
        let target = PerryButtonTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        // Store the closure callback
        BUTTON_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_press);
        });

        // Set target and action
        let sel = Sel::register(c"buttonPressed:");
        button.setTarget(Some(&target));
        button.setAction(Some(sel));

        // Prevent target from being deallocated (leak the Retained reference)
        std::mem::forget(target);

        let view: Retained<NSView> = Retained::cast_unchecked(button);
        let handle = super::register_widget(view);
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            perry_geisterhand_register(handle, 0, 0, on_press, label_ptr);
        }
        handle
    }
}

/// Set whether a button has a border.
/// When removing the border, re-applies the button's attributed title to preserve text color.
/// Without this, borderless NSButtons render invisible text in dark mode.
pub fn set_bordered(handle: i64, bordered: bool) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);
            // Save attributed title before style change
            let attr_title: *const AnyObject = msg_send![btn, attributedTitle];
            btn.setBordered(bordered);
            if !bordered {
                // Set plain bezel style to avoid visual artifacts
                let _: () = msg_send![btn, setBezelStyle: 0i64]; // NSBezelStyleRounded = 0, avoids shadow
                                                                 // Re-apply attributed title (setBordered resets it)
                if !attr_title.is_null() {
                    let _: () = msg_send![btn, setAttributedTitle: attr_title];
                }
            }
        }
    }
}

/// Set the text color of a button.
/// Uses both NSAttributedString (for bordered buttons) and contentTintColor (for borderless).
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);

            // Create NSColor
            let color: Retained<AnyObject> = msg_send![
                AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];

            // Set contentTintColor — works for borderless buttons in dark mode
            let _: () = msg_send![btn, setContentTintColor: &*color];

            // Also set attributed string for bordered buttons
            let key = NSString::from_str("NSColor");
            let attrs: Retained<AnyObject> = msg_send![
                AnyClass::get(c"NSDictionary").unwrap(),
                dictionaryWithObject: &*color,
                forKey: &*key
            ];

            let title = btn.title();
            let ns_title: *const AnyObject = Retained::as_ptr(&title) as *const AnyObject;
            let cls = AnyClass::get(c"NSAttributedString").unwrap();
            let alloc: *mut AnyObject = msg_send![cls, alloc];
            let attr_str: *mut AnyObject = msg_send![
                alloc,
                initWithString: ns_title,
                attributes: &*attrs
            ];

            let _: () = msg_send![btn, setAttributedTitle: attr_str];
        }
    }
}

/// Set an SF Symbol image on a button with a large point size.
pub fn set_image(handle: i64, name_ptr: *const u8) {
    let name = str_from_header(name_ptr);
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);
            let ns_name = NSString::from_str(name);
            // NSImage.imageWithSystemSymbolName:accessibilityDescription:
            let img_cls = AnyClass::get(c"NSImage").unwrap();
            let img: *mut AnyObject = msg_send![
                img_cls,
                imageWithSystemSymbolName: &*ns_name,
                accessibilityDescription: std::ptr::null::<AnyObject>()
            ];
            if !img.is_null() {
                // Apply large symbol scale
                // NSImageSymbolScale: 1=small, 2=medium, 3=large
                let config_cls = AnyClass::get(c"NSImageSymbolConfiguration").unwrap();
                let config: *mut AnyObject = msg_send![
                    config_cls,
                    configurationWithScale: 3_isize  // NSImageSymbolScaleLarge
                ];
                if !config.is_null() {
                    let sized_img: *mut AnyObject =
                        msg_send![img, imageWithSymbolConfiguration: config];
                    if !sized_img.is_null() {
                        let _: () = msg_send![btn, setImage: sized_img];
                    } else {
                        let _: () = msg_send![btn, setImage: img];
                    }
                } else {
                    let _: () = msg_send![btn, setImage: img];
                }
                // NSImageLeading = 7 (icon before text, respects layout direction)
                let _: () = msg_send![btn, setImagePosition: 7_isize];
            }
        }
    }
}

/// Set the image position of a button.
/// 0=NSNoImage, 1=NSImageOnly, 2=NSImageLeft, 3=NSImageRight,
/// 4=NSImageBelow, 5=NSImageAbove, 6=NSImageOverlaps, 7=NSImageLeading, 8=NSImageTrailing
pub fn set_image_position(handle: i64, position: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);
            let _: () = msg_send![btn, setImagePosition: position as isize];
        }
    }
}

/// Set the content tint color of a button (affects SF Symbol icon color).
pub fn set_content_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);
            let color: Retained<AnyObject> = msg_send![
                AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            let _: () = msg_send![btn, setContentTintColor: &*color];
        }
    }
}

/// Set the title text of a button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_title = NSString::from_str(title);
        unsafe {
            let btn: &NSButton = &*(Retained::as_ptr(&view) as *const NSButton);
            btn.setTitle(&ns_title);
        }
    }
}
