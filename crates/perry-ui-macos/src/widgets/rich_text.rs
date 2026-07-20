//! macOS Rich text editor widget (issue #478, v1).
//!
//! Wraps `NSTextView` with `setRichText:true` so the underlying
//! `NSAttributedString` storage is preserved across edits. Inline
//! bold / italic / underline are wired through `NSFontManager`
//! `addFontTrait:` / NSTextView's `underline:` responder action.
//! Get/set HTML uses NSAttributedString's `dataFromRange:documentAttributes:`
//! and `initWithData:options:documentAttributes:error:` for HTML
//! round-trip.
//!
//! Out of scope this iteration (per #478 scope): markdown round-trip,
//! block formatting (headings H1-H3, lists, blockquotes, code blocks),
//! configurable toolbar, paste handling for HTML / code blocks. Plain
//! and HTML round-trip cover the storage half; bold/italic/underline
//! cover the inline-formatting commands; toolbar + block commands +
//! markdown ship in a follow-up.

use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{NSObject, NSString};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    /// Map widget handle → text-view pointer (the NSTextView lives
    /// inside an NSScrollView, but the handle returned to the user is
    /// the scroll view; we cache the inner text view here so commands
    /// like `set_string` reach it directly).
    static TEXT_VIEWS: RefCell<HashMap<i64, Retained<AnyObject>>> = RefCell::new(HashMap::new());
    static CHANGE_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
    }
}

fn ns_string_to_rust(ns: *mut AnyObject) -> String {
    if ns.is_null() {
        return String::new();
    }
    unsafe {
        let ns_typed: &NSString = &*(ns as *const NSString);
        ns_typed.to_string()
    }
}

pub struct PerryRichTextDelegateIvars {
    pub handle: Cell<i64>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryRichTextDelegate"]
    #[ivars = PerryRichTextDelegateIvars]
    pub struct PerryRichTextDelegate;

    impl PerryRichTextDelegate {
        // NSTextDelegate — fires on every keystroke / programmatic edit.
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, _notification: &AnyObject) {
            let handle = self.ivars().handle.get();
            let addr = self as *const Self as usize;
            crate::catch_callback_panic("rich-text change", std::panic::AssertUnwindSafe(|| {
                let cb = CHANGE_CALLBACKS.with(|m| m.borrow().get(&addr).copied());
                let Some(callback) = cb else { return };
                let plain = get_string_inner(handle);
                unsafe {
                    let bytes = plain.as_bytes();
                    let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                    let arg = js_nanbox_string(header as i64);
                    let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                    js_closure_call1(closure_ptr, arg);
                }
            }));
        }
    }
);

impl PerryRichTextDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryRichTextDelegateIvars {
            handle: Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn get_string_inner(handle: i64) -> String {
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return String::new() };
    unsafe {
        let s: *mut AnyObject = msg_send![&*tv, string];
        ns_string_to_rust(s)
    }
}

pub fn create(width: f64, height: f64, on_change: f64) -> i64 {
    unsafe {
        let scroll_cls = AnyClass::get(c"NSScrollView").unwrap();
        let scroll_alloc: *mut AnyObject = msg_send![scroll_cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(width.max(40.0), height.max(40.0)),
        );
        let scroll: Retained<AnyObject> =
            Retained::from_raw(msg_send![scroll_alloc, initWithFrame: frame]).unwrap();
        let _: () = msg_send![&*scroll, setHasVerticalScroller: true];
        let _: () = msg_send![&*scroll, setBorderType: 1u64]; // line border

        let tv_cls = AnyClass::get(c"NSTextView").unwrap();
        let tv_alloc: *mut AnyObject = msg_send![tv_cls, alloc];
        let tv: Retained<AnyObject> =
            Retained::from_raw(msg_send![tv_alloc, initWithFrame: frame]).unwrap();
        let _: () = msg_send![&*tv, setRichText: true];
        let _: () = msg_send![&*tv, setAllowsUndo: true];
        let _: () = msg_send![&*tv, setEditable: true];
        let _: () = msg_send![&*tv, setSelectable: true];
        // Enable smart-link / smart-quote for paste handling — gives
        // basic paste-friendly behaviour without a dedicated paste hook.
        let _: () = msg_send![&*tv, setAutomaticLinkDetectionEnabled: true];
        let _: () = msg_send![&*tv, setAutomaticQuoteSubstitutionEnabled: true];

        let _: () = msg_send![&*scroll, setDocumentView: &*tv];

        let scroll_view: Retained<NSView> = Retained::cast_unchecked(scroll);
        let handle = super::register_widget(scroll_view);

        TEXT_VIEWS.with(|m| {
            m.borrow_mut().insert(handle, tv.clone());
        });

        if on_change != 0.0 {
            let delegate = PerryRichTextDelegate::new();
            delegate.ivars().handle.set(handle);
            let target_addr = Retained::as_ptr(&delegate) as usize;
            CHANGE_CALLBACKS.with(|m| {
                m.borrow_mut().insert(target_addr, on_change);
            });
            let _: () = msg_send![&*tv, setDelegate: &*delegate];
            std::mem::forget(delegate);
        }

        handle
    }
}

pub fn set_string(handle: i64, text_ptr: *const u8) {
    let s = str_from_header(text_ptr);
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return };
    unsafe {
        let ns = NSString::from_str(&s);
        let _: () = msg_send![&*tv, setString: &*ns];
    }
}

pub fn get_string(handle: i64) -> f64 {
    let s = get_string_inner(handle);
    unsafe {
        let bytes = s.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(header as i64)
    }
}

/// Replace contents with rendered HTML (NSAttributedString HTML
/// importer). Returns 1 on success, 0 on failure.
pub fn set_html(handle: i64, html_ptr: *const u8) -> i64 {
    let html = str_from_header(html_ptr);
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return 0 };
    unsafe {
        let ns_html = NSString::from_str(&html);
        let data: *mut AnyObject = msg_send![
            &*ns_html, dataUsingEncoding: 4u64 // NSUTF8StringEncoding
        ];
        if data.is_null() {
            return 0;
        }
        let Some(astr_cls) = AnyClass::get(c"NSAttributedString") else {
            return 0;
        };
        let alloc: *mut AnyObject = msg_send![astr_cls, alloc];
        let key = NSString::from_str("DocumentType");
        let value = NSString::from_str("NSHTMLTextDocumentType");
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let opts: *mut AnyObject = msg_send![dict_cls, new];
        let _: () = msg_send![opts, setObject: &*value, forKey: &*key];
        let mut err: *mut AnyObject = std::ptr::null_mut();
        let astr: *mut AnyObject = msg_send![
            alloc, initWithData: data, options: opts, documentAttributes: std::ptr::null_mut::<*mut AnyObject>(), error: &mut err
        ];
        if astr.is_null() {
            return 0;
        }
        let storage: *mut AnyObject = msg_send![&*tv, textStorage];
        if storage.is_null() {
            return 0;
        }
        let _: () = msg_send![storage, setAttributedString: astr];
        1
    }
}

/// Serialize current contents as HTML.
pub fn get_html(handle: i64) -> f64 {
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    unsafe {
        let storage: *mut AnyObject = msg_send![&*tv, textStorage];
        if storage.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let length: usize = msg_send![storage, length];
        let range = objc2_foundation::NSRange::from(0..length);
        let key = NSString::from_str("DocumentType");
        let value = NSString::from_str("NSHTMLTextDocumentType");
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let opts: *mut AnyObject = msg_send![dict_cls, new];
        let _: () = msg_send![opts, setObject: &*value, forKey: &*key];
        let data: *mut AnyObject = msg_send![
            storage, dataFromRange: range, documentAttributes: opts, error: std::ptr::null_mut::<*mut AnyObject>()
        ];
        if data.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let str_alloc: *mut AnyObject = msg_send![AnyClass::get(c"NSString").unwrap(), alloc];
        let str_obj: Retained<NSString> = match Retained::from_raw(msg_send![
            str_alloc, initWithData: data, encoding: 4u64
        ]) {
            Some(s) => s,
            None => return f64::from_bits(0x7FFC_0000_0000_0001),
        };
        let html = str_obj.to_string();
        let bytes = html.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(header as i64)
    }
}

unsafe fn invoke_responder(handle: i64, sel: Sel) {
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return };
    let _: () =
        msg_send![&*tv, performSelector: sel, withObject: std::ptr::null_mut::<AnyObject>()];
}

pub fn toggle_bold(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"toggleBold:"));
    }
}

pub fn toggle_italic(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"toggleItalic:"));
    }
}

pub fn toggle_underline(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"underline:"));
    }
}
