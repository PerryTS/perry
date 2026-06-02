//! Issue #710 — `AttributedText` for per-range styling inside a single
//! UILabel. Builder API (see `attributed_text.rs` in perry-ui-macos for
//! the same design rationale).

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use objc2_ui_kit::{UILabel, UIView};
use perry_runtime::string::StringHeader;
use std::cell::RefCell;
use std::collections::HashMap;

use super::register_widget;

thread_local! {
    static BUFFERS: RefCell<HashMap<i64, Retained<AnyObject>>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

pub fn create() -> i64 {
    unsafe {
        let label: Retained<UILabel> = msg_send![AnyClass::get(c"UILabel").unwrap(), new];
        let _: () = msg_send![&*label, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Default to multi-line wrap. Apps can override with `textSetNumberOfLines`.
        let _: () = msg_send![&*label, setNumberOfLines: 0i64];
        // NSLineBreakByWordWrapping = 0
        let _: () = msg_send![&*label, setLineBreakMode: 0u64];

        let view: Retained<UIView> = Retained::cast_unchecked(label);
        let handle = register_widget(view);

        let cls = AnyClass::get(c"NSMutableAttributedString").unwrap();
        let buf: Retained<AnyObject> = msg_send![cls, new];
        BUFFERS.with(|b| {
            b.borrow_mut().insert(handle, buf);
        });
        handle
    }
}

pub fn append(
    handle: i64,
    text_ptr: *const u8,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    let text = str_from_header(text_ptr);
    if text.is_empty() {
        return;
    }
    unsafe {
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let attrs: Retained<AnyObject> = msg_send![dict_cls, new];

        let font_cls = AnyClass::get(c"UIFont").unwrap();
        // UIFont.systemFontSize as default when font_size == 0.
        let size = if font_size > 0.0 {
            font_size
        } else {
            let s: objc2_core_foundation::CGFloat = msg_send![font_cls, systemFontSize];
            s as f64
        };

        // Pick the right factory based on bold/italic. UIFont exposes
        // distinct helpers for each combination, and -fontWithDescriptor:
        // is the union path.
        let font: Retained<AnyObject> = if bold != 0 && italic != 0 {
            // Bold+italic: build the descriptor manually.
            let base: *mut AnyObject = msg_send![
                font_cls,
                boldSystemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            let desc: *mut AnyObject = msg_send![base, fontDescriptor];
            // UIFontDescriptorTraitBold = 1<<1 = 2; UIFontDescriptorTraitItalic = 1<<0 = 1; both = 3.
            let with_traits: *mut AnyObject =
                msg_send![desc, fontDescriptorWithSymbolicTraits: 3u32];
            if with_traits.is_null() {
                Retained::retain(base).unwrap()
            } else {
                let f: *mut AnyObject = msg_send![
                    font_cls,
                    fontWithDescriptor: with_traits,
                    size: size as objc2_core_foundation::CGFloat
                ];
                if f.is_null() {
                    Retained::retain(base).unwrap()
                } else {
                    Retained::retain(f).unwrap()
                }
            }
        } else if bold != 0 {
            let f: *mut AnyObject = msg_send![
                font_cls,
                boldSystemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            Retained::retain(f).unwrap()
        } else if italic != 0 {
            let f: *mut AnyObject = msg_send![
                font_cls,
                italicSystemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            Retained::retain(f).unwrap()
        } else {
            let f: *mut AnyObject = msg_send![
                font_cls,
                systemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            Retained::retain(f).unwrap()
        };

        let font_key = NSString::from_str("NSFont");
        let _: () = msg_send![&*attrs, setObject: &*font, forKey: &*font_key];

        if underline != 0 {
            let underline_key = NSString::from_str("NSUnderline");
            let num_cls = AnyClass::get(c"NSNumber").unwrap();
            let one: Retained<AnyObject> = msg_send![num_cls, numberWithInt: 1i32];
            let _: () = msg_send![&*attrs, setObject: &*one, forKey: &*underline_key];
        }

        if a > 0.0 {
            let color: Retained<AnyObject> = msg_send![
                AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            let color_key = NSString::from_str("NSColor");
            let _: () = msg_send![&*attrs, setObject: &*color, forKey: &*color_key];
        }

        let ns_text = NSString::from_str(text);
        let attr_cls = AnyClass::get(c"NSAttributedString").unwrap();
        let alloc: *mut AnyObject = msg_send![attr_cls, alloc];
        let piece: Retained<AnyObject> = Retained::retain(msg_send![
            alloc,
            initWithString: &*ns_text,
            attributes: &*attrs
        ])
        .unwrap();

        BUFFERS.with(|b| {
            if let Some(buf) = b.borrow().get(&handle) {
                let _: () = msg_send![&**buf, appendAttributedString: &*piece];
            }
        });

        if let Some(view) = super::get_widget(handle) {
            BUFFERS.with(|b| {
                if let Some(buf) = b.borrow().get(&handle) {
                    let _: () = msg_send![&*view, setAttributedText: &**buf];
                }
            });
        }
    }
}

pub fn clear(handle: i64) {
    unsafe {
        let cls = AnyClass::get(c"NSMutableAttributedString").unwrap();
        let buf: Retained<AnyObject> = msg_send![cls, new];
        BUFFERS.with(|b| {
            b.borrow_mut().insert(handle, buf);
        });
        if let Some(view) = super::get_widget(handle) {
            BUFFERS.with(|b| {
                if let Some(buf) = b.borrow().get(&handle) {
                    let _: () = msg_send![&*view, setAttributedText: &**buf];
                }
            });
        }
    }
}
