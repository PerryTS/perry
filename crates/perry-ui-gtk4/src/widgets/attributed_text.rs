//! Issue #710 — `AttributedText` on GTK4 via a GtkLabel backed by a
//! Pango AttrList. Each `append` extends the label's text and adds the
//! per-run attributes spanning the appended substring.

use gtk4::pango;
use gtk4::prelude::*;
use gtk4::Label;
use std::cell::RefCell;
use std::collections::HashMap;

use super::register_widget;

struct Buffer {
    /// Concatenated label text — used both as the GtkLabel content and
    /// to compute byte offsets for new runs.
    text: String,
    /// Accumulated Pango attributes across all runs.
    attrs: pango::AttrList,
}

thread_local! {
    static BUFFERS: RefCell<HashMap<i64, Buffer>> = RefCell::new(HashMap::new());
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

pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let label = Label::new(Some(""));
    label.set_xalign(0.0);
    label.set_wrap(true);
    label.set_wrap_mode(pango::WrapMode::WordChar);
    let handle = register_widget(label.upcast());

    BUFFERS.with(|b| {
        b.borrow_mut().insert(
            handle,
            Buffer {
                text: String::new(),
                attrs: pango::AttrList::new(),
            },
        );
    });
    handle
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
    let chunk = str_from_header(text_ptr);
    if chunk.is_empty() {
        return;
    }
    let Some(widget) = super::get_widget(handle) else {
        return;
    };
    let Some(label) = widget.downcast_ref::<Label>() else {
        return;
    };

    // `bufs` not `b` — the outer fn's `b: f64` (blue color component)
    // would otherwise be shadowed inside the closure, causing E0308 on
    // `to16(b)` below ("expected f64, found &RefCell<...>").
    BUFFERS.with(|bufs| {
        let mut map = bufs.borrow_mut();
        let Some(buf) = map.get_mut(&handle) else {
            return;
        };
        // Pango byte offsets are byte-based (UTF-8 byte indices), not
        // codepoint-based — `text.len()` is exactly the offset of the new
        // chunk's first byte.
        let start = buf.text.len() as u32;
        buf.text.push_str(chunk);
        let end = buf.text.len() as u32;

        let mut push = |mut attr: pango::Attribute| {
            attr.set_start_index(start);
            attr.set_end_index(end);
            buf.attrs.insert(attr);
        };

        // pango-rs sub-attribute types (`AttrInt`, `AttrSize`, `AttrColor`)
        // are subclasses of `pango::Attribute` in the glib type system.
        // `.upcast()` lifts them to the parent type the push closure
        // expects. Without the upcast the build fails with E0308
        // mismatched-types (introduced in PR #716, surfaced when the
        // release-packages workflow built perry-ui-gtk4 cross-arch).
        use gtk4::glib::object::Cast;
        if bold != 0 {
            push(pango::AttrInt::new_weight(pango::Weight::Bold).upcast());
        }
        if italic != 0 {
            push(pango::AttrInt::new_style(pango::Style::Italic).upcast());
        }
        if underline != 0 {
            push(pango::AttrInt::new_underline(pango::Underline::Single).upcast());
        }
        if font_size > 0.0 {
            push(pango::AttrSize::new((font_size * pango::SCALE as f64) as i32).upcast());
        }
        if a > 0.0 {
            let to16 = |v: f64| (v.clamp(0.0, 1.0) * 65535.0) as u16;
            push(pango::AttrColor::new_foreground(to16(r), to16(g), to16(b)).upcast());
        }

        label.set_text(&buf.text);
        label.set_attributes(Some(&buf.attrs));
    });
}

pub fn clear(handle: i64) {
    let Some(widget) = super::get_widget(handle) else {
        return;
    };
    let Some(label) = widget.downcast_ref::<Label>() else {
        return;
    };
    BUFFERS.with(|b| {
        if let Some(buf) = b.borrow_mut().get_mut(&handle) {
            buf.text.clear();
            buf.attrs = pango::AttrList::new();
            label.set_text("");
            label.set_attributes(Some(&buf.attrs));
        }
    });
}
