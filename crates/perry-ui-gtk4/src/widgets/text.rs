use gtk4::pango;
use gtk4::prelude::*;
use gtk4::Label;
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

/// Create a GtkLabel widget.
pub fn create(text_ptr: *const u8) -> i64 {
    crate::app::ensure_gtk_init();
    let text = str_from_header(text_ptr);
    let label = Label::new(Some(text));
    label.set_xalign(0.0); // Left-align text like macOS labels
    label.set_selectable(false);
    register_widget(label.upcast())
}

/// Update the text of an existing Text widget from a Rust &str.
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_text(text);
        }
    }
}

/// Update the text of an existing Text widget from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Set the text color of a Text widget using Pango markup attributes.
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            let attrs = pango::AttrList::new();
            let r16 = (r * 65535.0) as u16;
            let g16 = (g * 65535.0) as u16;
            let b16 = (b * 65535.0) as u16;
            let attr = pango::AttrColor::new_foreground(r16, g16, b16);
            attrs.insert(attr);
            label.set_attributes(Some(&attrs));
        }
    }
}

/// Set the font size of a Text widget.
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            let attrs = label.attributes().unwrap_or_else(pango::AttrList::new);
            let attr = pango::AttrSize::new((size * pango::SCALE as f64) as i32);
            attrs.insert(attr);
            label.set_attributes(Some(&attrs));
        }
    }
}

/// Set the font weight of a Text widget (0.0 = regular, 1.0 = bold).
pub fn set_font_weight(handle: i64, size: f64, weight: f64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            let attrs = label.attributes().unwrap_or_else(pango::AttrList::new);
            // Set size
            let size_attr = pango::AttrSize::new((size * pango::SCALE as f64) as i32);
            attrs.insert(size_attr);
            // Set weight: Perry uses 0.0=regular, 1.0=bold
            // Pango weight: 400=normal, 700=bold
            let pango_weight = if weight >= 0.5 {
                pango::Weight::Bold
            } else {
                pango::Weight::Normal
            };
            let weight_attr = pango::AttrInt::new_weight(pango_weight);
            attrs.insert(weight_attr);
            label.set_attributes(Some(&attrs));
        }
    }
}

/// Set whether a Text widget is selectable.
pub fn set_selectable(handle: i64, selectable: bool) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_selectable(selectable);
        }
    }
}

/// Enable word wrapping on a Text widget.
/// `max_width` is currently unused on GTK4; the label wraps to its allocated width.
pub fn set_wraps(handle: i64, _max_width: f64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            label.set_wrap(true);
            label.set_wrap_mode(pango::WrapMode::WordChar);
        }
    }
}

/// Set text decoration via Pango attributes (issue #185 Phase B).
/// `decoration`: 0=none, 1=underline, 2=strikethrough.
pub fn set_decoration(handle: i64, decoration: i64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            let attrs = label.attributes().unwrap_or_else(pango::AttrList::new);
            // Reset any prior underline/strikethrough so calls compose correctly.
            let underline = pango::AttrInt::new_underline(if decoration == 1 {
                pango::Underline::Single
            } else {
                pango::Underline::None
            });
            let strikethrough = pango::AttrInt::new_strikethrough(decoration == 2);
            attrs.insert(underline);
            attrs.insert(strikethrough);
            label.set_attributes(Some(&attrs));
        }
    }
}

/// Set the font family of a Text widget.
pub fn set_font_family(handle: i64, family_ptr: *const u8) {
    let family = str_from_header(family_ptr);
    if let Some(widget) = super::get_widget(handle) {
        if let Some(label) = widget.downcast_ref::<Label>() {
            let attrs = label.attributes().unwrap_or_else(pango::AttrList::new);
            let resolved = match family {
                "monospace" | "monospaced" => "monospace",
                "serif" => "serif",
                "sans-serif" => "sans-serif",
                other => other,
            };
            let mut font_desc = pango::FontDescription::new();
            font_desc.set_family(resolved);
            let attr = pango::AttrFontDesc::new(&font_desc);
            attrs.insert(attr);
            label.set_attributes(Some(&attrs));
        }
    }
}
