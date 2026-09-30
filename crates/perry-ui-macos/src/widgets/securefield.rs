use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, ClassType, DefinedClass};
use objc2_app_kit::{NSSecureTextField, NSTextField, NSTextView, NSView};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObject, NSRange, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SECUREFIELD_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
}

pub(crate) fn scan_macos_securefield_gc_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    SECUREFIELD_CALLBACKS.with(|callbacks| {
        for (callback, _) in callbacks.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(callback);
        }
    });
}

extern "C" {
    fn js_closure_call1(closure: *const u8, this: perry_ffi::JsThis, arg: f64) -> f64;
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
                            js_closure_call1(closure_ptr as *const u8, perry_ffi::JsThis::UNDEFINED, nanboxed);
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

define_class!(
    #[unsafe(super(NSSecureTextField))]
    #[name = "PerrySecureTextField"]
    pub struct PerrySecureTextField;

    impl PerrySecureTextField {
        #[unsafe(method(cellClass))]
        fn cell_class() -> &'static AnyClass {
            super::padding::PerryInsetSecureTextFieldCell::class()
        }

        #[unsafe(method(setStringValue:))]
        fn set_string_value(&self, value: &NSString) {
            let value = super::textfield::one_line_value(value);
            unsafe { msg_send![super(self), setStringValue: &*value] }
        }

        #[unsafe(method(textView:shouldChangeTextInRange:replacementString:))]
        fn should_change_text(
            &self,
            editor: &NSTextView,
            range: NSRange,
            replacement: Option<&NSString>,
        ) -> bool {
            match super::textfield::entered_text_with_spaces(replacement) {
                // Inserting the spaced text asks this method again, now with no
                // line break, so the edit still passes through super.
                Some(spaced) => {
                    let _: () = unsafe { msg_send![editor, insertText: &*spaced, replacementRange: range] };
                    false
                }
                None => unsafe {
                    msg_send![super(self), textView: editor, shouldChangeTextInRange: range, replacementString: replacement]
                },
            }
        }

        #[unsafe(method(textView:doCommandBySelector:))]
        fn do_command(&self, editor: &NSTextView, command: Sel) -> bool {
            super::textfield::is_line_break_command(command)
                || unsafe { msg_send![super(self), textView: editor, doCommandBySelector: command] }
        }
    }
);

/// A one-line secure text field, as `textFieldWithString:` builds it, with an
/// inset cell so `set_edge_insets` can pad it.
pub(crate) fn secure_text_field(
    string: &NSString,
    _mtm: MainThreadMarker,
) -> Retained<NSSecureTextField> {
    let field: Retained<PerrySecureTextField> =
        unsafe { msg_send![PerrySecureTextField::class(), textFieldWithString: string] };
    field.into_super()
}

/// Extract a &str from a *const StringHeader pointer.
use perry_ffi::copy_string_from_raw as str_from_header;

/// Create a secure (password) NSSecureTextField with a placeholder string and onChange callback.
/// `placeholder_ptr` is a StringHeader pointer, `on_change` is a NaN-boxed closure.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = unsafe { str_from_header(placeholder_ptr) };

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_placeholder = NSString::from_str(&placeholder);

    unsafe {
        let text_field = secure_text_field(&NSString::from_str(""), mtm);
        text_field.setPlaceholderString(Some(&ns_placeholder));

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
