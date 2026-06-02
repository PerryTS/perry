//! GTK4 Combobox widget — `gtk4::Entry` + `gtk4::EntryCompletion`
//! against a `gtk4::ListStore` for as-you-type filtering (issue #475
//! / Linux parity work).
//!
//! GTK4 has `GtkDropDown` for non-editable selection, but #475 needs
//! free-text editing + dropdown suggestions. `EntryCompletion` is the
//! native primitive for that — even though GTK 4.10 marked it as a
//! transitional API, gtk4-rs 0.9's `v4_6` feature gate (which this
//! crate already uses) keeps the type usable. The fallback once the
//! API is removed is a custom `GtkEntry` + `GtkPopover` composition.

use gtk4::glib::Type as GType;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    /// Per-handle backing store so `add_item` can append more rows
    /// after construction.
    static MODELS: RefCell<HashMap<i64, gtk4::ListStore>> = RefCell::new(HashMap::new());
}

fn fire_change(callback: f64, text: &str) {
    let bytes = text.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        let arg = js_nanbox_string(header as i64);
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
        js_closure_call1(closure_ptr, arg);
    }
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

pub fn create(initial_ptr: *const u8, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let entry = gtk4::Entry::new();
    let initial = str_from_header(initial_ptr);
    if !initial.is_empty() {
        entry.set_text(initial);
    }

    // Single-column list store of strings — column 0 is the suggestion text.
    let model = gtk4::ListStore::new(&[GType::STRING]);
    let completion = gtk4::EntryCompletion::new();
    completion.set_model(Some(&model));
    completion.set_text_column(0);
    // Inline + popup matching — typing prefix highlights the rest of
    // the match in the entry, plus the popdown lists every match.
    completion.set_inline_completion(true);
    completion.set_popup_completion(true);
    completion.set_minimum_key_length(0);
    entry.set_completion(Some(&completion));

    if on_change != 0.0 {
        let on_a = on_change;
        entry.connect_activate(move |e| {
            fire_change(on_a, &e.text().to_string());
        });
        let on_m = on_change;
        completion.connect_match_selected(move |c, model, iter| {
            if let Ok(text) = model.get_value(iter, 0).get::<String>() {
                if let Some(entry) = c.entry() {
                    entry.set_text(&text);
                    entry.set_position(-1);
                }
                fire_change(on_m, &text);
            }
            gtk4::glib::Propagation::Stop
        });
    }

    let handle = super::register_widget(entry.upcast());
    MODELS.with(|m| m.borrow_mut().insert(handle, model));
    handle
}

pub fn add_item(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    MODELS.with(|m| {
        if let Some(model) = m.borrow().get(&handle) {
            let iter = model.append();
            model.set_value(&iter, 0, &value.to_value());
        }
    });
}

pub fn set_value(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    if let Some(widget) = super::get_widget(handle) {
        if let Some(entry) = widget.downcast_ref::<gtk4::Entry>() {
            entry.set_text(value);
            entry.set_position(-1);
        }
    }
}

pub fn get_value(handle: i64) -> f64 {
    let Some(widget) = super::get_widget(handle) else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let Some(entry) = widget.downcast_ref::<gtk4::Entry>() else {
        return f64::from_bits(0x7FFC_0000_0000_0001);
    };
    let text = entry.text().to_string();
    let bytes = text.as_bytes();
    unsafe {
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(header as i64)
    }
}
