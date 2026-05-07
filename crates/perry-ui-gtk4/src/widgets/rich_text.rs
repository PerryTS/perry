//! GTK4 Rich text editor — `GtkTextView` + `GtkTextBuffer` with
//! pre-registered bold / italic / underline tags applied to the
//! current selection (issue #478 / Linux parity work).
//!
//! GTK4 has no built-in HTML round-trip for `GtkTextBuffer` (unlike
//! NSAttributedString's `NSHTMLTextDocumentType`). For v1 the
//! `set_html` path strips tags via a coarse heuristic and stores the
//! result as plain text; `get_html` returns the buffer's plain text
//! wrapped in `<p>...</p>`. A proper round-trip would need
//! Pango-Markup ↔ HTML conversion; tracked as a #478 follow-up.

use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

struct RichTextEntry {
    text_view: gtk4::TextView,
    buffer: gtk4::TextBuffer,
    bold_tag: gtk4::TextTag,
    italic_tag: gtk4::TextTag,
    underline_tag: gtk4::TextTag,
}

thread_local! {
    static EDITORS: RefCell<HashMap<i64, RichTextEntry>> = RefCell::new(HashMap::new());
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

pub fn create(width: f64, height: f64, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let buffer = gtk4::TextBuffer::new(None);
    let bold_tag = buffer
        .create_tag(Some("perry_bold"), &[("weight", &(700i32))])
        .expect("create bold tag");
    let italic_tag = buffer
        .create_tag(
            Some("perry_italic"),
            &[("style", &gtk4::pango::Style::Italic)],
        )
        .expect("create italic tag");
    let underline_tag = buffer
        .create_tag(
            Some("perry_underline"),
            &[("underline", &gtk4::pango::Underline::Single)],
        )
        .expect("create underline tag");

    let text_view = gtk4::TextView::with_buffer(&buffer);
    text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
    text_view.set_editable(true);
    text_view.set_monospace(false);
    text_view.set_top_margin(8);
    text_view.set_bottom_margin(8);
    text_view.set_left_margin(8);
    text_view.set_right_margin(8);

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scroll.set_min_content_width(width.max(40.0) as i32);
    scroll.set_min_content_height(height.max(40.0) as i32);
    scroll.set_child(Some(&text_view));

    if on_change != 0.0 {
        let on = on_change;
        let buf_for_signal = buffer.clone();
        buffer.connect_changed(move |_| {
            let start = buf_for_signal.start_iter();
            let end = buf_for_signal.end_iter();
            let text = buf_for_signal.text(&start, &end, false).to_string();
            let bytes = text.as_bytes();
            unsafe {
                let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let arg = js_nanbox_string(header as i64);
                let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
                js_closure_call1(closure_ptr, arg);
            }
        });
    }

    let handle = super::register_widget(scroll.upcast());
    EDITORS.with(|m| {
        m.borrow_mut().insert(
            handle,
            RichTextEntry {
                text_view,
                buffer,
                bold_tag,
                italic_tag,
                underline_tag,
            },
        );
    });
    handle
}

pub fn set_string(handle: i64, text_ptr: *const u8) {
    let s = str_from_header(text_ptr);
    EDITORS.with(|m| {
        if let Some(entry) = m.borrow().get(&handle) {
            entry.buffer.set_text(s);
        }
    });
}

pub fn get_string(handle: i64) -> f64 {
    let text = EDITORS.with(|m| {
        m.borrow().get(&handle).map(|e| {
            let start = e.buffer.start_iter();
            let end = e.buffer.end_iter();
            e.buffer.text(&start, &end, false).to_string()
        })
    });
    let text = text.unwrap_or_default();
    let bytes = text.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}

/// `set_html` v1 — strips tags via a coarse regex-free pass and stores
/// the textual content as plain text. Returns 1 if any text landed,
/// 0 otherwise. Proper HTML → Pango Markup conversion is a follow-up.
pub fn set_html(handle: i64, html_ptr: *const u8) -> i64 {
    let html = str_from_header(html_ptr);
    if html.is_empty() {
        return 0;
    }
    let plain = strip_html_tags(html);
    EDITORS.with(|m| {
        if let Some(entry) = m.borrow().get(&handle) {
            entry.buffer.set_text(&plain);
        }
    });
    if plain.is_empty() {
        0
    } else {
        1
    }
}

fn strip_html_tags(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// `get_html` v1 — wraps the buffer's plain text in `<p>...</p>` with
/// minimal HTML escaping. Pango-Markup ↔ HTML conversion is a
/// follow-up.
pub fn get_html(handle: i64) -> f64 {
    let plain = EDITORS.with(|m| {
        m.borrow().get(&handle).map(|e| {
            let start = e.buffer.start_iter();
            let end = e.buffer.end_iter();
            e.buffer.text(&start, &end, false).to_string()
        })
    });
    let plain = plain.unwrap_or_default();
    let escaped = plain
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let html = format!("<p>{}</p>", escaped);
    let bytes = html.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}

fn apply_or_remove_tag(handle: i64, tag_kind: TagKind) {
    EDITORS.with(|m| {
        let map = m.borrow();
        let Some(entry) = map.get(&handle) else {
            return;
        };
        let bounds = entry.buffer.selection_bounds();
        let (start, end) = match bounds {
            Some(b) => b,
            None => return, // no selection — nothing to format
        };
        let tag = match tag_kind {
            TagKind::Bold => &entry.bold_tag,
            TagKind::Italic => &entry.italic_tag,
            TagKind::Underline => &entry.underline_tag,
        };
        // Toggle: if start already has the tag, remove it; else apply.
        let mut probe = start.clone();
        let already_on = probe.has_tag(tag);
        if already_on {
            entry.buffer.remove_tag(tag, &start, &end);
        } else {
            entry.buffer.apply_tag(tag, &start, &end);
        }
        // Silence unused if the iter probe ever changes shape.
        let _ = probe;
    });
}

#[derive(Copy, Clone)]
enum TagKind {
    Bold,
    Italic,
    Underline,
}

pub fn toggle_bold(handle: i64) {
    apply_or_remove_tag(handle, TagKind::Bold);
}

pub fn toggle_italic(handle: i64) {
    apply_or_remove_tag(handle, TagKind::Italic);
}

pub fn toggle_underline(handle: i64) {
    apply_or_remove_tag(handle, TagKind::Underline);
}
