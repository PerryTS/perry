use crate::ffi::js_string_from_bytes;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObject, NSString,
};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from observer object address to (closure_f64, textfield_view_ptr, textfield_handle)
    static TEXTFIELD_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject, i64)>> = RefCell::new(HashMap::new());
    /// Map from observer address to (submit_closure_f64, textfield_view_ptr)
    static TEXTFIELD_SUBMIT_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
    /// Map from observer address to (focus_closure_f64, textfield_view_ptr)
    static TEXTFIELD_FOCUS_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Internal state for the notification observer
pub struct PerryTextFieldObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldObserver"]
    #[ivars = PerryTextFieldObserverIvars]
    pub struct PerryTextFieldObserver;

    impl PerryTextFieldObserver {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("textfield callback", std::panic::AssertUnwindSafe(|| {
                TEXTFIELD_CALLBACKS.with(|cbs| {
                    if let Some(&(closure_f64, tf_ptr, tf_handle)) = cbs.borrow().get(&key) {
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

                        // If this textfield is bound to a state cell via
                        // `stateBindTextfield`, set the binding's suppress flag
                        // around the user closure so any `state.set` inside it
                        // doesn't echo back to setStringValue and reset the
                        // user's cursor / selection. See #1198.
                        let binding = crate::state::TEXTFIELD_TO_BINDING
                            .with(|m| m.borrow().get(&tf_handle).cloned());
                        if let Some(ref b) = binding {
                            b.suppress.set(true);
                        }
                        unsafe {
                            js_closure_call1(closure_ptr as *const u8, nanboxed);
                        }
                        if let Some(b) = binding {
                            b.suppress.set(false);
                        }
                    }
                });
            }));
        }
    }
);

impl PerryTextFieldObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldObserverIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Observer for NSControlTextDidEndEditingNotification (Enter/Return key).
pub struct PerryTextFieldSubmitObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldSubmitObserver"]
    #[ivars = PerryTextFieldSubmitObserverIvars]
    pub struct PerryTextFieldSubmitObserver;

    impl PerryTextFieldSubmitObserver {
        #[unsafe(method(textDidEndEditing:))]
        fn text_did_end_editing(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("textfield submit callback", std::panic::AssertUnwindSafe(|| {
                TEXTFIELD_SUBMIT_CALLBACKS.with(|cbs| {
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

                        // Read current text value
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

impl PerryTextFieldSubmitObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldSubmitObserverIvars {
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

/// Create an editable NSTextField with a placeholder string and onChange callback.
/// `placeholder_ptr` is a StringHeader pointer, `on_change` is a NaN-boxed closure.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_placeholder = NSString::from_str(placeholder);

    unsafe {
        let text_field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        text_field.setPlaceholderString(Some(&ns_placeholder));

        // Make it editable
        text_field.setEditable(true);
        text_field.setBezeled(true);

        let view: Retained<NSView> = Retained::cast_unchecked(text_field);
        let handle = super::register_widget(view);

        // Get the raw pointer to the text field for notification matching
        let tf_view = super::get_widget(handle).unwrap();
        let tf_raw: *const AnyObject = Retained::as_ptr(&tf_view) as *const AnyObject;

        // Set up notification observer for text changes
        let observer = PerryTextFieldObserver::new();
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().callback_key.set(observer_addr);

        TEXTFIELD_CALLBACKS.with(|cbs| {
            cbs.borrow_mut()
                .insert(observer_addr, (on_change, tf_raw, handle));
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
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

/// Focus an editable text field (make it first responder).
pub fn focus(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            if let Some(window) = tf.window() {
                window.makeFirstResponder(Some(tf));
            }
        }
    }
}

/// Set the text of an editable text field from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setStringValue(&ns_string);
        }
    }
}

/// Get the current string value from a textfield, returns a NaN-boxed string pointer.
/// The returned string is pinned (GC_FLAG_PINNED) so it survives garbage collection
/// until the caller has consumed it. Without pinning, the GC can collect the string
/// between the return and the caller's use, causing undefined values.
pub fn get_string_value(handle: i64) -> *const u8 {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let value = tf.stringValue();
            let bytes = value.to_string();
            let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            // Pin the GC allocation so it won't be collected before the caller uses it.
            // GcHeader layout: obj_type(u8) + gc_flags(u8) + reserved(u16) + size(u32) = 8 bytes
            // GcHeader sits BEFORE the user pointer (ptr - 8). gc_flags is at offset 1.
            // GC_FLAG_PINNED = 0x04
            // Pin the GC allocation so it survives until the caller consumes it
            let gc_flags_ptr = (ptr as *mut u8).sub(8).add(1);
            *gc_flags_ptr |= 0x04; // GC_FLAG_PINNED
            return ptr as *const u8;
        }
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) as *const u8 }
}

/// Set an onSubmit callback (fires when user presses Enter/Return).
/// `on_submit` is a NaN-boxed closure that receives the text as a NaN-boxed string.
pub fn set_on_submit(handle: i64, on_submit: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf_raw: *const AnyObject = Retained::as_ptr(&view) as *const AnyObject;

            let observer = PerryTextFieldSubmitObserver::new();
            let observer_addr = Retained::as_ptr(&observer) as usize;
            observer.ivars().callback_key.set(observer_addr);

            TEXTFIELD_SUBMIT_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(observer_addr, (on_submit, tf_raw));
            });

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                }
                perry_geisterhand_register(handle, 1, 2, on_submit, std::ptr::null());
            }

            let center = NSNotificationCenter::defaultCenter();
            let notif_name = NSString::from_str("NSControlTextDidEndEditingNotification");
            let sel = Sel::register(c"textDidEndEditing:");
            let _: () = msg_send![&center, addObserver: &*observer, selector: sel, name: &*notif_name, object: tf_raw];

            std::mem::forget(observer);
        }
    }
}

/// Observer for NSControlTextDidBeginEditingNotification (text field gained focus/started editing).
pub struct PerryTextFieldFocusObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldFocusObserver"]
    #[ivars = PerryTextFieldFocusObserverIvars]
    pub struct PerryTextFieldFocusObserver;

    impl PerryTextFieldFocusObserver {
        #[unsafe(method(textDidBeginEditing:))]
        fn text_did_begin_editing(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("textfield focus callback", std::panic::AssertUnwindSafe(|| {
                TEXTFIELD_FOCUS_CALLBACKS.with(|cbs| {
                    if let Some(&(closure_f64, tf_ptr)) = cbs.borrow().get(&key) {
                        if tf_ptr.is_null() { return; }
                        let notif_obj = notification.object();
                        if let Some(obj) = notif_obj {
                            let obj_ptr = &*obj as *const AnyObject;
                            if obj_ptr != tf_ptr { return; }
                        } else { return; }
                        let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                        unsafe {
                            js_closure_call1(closure_ptr as *const u8, 0.0);
                        }
                    }
                });
            }));
        }
    }
);

impl PerryTextFieldFocusObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldFocusObserverIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set an onFocus callback (fires when text field begins editing).
pub fn set_on_focus(handle: i64, on_focus: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf_raw: *const AnyObject = Retained::as_ptr(&view) as *const AnyObject;

            let observer = PerryTextFieldFocusObserver::new();
            let observer_addr = Retained::as_ptr(&observer) as usize;
            observer.ivars().callback_key.set(observer_addr);

            TEXTFIELD_FOCUS_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(observer_addr, (on_focus, tf_raw));
            });

            let center = NSNotificationCenter::defaultCenter();
            let notif_name = NSString::from_str("NSControlTextDidBeginEditingNotification");
            let sel = Sel::register(c"textDidBeginEditing:");
            let _: () = msg_send![&center, addObserver: &*observer, selector: sel, name: &*notif_name, object: tf_raw];

            std::mem::forget(observer);
        }
    }
}

/// Resign first responder from the key window (blur all text fields).
pub fn blur_all() {
    unsafe {
        let app_cls = objc2::runtime::AnyClass::get(c"NSApplication").unwrap();
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        let window: *mut AnyObject = msg_send![app, keyWindow];
        if !window.is_null() {
            let _: () = msg_send![window, makeFirstResponder: std::ptr::null::<AnyObject>()];
        }
    }
}

/// Set the text of a textfield from a Rust &str (used by state binding).
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setStringValue(&ns_string);
        }
    }
}

/// Set whether the text field is borderless (0 = bordered, 1 = borderless).
/// When borderless, also disables the macOS blue focus ring.
pub fn set_borderless(handle: i64, borderless: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            if borderless > 0.5 {
                tf.setBezeled(false);
                tf.setBordered(false);
                // NSFocusRingTypeNone = 1
                let _: () = objc2::msg_send![tf, setFocusRingType: 1u64];
            } else {
                tf.setBezeled(true);
                tf.setBordered(true);
                // NSFocusRingTypeDefault = 0
                let _: () = objc2::msg_send![tf, setFocusRingType: 0u64];
            }
        }
    }
}

/// Set the background color of the text field.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setDrawsBackground(true);
            let color: Retained<objc2_app_kit::NSColor> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            tf.setBackgroundColor(Some(&color));
        }
    }
}

/// Set the font size of the text field.
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let font: Retained<objc2_app_kit::NSFont> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            tf.setFont(Some(&font));
        }
    }
}

/// Set the next key view for tab navigation.
/// When the user presses Tab in `handle`, focus moves to `next_handle`.
pub fn set_next_key_view(handle: i64, next_handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        if let Some(next_view) = super::get_widget(next_handle) {
            unsafe {
                let _: () = msg_send![&*view, setNextKeyView: &*next_view];
            }
        }
    }
}

/// Set the text color of the text field.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let color: Retained<objc2_app_kit::NSColor> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            tf.setTextColor(Some(&color));
        }
    }
}
