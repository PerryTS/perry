use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::{UITextField, UIView};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TEXTFIELD_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

pub struct PerryTextFieldTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldTarget"]
    #[ivars = PerryTextFieldTargetIvars]
    pub struct PerryTextFieldTarget;

    impl PerryTextFieldTarget {
        #[unsafe(method(textFieldChanged:))]
        fn text_field_changed(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            TEXTFIELD_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    let text: Retained<NSString> = unsafe { msg_send![sender, text] };
                    let rust_str = text.to_string();
                    let bytes = rust_str.as_bytes();

                    let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                    let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };

                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call1(closure_ptr as *const u8, nanboxed);
                    }
                }
            });
        }
    }
);

impl PerryTextFieldTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldTargetIvars {
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

/// Create a UITextField with placeholder and onChange callback.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);

    unsafe {
        let text_field: Retained<UITextField> = msg_send![
            objc2::runtime::AnyClass::get(c"UITextField").unwrap(),
            new
        ];
        let ns_placeholder = NSString::from_str(placeholder);
        let _: () = msg_send![&*text_field, setPlaceholder: &*ns_placeholder];
        let _: () = msg_send![&*text_field, setAccessibilityLabel: &*ns_placeholder];
        let _: () = msg_send![&*text_field, setBorderStyle: 3i64]; // UITextBorderStyleRoundedRect = 3
        let _: () = msg_send![&*text_field, setTranslatesAutoresizingMaskIntoConstraints: false];

        let target = PerryTextFieldTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        TEXTFIELD_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"textFieldChanged:");
        // UIControlEventEditingChanged = 1 << 17 = 131072
        let _: () = msg_send![&*text_field, addTarget: &*target, action: sel, forControlEvents: 131072u64];

        std::mem::forget(target);

        let view: Retained<UIView> = Retained::cast_unchecked(text_field);
        let handle = super::register_widget(view);
        #[cfg(feature = "geisterhand")]
        {
            extern "C" { fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8); }
            unsafe { perry_geisterhand_register(handle, 1, 1, on_change, placeholder_ptr); }
        }
        handle
    }
}

/// Focus a UITextField (make it first responder).
pub fn focus(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, becomeFirstResponder];
        }
    }
}

/// Set the text of a UITextField from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns_string];
        }
    }
}

/// Set the text of a UITextField from a Rust string slice.
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns_string];
        }
    }
}

/// Get the current string value from a UITextField, returns a StringHeader pointer.
pub fn get_string_value(handle: i64) -> *const u8 {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let text: Retained<NSString> = msg_send![&*view, text];
            let rust_str = text.to_string();
            let bytes = rust_str.as_bytes();
            return js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        }
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

/// Set whether the text field is borderless (0 = bordered, 1 = borderless).
pub fn set_borderless(handle: i64, borderless: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            if borderless > 0.5 {
                // UITextBorderStyleNone = 0
                let _: () = msg_send![&*view, setBorderStyle: 0i64];
            } else {
                // UITextBorderStyleRoundedRect = 3
                let _: () = msg_send![&*view, setBorderStyle: 3i64];
            }
        }
    }
}

/// Set the background color of the text field.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let cls = objc2::runtime::AnyClass::get(c"UIColor").unwrap();
            let color: Retained<AnyObject> = msg_send![
                cls,
                colorWithRed: r as f64,
                green: g as f64,
                blue: b as f64,
                alpha: a as f64
            ];
            let _: () = msg_send![&*view, setBackgroundColor: &*color];
        }
    }
}

/// Set the font size of the text field.
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let cls = objc2::runtime::AnyClass::get(c"UIFont").unwrap();
            let font: Retained<AnyObject> = msg_send![cls, systemFontOfSize: size as f64];
            let _: () = msg_send![&*view, setFont: &*font];
        }
    }
}

/// Set the text color of the text field.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let cls = objc2::runtime::AnyClass::get(c"UIColor").unwrap();
            let color: Retained<AnyObject> = msg_send![
                cls,
                colorWithRed: r as f64,
                green: g as f64,
                blue: b as f64,
                alpha: a as f64
            ];
            let _: () = msg_send![&*view, setTextColor: &*color];
        }
    }
}

thread_local! {
    static SUBMIT_CALLBACKS: RefCell<HashMap<usize, (f64, *const objc2::runtime::AnyObject)>> = RefCell::new(HashMap::new());
}

pub struct PerryTextFieldSubmitTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldSubmitTarget"]
    #[ivars = PerryTextFieldSubmitTargetIvars]
    pub struct PerryTextFieldSubmitTarget;

    impl PerryTextFieldSubmitTarget {
        #[unsafe(method(textFieldDidReturn:))]
        fn text_field_did_return(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            SUBMIT_CALLBACKS.with(|cbs| {
                if let Some(&(closure_f64, _tf_ptr)) = cbs.borrow().get(&key) {
                    // Get the text
                    let text: Retained<NSString> = unsafe { msg_send![sender, text] };
                    let rust_str = text.to_string();
                    let bytes = rust_str.as_bytes();
                    let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                    let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call1(closure_ptr as *const u8, nanboxed);
                    }
                }
            });
        }
    }
);

impl PerryTextFieldSubmitTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldSubmitTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set an onSubmit callback (fires when user presses Return).
pub fn set_on_submit(handle: i64, on_submit: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let target = PerryTextFieldSubmitTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            let tf_raw = Retained::as_ptr(&view) as *const objc2::runtime::AnyObject;
            SUBMIT_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, (on_submit, tf_raw));
            });

            let sel = Sel::register(c"textFieldDidReturn:");
            // UIControlEventEditingDidEndOnExit = 1 << 19 = 524288
            let _: () = msg_send![&*view, addTarget: &*target, action: sel, forControlEvents: 524288_u64];

            std::mem::forget(target);
        }
    }
}
