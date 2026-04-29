use crate::string_header::StringHeader;
use objc2::rc::Retained;
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

use super::register_widget;

/// Extract a &str from a *const StringHeader pointer.
/// StringHeader is { length: u32, capacity: u32 } followed by UTF-8 data.
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

/// Create an NSTextField configured as a non-editable label.
pub fn create(text_ptr: *const u8) -> i64 {
    let text = str_from_header(text_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_string = NSString::from_str(text);

    let label = NSTextField::labelWithString(&ns_string, mtm);
    unsafe {
        let _: () = objc2::msg_send![&*label, setAccessibilityLabel: &*ns_string];
        // Disable autoresizing mask so Auto Layout can size this view in NSStackView.
        // Without this, NSTextField collapses to zero size when added via addArrangedSubview.
        let _: () = objc2::msg_send![&*label, setTranslatesAutoresizingMaskIntoConstraints: false];
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(label) };
    register_widget(view)
}

/// Update the text of an existing Text widget (NSTextField).
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setStringValue(&ns_string);
        }
    }
}

/// Update the text of an existing Text widget from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Set the text color of a Text widget.
///
/// Phase C step 6/7's inline-style codegen dispatches every `color: ...`
/// prop through this entry point, regardless of the actual widget class
/// (`crates/perry-codegen/src/lower_call.rs` ~3231). The codegen comment
/// states "no-op on widgets that ignore it" — so this function probes
/// the widget's runtime class and routes appropriately:
///
/// - NSTextField → setTextColor: (the original path)
/// - NSButton    → forward to button::set_text_color (NSButton has no
///                 `setTextColor:` selector; calling it raises an
///                 unrecognized-selector ObjC exception, which crosses
///                 the FFI boundary as a non-unwinding panic and aborts
///                 the process — exactly the regression seen on
///                 `docs/examples/ui/styling/{hex_gradient,dynamic_color}.ts`
///                 before this fix)
/// - other       → silent no-op (matches the codegen's documented intent)
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        if let Some(btn_cls) = objc2::runtime::AnyClass::get(c"NSButton") {
            let is_btn: bool = objc2::msg_send![&*view, isKindOfClass: btn_cls];
            if is_btn {
                drop(view);
                super::button::set_text_color(handle, r, g, b, a);
                return;
            }
        }
        if let Some(tf_cls) = objc2::runtime::AnyClass::get(c"NSTextField") {
            let is_tf: bool = objc2::msg_send![&*view, isKindOfClass: tf_cls];
            if !is_tf {
                return;
            }
        }
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

/// Set the font size of a Text widget.
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

/// Set the font weight of a Text widget (0.0 = regular, 1.0 = bold).
pub fn set_font_weight(handle: i64, size: f64, weight: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let font: Retained<objc2_app_kit::NSFont> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat,
                weight: weight as objc2_core_foundation::CGFloat
            ];
            tf.setFont(Some(&font));
        }
    }
}

/// Enable word wrapping on a Text widget.
/// max_width sets the preferred wrapping width (0 = use intrinsic width).
pub fn set_wraps(handle: i64, max_width: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            // Enable wrapping on the cell
            let cell = tf.cell().unwrap();
            let _: () = objc2::msg_send![&*cell, setWraps: true];
            let _: () = objc2::msg_send![&*cell, setLineBreakMode: 0u64]; // NSLineBreakByWordWrapping = 0
                                                                          // Unlimited lines
            let _: () = objc2::msg_send![tf, setMaximumNumberOfLines: 0i64];
            // Set preferred max layout width for Auto Layout wrapping
            if max_width > 0.0 {
                let _: () = objc2::msg_send![tf, setPreferredMaxLayoutWidth: max_width as objc2_core_foundation::CGFloat];
            }
        }
    }
}

/// Set whether a Text widget is selectable.
pub fn set_selectable(handle: i64, selectable: bool) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setSelectable(selectable);
        }
    }
}

/// Set text decoration on a Text widget via `NSAttributedString` (issue
/// #185 Phase B). `decoration`: 0=none, 1=underline, 2=strikethrough.
/// Reads the current `stringValue`, wraps it with the requested
/// underline / strikethrough attribute (NSUnderlineStyleSingle = 1),
/// and calls `setAttributedStringValue:`. Calling this with `decoration =
/// 0` resets to the plain string. Pattern mirrors `button::set_text_color`.
pub fn set_decoration(handle: i64, decoration: i64) {
    use objc2::runtime::{AnyClass, AnyObject};
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let current: Retained<NSString> = objc2::msg_send![tf, stringValue];
            if decoration == 0 {
                tf.setStringValue(&current);
                return;
            }
            let key = if decoration == 1 {
                NSString::from_str("NSUnderline")
            } else {
                NSString::from_str("NSStrikethrough")
            };
            let num_cls = AnyClass::get(c"NSNumber").unwrap();
            let one: Retained<AnyObject> = objc2::msg_send![num_cls, numberWithInt: 1i32];
            let attrs: Retained<AnyObject> = objc2::msg_send![
                AnyClass::get(c"NSDictionary").unwrap(),
                dictionaryWithObject: &*one,
                forKey: &*key
            ];
            let ns_str: *const AnyObject = Retained::as_ptr(&current) as *const AnyObject;
            let cls = AnyClass::get(c"NSAttributedString").unwrap();
            let alloc: *mut AnyObject = objc2::msg_send![cls, alloc];
            let attr_str: *mut AnyObject = objc2::msg_send![
                alloc,
                initWithString: ns_str,
                attributes: &*attrs
            ];
            let _: () = objc2::msg_send![tf, setAttributedStringValue: attr_str];
        }
    }
}
