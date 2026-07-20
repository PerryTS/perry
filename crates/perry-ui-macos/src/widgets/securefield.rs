use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSSecureTextField, NSTextField, NSView};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObject, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SECUREFIELD_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

pub struct PerrySecureFieldObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerrySecureFieldObserver"]
    #[ivars = PerrySecureFieldObserverIvars]
    pub struct PerrySecureFieldObserver;

    impl PerrySecureFieldObserver {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("securefield callback", std::panic::AssertUnwindSafe(|| {
                SECUREFIELD_CALLBACKS.with(|cbs| {
                    if let Some(&(closure_f64, tf_ptr)) = cbs.borrow().get(&key) {
                        if tf_ptr.is_null() {
                            return;
                        }

                        let notif_obj = notification.object();
                        if let Some(obj) = notif_obj {
                            let obj_ptr = &*obj as *const AnyObject;
                            if obj_ptr != tf_ptr {
                                return;
                            }
                        } else {
                            return;
                        }

                        let text_field = tf_ptr as *const NSTextField;
                        let text: Retained<NSString> = unsafe { (*text_field).stringValue() };
                        let rust_str = text.to_string();
                        let bytes = rust_str.as_bytes();

                        let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) };
                        let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };

                        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                        unsafe {
                            js_closure_call1(closure_ptr as *const u8, nanboxed);
                        }
                    }
                });
            }));
        }
    }
);

impl PerrySecureFieldObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerrySecureFieldObserverIvars {
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

/// Create a secure (password) NSSecureTextField with a placeholder string and onChange callback.
/// `placeholder_ptr` is a StringHeader pointer, `on_change` is a NaN-boxed closure.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_placeholder = NSString::from_str(placeholder);

    unsafe {
        let text_field: Retained<NSSecureTextField> = msg_send![
            NSSecureTextField::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(200.0, 22.0),
            )
        ];

        let tf_ref: &NSTextField = &Retained::cast_unchecked::<NSTextField>(text_field.clone());
        tf_ref.setPlaceholderString(Some(&ns_placeholder));
        tf_ref.setEditable(true);
        tf_ref.setBezeled(true);

        let view: Retained<NSView> = Retained::cast_unchecked(text_field);
        let handle = super::register_widget(view);

        // Get the raw pointer to the text field for notification matching
        let tf_view = super::get_widget(handle).unwrap();
        let tf_raw: *const AnyObject = Retained::as_ptr(&tf_view) as *const AnyObject;

        // Set up notification observer for text changes
        let observer = PerrySecureFieldObserver::new();
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().callback_key.set(observer_addr);

        SECUREFIELD_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(observer_addr, (on_change, tf_raw));
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            // Re-use WIDGET_TEXTFIELD (=1) + CB_ON_CHANGE (=1): NSSecureTextField
            // is-a NSTextField at the AppKit layer, so the runtime's SetText pump
            // (`perry_ui_textfield_set_string` + REGISTRY-driven onChange dispatch)
            // works against secure fields the same way it does against regular
            // text fields. Without this registration, `/type/<secure_handle>`
            // would set the string but never fire onChange — issue #640's "Add
            // Shop" form has at least one password input that suffered exactly
            // this when the geisterhand smoke harness drove it.
            unsafe {
                perry_geisterhand_register(handle, 1, 1, on_change, placeholder_ptr);
            }
        }

        // Register for NSControlTextDidChangeNotification
        let center = NSNotificationCenter::defaultCenter();
        let notif_name = NSString::from_str("NSControlTextDidChangeNotification");
        let sel = Sel::register(c"textDidChange:");
        let _: () = msg_send![&center, addObserver: &*observer, selector: sel, name: &*notif_name, object: tf_raw];

        // Prevent observer from being deallocated
        std::mem::forget(observer);

        handle
    }
}
