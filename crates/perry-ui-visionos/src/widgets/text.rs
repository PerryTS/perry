use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyClass;
use objc2_ui_kit::{UILabel, UIView};
use objc2_foundation::NSString;
use perry_runtime::string::StringHeader;

use super::register_widget;

/// Extract a &str from a *const StringHeader pointer.
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

/// Create a UILabel.
pub fn create(text_ptr: *const u8) -> i64 {
    let text = str_from_header(text_ptr);

    unsafe {
        let label: Retained<UILabel> = msg_send![objc2::runtime::AnyClass::get(c"UILabel").unwrap(), new];
        let ns_string = NSString::from_str(text);
        let _: () = msg_send![&*label, setText: &*ns_string];
        let _: () = msg_send![&*label, setAccessibilityLabel: &*ns_string];
        // translatesAutoresizingMaskIntoConstraints = false for Auto Layout
        let _: () = msg_send![&*label, setTranslatesAutoresizingMaskIntoConstraints: false];

        let view: Retained<UIView> = Retained::cast_unchecked(label);
        register_widget(view)
    }
}

/// Update the text of an existing UILabel.
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let _: () = msg_send![&*view, setText: &*ns_string];
        }
    }
}

/// Update the text of an existing UILabel from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Set the text color of a UILabel (RGBA 0.0-1.0).
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color: Retained<objc2::runtime::AnyObject> = msg_send![
                objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            let _: () = msg_send![&*view, setTextColor: &*color];
        }
    }
}

/// Determine the correct target for font/text operations.
/// For UIButton, returns its titleLabel; for other views, returns the view itself.
fn font_target(view: &UIView) -> *const objc2::runtime::AnyObject {
    if let Some(btn_cls) = AnyClass::get(c"UIButton") {
        let is_button: bool = unsafe { msg_send![view, isKindOfClass: btn_cls] };
        if is_button {
            // UIButton: set font on titleLabel, not the button itself
            unsafe {
                let title_label: *const objc2::runtime::AnyObject = msg_send![view, titleLabel];
                return title_label;
            }
        }
    }
    view as *const UIView as *const objc2::runtime::AnyObject
}

/// Set the font size of a UILabel (or UIButton's titleLabel).
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let font: Retained<objc2::runtime::AnyObject> = msg_send![
                AnyClass::get(c"UIFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            let target = font_target(&view);
            if !target.is_null() {
                let _: () = msg_send![target, setFont: &*font];
            }
        }
    }
}

/// Set the font weight of a UILabel (or UIButton's titleLabel).
pub fn set_font_weight(handle: i64, size: f64, weight: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let font: Retained<objc2::runtime::AnyObject> = msg_send![
                AnyClass::get(c"UIFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat,
                weight: weight as objc2_core_foundation::CGFloat
            ];
            let target = font_target(&view);
            if !target.is_null() {
                let _: () = msg_send![target, setFont: &*font];
            }
        }
    }
}

/// Enable word wrapping on a UILabel.
/// max_width sets the preferred wrapping width (0 = use intrinsic width).
pub fn set_wraps(handle: i64, max_width: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            // Set numberOfLines = 0 for unlimited lines
            let _: () = msg_send![&*view, setNumberOfLines: 0_i64];
            // NSLineBreakByWordWrapping = 0
            let _: () = msg_send![&*view, setLineBreakMode: 0_i64];
            // Set preferred max layout width for Auto Layout wrapping
            if max_width > 0.0 {
                let _: () = msg_send![&*view, setPreferredMaxLayoutWidth: max_width];
            }
        }
    }
}

/// Set whether a UILabel is selectable (UILabel doesn't support this, no-op).
pub fn set_selectable(_handle: i64, _selectable: bool) {
    // UILabel is not selectable by default and making it so requires
    // UITextView instead. No-op for now.
}
