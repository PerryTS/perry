use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_ui_kit::UIView;
use objc2_foundation::{NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TEXTAREA_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

pub struct PerryTextAreaDelegateIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextAreaDelegate"]
    #[ivars = PerryTextAreaDelegateIvars]
    pub struct PerryTextAreaDelegate;

    impl PerryTextAreaDelegate {
        /// UITextViewDelegate method — called when the text view's text changes.
        #[unsafe(method(textViewDidChange:))]
        fn text_view_did_change(&self, text_view: &AnyObject) {
            let key = self.ivars().callback_key.get();
            TEXTAREA_CALLBACKS.with(|cbs| {
                if let Some(&(closure_f64, _tv_ptr)) = cbs.borrow().get(&key) {
                    let text: Retained<NSString> = unsafe { msg_send![text_view, text] };
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

impl PerryTextAreaDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextAreaDelegateIvars {
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

/// Create a UITextView with placeholder text and onChange callback.
/// Returns a widget handle.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let _placeholder = str_from_header(placeholder_ptr);

    unsafe {
        let text_view: Retained<AnyObject> = msg_send![
            AnyClass::get(c"UITextView").unwrap(),
            new
        ];
        let _: () = msg_send![&*text_view, setEditable: true];
        let _: () = msg_send![&*text_view, setSelectable: true];
        let _: () = msg_send![&*text_view, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Set default font (system 14pt)
        let font_cls = AnyClass::get(c"UIFont").unwrap();
        let font: Retained<AnyObject> = msg_send![font_cls, systemFontOfSize: 14.0f64];
        let _: () = msg_send![&*text_view, setFont: &*font];

        // Create delegate for text change notifications
        let delegate = PerryTextAreaDelegate::new();
        let delegate_addr = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().callback_key.set(delegate_addr);

        let tv_ptr = Retained::as_ptr(&text_view) as *const AnyObject;
        TEXTAREA_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(delegate_addr, (on_change, tv_ptr));
        });

        // Set the delegate on the UITextView
        let _: () = msg_send![&*text_view, setDelegate: &*delegate];

        std::mem::forget(delegate);

        let view: Retained<UIView> = Retained::cast_unchecked(text_view);
        let handle = super::register_widget(view);
        handle
    }
}

/// Set the text content of a UITextView from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns_string];
        }
    }
}

/// Get the current text content of a UITextView, returns a StringHeader pointer.
pub fn get_string(handle: i64) -> *const u8 {
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
