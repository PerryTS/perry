use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2_foundation::NSString;
use objc2_ui_kit::{UILabel, UIView};
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
        let label: Retained<UILabel> =
            msg_send![objc2::runtime::AnyClass::get(c"UILabel").unwrap(), new];
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

/// Set the text color (RGBA 0.0-1.0). Routes by widget kind:
/// - UIButton    → `super::button::set_text_color` (uses
///                 `setTitleColor:forState:UIControlStateNormal`)
/// - UILabel     → `setTextColor:`
/// - other       → silent no-op (matches the codegen's documented intent
///                 — `apply_inline_style` routes every `color: ...` prop
///                 through `text_set_color`, including widgets like
///                 Button that don't respond to `setTextColor:` and
///                 would raise `unrecognized selector` → non-unwinding
///                 panic across the FFI boundary → process abort)
///
/// Issue #1107 — on iOS 26 devices a partial-alpha UIColor passed to
/// `setTextColor:` causes UILabel to render zero glyphs (alpha == 1.0
/// works; iOS 17 simulator works; iOS 26 device fails). The
/// `AttributedText` widget's path (UIColor as the `NSColor`/
/// `NSForegroundColorAttributeName` value inside an
/// `NSAttributedString`'s attributes dict, applied via
/// `setAttributedText:`) is the one path the reporter confirmed renders
/// correctly with sub-1.0 alpha. So for the alpha < 1.0 case we mirror
/// that path: read the label's current `text` + `font`, build an
/// `NSAttributedString` with `NSFont` + `NSColor` attrs, and call
/// `setAttributedText:`. `textColor` is still set as well so future
/// `setText:` calls (which clobber the attributed buffer) still pick up
/// at least the solid-color approximation rather than reverting to
/// system default.
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let Some(view) = super::get_widget(handle) else {
        return;
    };
    unsafe {
        if let Some(btn_cls) = AnyClass::get(c"UIButton") {
            let is_btn: bool = msg_send![&*view, isKindOfClass: btn_cls];
            if is_btn {
                drop(view);
                super::button::set_text_color(handle, r, g, b, a);
                return;
            }
        }
        if let Some(lbl_cls) = AnyClass::get(c"UILabel") {
            let is_lbl: bool = msg_send![&*view, isKindOfClass: lbl_cls];
            if !is_lbl {
                return;
            }
        }
        let color: Retained<objc2::runtime::AnyObject> = msg_send![
            AnyClass::get(c"UIColor").unwrap(),
            colorWithRed: r as objc2_core_foundation::CGFloat,
            green: g as objc2_core_foundation::CGFloat,
            blue: b as objc2_core_foundation::CGFloat,
            alpha: a as objc2_core_foundation::CGFloat
        ];
        let _: () = msg_send![&*view, setTextColor: &*color];

        // iOS 26 partial-alpha workaround (issue #1107).
        // alpha == 1.0 is unaffected by the bug, keep the simple
        // setTextColor: path so we don't disturb intrinsic-content sizing.
        if a < 1.0 {
            apply_label_color_via_attributed(&view, &color);
        } else {
            // Clear any prior attributedText we may have set so the plain
            // textColor path takes effect again.
            clear_label_attributed_text(&view);
        }
    }
}

/// Issue #1107 / #1122 workaround — mirror `AttributedText::append`'s
/// code path so a partial-alpha NSColor actually paints glyphs on iOS 26.
///
/// PR #1109's first attempt at this read `view.font` and `view.text`
/// straight off the label and passed them through. That didn't paint on
/// real device — the borrowed-font reference plus the un-retained
/// NSAttributedString result both differ from `AttributedText::append`'s
/// path. This version builds a fresh `[UIFont systemFontOfSize:]` from
/// the label's current point size and retains the resulting
/// NSAttributedString, exactly like the AttributedText widget.
unsafe fn apply_label_color_via_attributed(view: &UIView, color: &objc2::runtime::AnyObject) {
    use objc2::runtime::AnyObject;

    let current_text: *const objc2_foundation::NSString = msg_send![view, text];
    if current_text.is_null() {
        return;
    }
    let length: u64 = msg_send![current_text, length];
    if length == 0 {
        return;
    }

    let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
    let attrs: Retained<AnyObject> = msg_send![dict_cls, new];

    // Build a fresh UIFont rather than re-using the label's borrowed
    // font pointer — this matches AttributedText's working path. Pull
    // the size off the existing font (defaulting to UILabel's 17pt
    // default) so any prior textSetFontSize is preserved.
    let existing_font: *mut AnyObject = msg_send![view, font];
    let size: objc2_core_foundation::CGFloat = if !existing_font.is_null() {
        msg_send![existing_font, pointSize]
    } else {
        17.0
    };
    let font_cls = AnyClass::get(c"UIFont").unwrap();
    let fresh_font: Retained<AnyObject> = msg_send![
        font_cls,
        systemFontOfSize: size
    ];
    let font_key = NSString::from_str("NSFont");
    let _: () = msg_send![&*attrs, setObject: &*fresh_font, forKey: &*font_key];

    let color_key = NSString::from_str("NSColor");
    let _: () = msg_send![&*attrs, setObject: color, forKey: &*color_key];

    let attr_cls = AnyClass::get(c"NSAttributedString").unwrap();
    let alloc: *mut AnyObject = msg_send![attr_cls, alloc];
    // Retain — initWithString:attributes: returns a +1 retain that we
    // own. setAttributedText: copies the value, but holding the retain
    // until after that call avoids a partially-constructed object being
    // observed by UIKit if the autorelease pool drains unexpectedly.
    let raw: *mut AnyObject = msg_send![
        alloc,
        initWithString: current_text,
        attributes: &*attrs
    ];
    if let Some(attr_str) = Retained::from_raw(raw) {
        let _: () = msg_send![view, setAttributedText: &*attr_str];
    }
}

/// Counterpart to `apply_label_color_via_attributed` — for alpha == 1.0
/// (or color cleared) we want plain `textColor` rendering to win again,
/// so explicitly drop the attributedText we may have set earlier by
/// rebuilding it from the current plain string with no attributes.
unsafe fn clear_label_attributed_text(view: &UIView) {
    use objc2::runtime::AnyObject;
    let current_text: *const objc2_foundation::NSString = msg_send![view, text];
    if current_text.is_null() {
        return;
    }
    // Re-issuing setText: with the same string forces UILabel to
    // discard any internal attributedText state. This is cheaper than
    // building a no-op NSAttributedString.
    let _: () = msg_send![view, setText: current_text as *const AnyObject];
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

/// Issue #707 — cap the maximum number of visible lines. 0 = unlimited.
/// Maps directly to UILabel.numberOfLines. Pair with `set_truncation_mode`
/// to control where the ellipsis appears when content overflows.
pub fn set_number_of_lines(handle: i64, lines: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            if let Some(lbl_cls) = AnyClass::get(c"UILabel") {
                let is_lbl: bool = msg_send![&*view, isKindOfClass: lbl_cls];
                if !is_lbl {
                    return;
                }
            }
            let _: () = msg_send![&*view, setNumberOfLines: lines];
            // Make sure lineBreakMode allows truncation when capped > 0.
            // 0=WordWrapping, 1=CharWrapping, 2=Clipping, 3=TruncatingHead,
            // 4=TruncatingTail, 5=TruncatingMiddle. We leave the existing
            // mode unless it's the default (0) and we just enabled a cap,
            // in which case tail-truncation is the natural fallback.
            if lines > 0 {
                let current: i64 = msg_send![&*view, lineBreakMode];
                if current == 0 {
                    let _: () = msg_send![&*view, setLineBreakMode: 4u64];
                }
            }
        }
    }
}

/// Issue #707 — control where the ellipsis appears when text overflows
/// the line cap. 0=word-wrap (no truncation), 1=head ("…foo"),
/// 2=middle ("fo…ar"), 3=tail ("foo…"). Tail is the most common.
pub fn set_truncation_mode(handle: i64, mode: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            if let Some(lbl_cls) = AnyClass::get(c"UILabel") {
                let is_lbl: bool = msg_send![&*view, isKindOfClass: lbl_cls];
                if !is_lbl {
                    return;
                }
            }
            // Map our public 0..3 → NSLineBreakMode values.
            let lbm: u64 = match mode {
                1 => 3, // head
                2 => 5, // middle
                3 => 4, // tail
                _ => 0, // word-wrap
            };
            let _: () = msg_send![&*view, setLineBreakMode: lbm];
        }
    }
}

/// Set text decoration on a UILabel via `NSAttributedString` (issue #185
/// Phase B). `decoration`: 0=none, 1=underline, 2=strikethrough.
pub fn set_decoration(handle: i64, decoration: i64) {
    use objc2::runtime::{AnyClass, AnyObject};
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let label: &UILabel = &*(Retained::as_ptr(&view) as *const UILabel);
            let current: Retained<objc2_foundation::NSString> = msg_send![label, text];
            if decoration == 0 {
                let _: () = msg_send![label, setText: &*current];
                return;
            }
            let key = objc2_foundation::NSString::from_str(if decoration == 1 {
                "NSUnderline"
            } else {
                "NSStrikethrough"
            });
            let num_cls = AnyClass::get(c"NSNumber").unwrap();
            let one: Retained<AnyObject> = msg_send![num_cls, numberWithInt: 1i32];
            let attrs: Retained<AnyObject> = msg_send![
                AnyClass::get(c"NSDictionary").unwrap(),
                dictionaryWithObject: &*one,
                forKey: &*key
            ];
            let ns_str: *const AnyObject = Retained::as_ptr(&current) as *const AnyObject;
            let cls = AnyClass::get(c"NSAttributedString").unwrap();
            let alloc: *mut AnyObject = msg_send![cls, alloc];
            let attr_str: *mut AnyObject = msg_send![
                alloc,
                initWithString: ns_str,
                attributes: &*attrs
            ];
            let _: () = msg_send![label, setAttributedText: attr_str];
        }
    }
}
