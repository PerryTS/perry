use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSNotificationCenter, NSObject, NSString,
};
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

pub struct PerryTextAreaObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextAreaObserver"]
    #[ivars = PerryTextAreaObserverIvars]
    pub struct PerryTextAreaObserver;

    impl PerryTextAreaObserver {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            crate::catch_callback_panic("textarea callback", std::panic::AssertUnwindSafe(|| {
                TEXTAREA_CALLBACKS.with(|cbs| {
                    if let Some(&(closure_f64, tv_ptr)) = cbs.borrow().get(&key) {
                        if tv_ptr.is_null() { return; }

                        let notif_obj = notification.object();
                        if let Some(obj) = notif_obj {
                            let obj_ptr = &*obj as *const AnyObject;
                            if obj_ptr != tv_ptr { return; }
                        } else {
                            return;
                        }

                        // Get the text from NSTextView via its NSTextStorage
                        let text: Retained<AnyObject> = unsafe { msg_send![tv_ptr, string] };
                        let rust_str: Retained<NSString> = unsafe { Retained::cast_unchecked(text) };
                        let s = rust_str.to_string();
                        let bytes = s.as_bytes();

                        let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
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

impl PerryTextAreaObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextAreaObserverIvars {
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
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a multi-line text area (NSScrollView + NSTextView) with onChange callback.
/// Returns a widget handle for the outer NSScrollView.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let _placeholder = str_from_header(placeholder_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Create NSTextView
        let tv_cls = AnyClass::get(c"NSTextView").unwrap();
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(400.0, 200.0),
        );
        let text_view: *mut AnyObject = msg_send![tv_cls, alloc];
        let text_view: *mut AnyObject = msg_send![text_view, initWithFrame: frame];

        // Configure text view
        let _: () = msg_send![text_view, setEditable: true];
        let _: () = msg_send![text_view, setSelectable: true];
        let _: () = msg_send![text_view, setRichText: false];
        let _: () = msg_send![text_view, setAllowsUndo: true];

        // Auto-resize width with scroll view
        let _: () = msg_send![text_view, setHorizontallyResizable: false];
        let _: () = msg_send![text_view, setVerticallyResizable: true];
        let max_size = objc2_core_foundation::CGSize::new(f64::MAX, f64::MAX);
        let _: () = msg_send![text_view, setMaxSize: max_size];
        let autoresizing: u64 = 1 << 1; // NSViewWidthSizable
        let _: () = msg_send![text_view, setAutoresizingMask: autoresizing];

        // Set monospace font
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let font_name = NSString::from_str("Menlo");
        let font: *mut AnyObject = msg_send![font_cls, fontWithName: &*font_name, size: 13.0f64];
        if !font.is_null() {
            let _: () = msg_send![text_view, setFont: font];
        }

        // Text container should track width
        let container: *mut AnyObject = msg_send![text_view, textContainer];
        if !container.is_null() {
            let container_size = objc2_core_foundation::CGSize::new(f64::MAX, f64::MAX);
            let _: () = msg_send![container, setContainerSize: container_size];
            let _: () = msg_send![container, setWidthTracksTextView: true];
        }

        // Create NSScrollView wrapper
        let sv_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll: *mut AnyObject = msg_send![sv_cls, alloc];
        let scroll: *mut AnyObject = msg_send![scroll, initWithFrame: frame];
        let _: () = msg_send![scroll, setHasVerticalScroller: true];
        let _: () = msg_send![scroll, setHasHorizontalScroller: false];
        let _: () = msg_send![scroll, setBorderType: 1i64]; // NSBezelBorder
        let _: () = msg_send![scroll, setDocumentView: text_view];

        // Disable autoresizing mask so Auto Layout works
        let _: () = msg_send![scroll, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Register the NSScrollView as the widget
        let scroll_retained: Retained<NSView> = Retained::retain(scroll as *mut NSView).unwrap();
        let handle = super::register_widget(scroll_retained);

        // Set up notification observer for text changes
        let observer = PerryTextAreaObserver::new();
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().callback_key.set(observer_addr);

        TEXTAREA_CALLBACKS.with(|cbs| {
            cbs.borrow_mut()
                .insert(observer_addr, (on_change, text_view as *const AnyObject));
        });

        let center = NSNotificationCenter::defaultCenter();
        let notif_name = NSString::from_str("NSTextDidChangeNotification");
        let sel = Sel::register(c"textDidChange:");
        let _: () = msg_send![&center, addObserver: &*observer, selector: sel, name: &*notif_name, object: text_view];

        std::mem::forget(observer);

        handle
    }
}

/// Set the text of a TextArea.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            // view is the NSScrollView, get the documentView (NSTextView)
            let tv: *mut AnyObject = msg_send![&*view, documentView];
            if !tv.is_null() {
                let ns_string = NSString::from_str(text);
                let _: () = msg_send![tv, setString: &*ns_string];
            }
        }
    }
}

/// Get the text of a TextArea.
pub fn get_string(handle: i64) -> *const u8 {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tv: *mut AnyObject = msg_send![&*view, documentView];
            if !tv.is_null() {
                let text: Retained<AnyObject> = msg_send![tv, string];
                let ns_str: &NSString = &*(Retained::as_ptr(&text) as *const NSString);
                let s = ns_str.to_string();
                return js_string_from_bytes(s.as_ptr(), s.len() as i64);
            }
        }
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
