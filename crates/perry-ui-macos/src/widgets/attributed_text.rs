//! Issue #710 — `AttributedText` for per-range styling inside a single
//! NSTextField. Distinct from `textSetDecoration` (whole-widget) and from
//! `rich_text` (#478 — full styled editor). The use case is inline
//! emphasis: bolding or coloring a word inside a paragraph that still
//! wraps as one block.
//!
//! API is a builder — `AttributedText()` returns an empty NSTextField,
//! `attributedTextAppend(widget, run...)` adds one styled run at a time,
//! `attributedTextClear(widget)` resets the buffer. We accumulate runs in
//! an NSMutableAttributedString keyed off the widget handle so the
//! lazy-bind doesn't lose state between calls.

use crate::string_header::StringHeader;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

use super::register_widget;

thread_local! {
    /// Mutable attributed-string buffer per AttributedText handle.
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

/// Create an empty AttributedText widget — a non-editable NSTextField
/// backed by an NSMutableAttributedString that is replaced on each
/// `append` / `clear`.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let empty = NSString::from_str("");
    let label = NSTextField::labelWithString(&empty, mtm);
    unsafe {
        let _: () = msg_send![&*label, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Enable wrapping by default — per-range styling is most useful
        // inside running prose that wraps.
        if let Some(cell) = label.cell() {
            let _: () = msg_send![&*cell, setWraps: true];
            // NSLineBreakByWordWrapping = 0
            let _: () = msg_send![&*cell, setLineBreakMode: 0u64];
        }
        let _: () = msg_send![&*label, setMaximumNumberOfLines: 0i64];
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(label) };
    let handle = register_widget(view);

    unsafe {
        let cls = AnyClass::get(c"NSMutableAttributedString").unwrap();
        let buf: Retained<AnyObject> = msg_send![cls, new];
        BUFFERS.with(|b| {
            b.borrow_mut().insert(handle, buf);
        });
    }
    handle
}

/// Append a styled run. `bold`/`italic`/`underline` are 0/1 booleans.
/// `font_size = 0` means inherit the widget's current font size.
/// If `a == 0.0`, the text color attribute is omitted (inherit).
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
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        // Build the attributes dictionary for this run.
        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let attrs: Retained<AnyObject> = msg_send![dict_cls, new];

        // Font: NSFont with optional bold/italic trait + optional size.
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let size = if font_size > 0.0 {
            font_size
        } else {
            // 0 means "use system default"; NSFont +systemFontSize gives it.
            let s: objc2_core_foundation::CGFloat = msg_send![font_cls, systemFontSize];
            s as f64
        };
        let base_font: Retained<AnyObject> = if bold != 0 {
            msg_send![font_cls, boldSystemFontOfSize: size as objc2_core_foundation::CGFloat]
        } else {
            msg_send![font_cls, systemFontOfSize: size as objc2_core_foundation::CGFloat]
        };
        let font: Retained<AnyObject> = if italic != 0 {
            // NSFontManager +sharedFontManager — convertFont:toHaveTrait: NSItalicFontMask = 1
            let mgr_cls = AnyClass::get(c"NSFontManager").unwrap();
            let mgr: *mut AnyObject = msg_send![mgr_cls, sharedFontManager];
            let italicized: *mut AnyObject =
                msg_send![mgr, convertFont: &*base_font, toHaveTrait: 1u64];
            if italicized.is_null() {
                base_font
            } else {
                // -convertFont:toHaveTrait: returns an autoreleased object;
                // retain it so the Retained drop matches.
                Retained::retain(italicized).unwrap_or(base_font)
            }
        } else {
            base_font
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
            let color_cls = AnyClass::get(c"NSColor").unwrap();
            let color: Retained<AnyObject> = msg_send![
                color_cls,
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

        // Apply to the widget.
        if let Some(view) = super::get_widget(handle) {
            BUFFERS.with(|b| {
                if let Some(buf) = b.borrow().get(&handle) {
                    let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
                    let _: () = msg_send![tf, setAttributedStringValue: &**buf];
                }
            });
        }
    }
}

/// Reset the buffer back to empty. Useful for re-rendering on state change.
pub fn clear(handle: i64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = AnyClass::get(c"NSMutableAttributedString").unwrap();
        let buf: Retained<AnyObject> = msg_send![cls, new];
        BUFFERS.with(|b| {
            b.borrow_mut().insert(handle, buf);
        });
        if let Some(view) = super::get_widget(handle) {
            BUFFERS.with(|b| {
                if let Some(buf) = b.borrow().get(&handle) {
                    let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
                    let _: () = msg_send![tf, setAttributedStringValue: &**buf];
                }
            });
        }
    }
}
