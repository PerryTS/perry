use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::{UITextField, UIView};
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SECUREFIELD_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

pub struct PerrySecureFieldTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerrySecureFieldTarget"]
    #[ivars = PerrySecureFieldTargetIvars]
    pub struct PerrySecureFieldTarget;

    impl PerrySecureFieldTarget {
        #[unsafe(method(textFieldChanged:))]
        fn text_field_changed(&self, sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            SECUREFIELD_CALLBACKS.with(|cbs| {
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

impl PerrySecureFieldTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerrySecureFieldTargetIvars {
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

/// Create a UITextField with secureTextEntry enabled (password field).
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);

    unsafe {
        let text_field: Retained<UITextField> = msg_send![
            objc2::runtime::AnyClass::get(c"UITextField").unwrap(),
            new
        ];
        let ns_placeholder = NSString::from_str(placeholder);
        let _: () = msg_send![&*text_field, setPlaceholder: &*ns_placeholder];
        let _: () = msg_send![&*text_field, setBorderStyle: 3i64]; // UITextBorderStyleRoundedRect = 3
        let _: () = msg_send![&*text_field, setSecureTextEntry: true];
        let _: () = msg_send![&*text_field, setTranslatesAutoresizingMaskIntoConstraints: false];

        let target = PerrySecureFieldTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        SECUREFIELD_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, on_change);
        });

        let sel = Sel::register(c"textFieldChanged:");
        // UIControlEventEditingChanged = 1 << 17 = 131072
        let _: () = msg_send![&*text_field, addTarget: &*target, action: sel, forControlEvents: 131072u64];

        std::mem::forget(target);

        let view: Retained<UIView> = Retained::cast_unchecked(text_field);
        super::register_widget(view)
    }
}

/// Focus a secure text field (make it first responder).
pub fn focus(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, becomeFirstResponder];
        }
    }
}

/// Set the text of a secure text field from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns_string];
        }
    }
}
