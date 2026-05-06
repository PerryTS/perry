//! iOS Rich text editor — `UITextView` with `NSAttributedString`
//! storage (issue #478 / iOS parity work).
//!
//! Same surface as the macOS impl: plain-text + HTML round-trip via
//! NSAttributedString's `dataFromRange:documentAttributes:` /
//! `initWithData:options:documentAttributes:error:`. Bold / italic /
//! underline use UITextView's `toggleBoldface:` / `toggleItalics:` /
//! `toggleUnderline:` responder actions — UITextView inherits them
//! from UIResponderStandardEditActions, same as NSTextView.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    static TEXT_VIEWS: RefCell<HashMap<i64, Retained<AnyObject>>> = RefCell::new(HashMap::new());
    static CHANGE_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
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
    #[name = "PerryRichTextDelegateVisionOS"]
    #[ivars = PerryRichTextDelegateIvars]
    pub struct PerryRichTextDelegate;

    impl PerryRichTextDelegate {
        // UITextViewDelegate protocol: textViewDidChange:
        #[unsafe(method(textViewDidChange:))]
        fn text_did_change(&self, _text_view: &AnyObject) {
            let handle = self.ivars().handle.get();
            let addr = self as *const Self as usize;
            let cb = CHANGE_CALLBACKS.with(|m| m.borrow().get(&addr).copied());
            let Some(callback) = cb else { return };
            let plain = get_string_inner(handle);
            unsafe {
                let bytes = plain.as_bytes();
                let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let arg = js_nanbox_string(header as i64);
                let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
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
        let s: *mut AnyObject = msg_send![&*tv, text];
        ns_string_to_rust(s)
    }
}

pub fn create(width: f64, height: f64, on_change: f64) -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = AnyClass::get(c"UITextView").unwrap();
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(width.max(40.0), height.max(40.0)),
        );
        let raw: *mut AnyObject = msg_send![alloc, initWithFrame: frame];
        let tv: Retained<AnyObject> = Retained::from_raw(raw).unwrap();
        let _: () = msg_send![&*tv, setEditable: true];
        let _: () = msg_send![&*tv, setSelectable: true];
        // Allow attributed-string editing — needed for bold / italic /
        // underline tags to survive across text-replacement events.
        let _: () = msg_send![&*tv, setAllowsEditingTextAttributes: true];

        let nsview: Retained<UIView> = Retained::cast_unchecked(tv.clone());
        let handle = super::register_widget(nsview);
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
        let ns = NSString::from_str(s);
        let _: () = msg_send![&*tv, setText: &*ns];
    }
}

pub fn get_string(handle: i64) -> f64 {
    let s = get_string_inner(handle);
    unsafe {
        let bytes = s.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}

pub fn set_html(handle: i64, html_ptr: *const u8) -> i64 {
    let html = str_from_header(html_ptr);
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return 0 };
    unsafe {
        let ns_html = NSString::from_str(html);
        let data: *mut AnyObject = msg_send![&*ns_html, dataUsingEncoding: 4u64];
        if data.is_null() {
            return 0;
        }
        let Some(astr_cls) = AnyClass::get(c"NSAttributedString") else { return 0 };
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
        // UITextView uses `attributedText` rather than NSTextView's
        // `textStorage` — but assigning attributedText snapshots the
        // attributed string (no live mutation tracking, but matches
        // user expectation for "load this HTML").
        let _: () = msg_send![&*tv, setAttributedText: astr];
        1
    }
}

pub fn get_html(handle: i64) -> f64 {
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    unsafe {
        let astr: *mut AnyObject = msg_send![&*tv, attributedText];
        if astr.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        let length: usize = msg_send![astr, length];
        let range = objc2_foundation::NSRange::from(0..length);
        let key = NSString::from_str("DocumentType");
        let value = NSString::from_str("NSHTMLTextDocumentType");
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let opts: *mut AnyObject = msg_send![dict_cls, new];
        let _: () = msg_send![opts, setObject: &*value, forKey: &*key];
        let data: *mut AnyObject = msg_send![
            astr, dataFromRange: range, documentAttributes: opts, error: std::ptr::null_mut::<*mut AnyObject>()
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
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}

unsafe fn invoke_responder(handle: i64, sel: Sel) {
    let tv = TEXT_VIEWS.with(|m| m.borrow().get(&handle).cloned());
    let Some(tv) = tv else { return };
    let _: () = msg_send![&*tv, performSelector: sel, withObject: std::ptr::null_mut::<AnyObject>()];
}

pub fn toggle_bold(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"toggleBoldface:"));
    }
}

pub fn toggle_italic(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"toggleItalics:"));
    }
}

pub fn toggle_underline(handle: i64) {
    unsafe {
        invoke_responder(handle, Sel::register(c"toggleUnderline:"));
    }
}
