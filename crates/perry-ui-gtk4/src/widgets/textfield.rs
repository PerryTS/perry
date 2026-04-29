use gtk4::prelude::*;
use gtk4::Entry;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from entry ID to closure pointer (f64 NaN-boxed)
    static TEXTFIELD_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_TEXTFIELD_ID: RefCell<usize> = RefCell::new(1);
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Extract a &str from a *const StringHeader pointer.
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

/// Create an editable GtkEntry with a placeholder string and onChange callback.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let placeholder = str_from_header(placeholder_ptr);
    let entry = Entry::new();
    entry.set_placeholder_text(Some(placeholder));

    let callback_id = NEXT_TEXTFIELD_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    TEXTFIELD_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    entry.connect_changed(move |entry| {
        let closure_f64 = TEXTFIELD_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let text = entry.text().to_string();
            let bytes = text.as_bytes();

            // Create a StringHeader-backed string and NaN-box it
            let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
            let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };

            let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
            unsafe {
                js_closure_call1(closure_ptr as *const u8, nanboxed);
            }
        }
    });

    super::register_widget(entry.upcast())
}

/// Focus an editable text field.
pub fn focus(handle: i64) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(entry) = widget.downcast_ref::<Entry>() {
            entry.grab_focus();
        }
    }
}

/// Get the current text of an editable text field, returning a NaN-boxed string.
pub fn get_string_value(handle: i64) -> i64 {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(entry) = widget.downcast_ref::<Entry>() {
            let text = entry.text().to_string();
            let bytes = text.as_bytes();
            let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
            return str_ptr as i64;
        }
    }
    // Return empty string
    unsafe { js_string_from_bytes(std::ptr::null(), 0) as i64 }
}

/// Set whether the text field is borderless (stub).
pub fn set_borderless(handle: i64, borderless: f64) {
    let _ = (handle, borderless);
}

/// Set the background color of the text field (stub).
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let _ = (handle, r, g, b, a);
}

/// Set the font size of the text field (stub).
pub fn set_font_size(handle: i64, size: f64) {
    let _ = (handle, size);
}

/// Set the text color of the text field (stub).
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let _ = (handle, r, g, b, a);
}

/// Set the text of an editable text field from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(widget) = super::get_widget(handle) {
        if let Some(entry) = widget.downcast_ref::<Entry>() {
            entry.set_text(text);
        }
    }
}
