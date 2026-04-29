use gtk4::prelude::*;
use gtk4::{ScrolledWindow, TextView};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from textarea ID to closure pointer (f64 NaN-boxed)
    static TEXTAREA_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_TEXTAREA_ID: RefCell<usize> = RefCell::new(1);
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

/// Create a multi-line GtkTextView inside a GtkScrolledWindow with an onChange callback.
/// Returns a widget handle for the outer ScrolledWindow.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let _placeholder = str_from_header(placeholder_ptr);

    let text_view = TextView::new();
    text_view.set_editable(true);
    text_view.set_wrap_mode(gtk4::WrapMode::Word);
    text_view.set_vexpand(true);
    text_view.set_hexpand(true);

    // Wrap in a ScrolledWindow for scrolling
    let scrolled = ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);
    scrolled.set_propagate_natural_height(true);
    scrolled.set_child(Some(&text_view));

    let callback_id = NEXT_TEXTAREA_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    TEXTAREA_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    // Connect to the buffer's "changed" signal
    text_view.buffer().connect_changed(move |buffer| {
        let closure_f64 = TEXTAREA_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let (start, end) = buffer.bounds();
            let text = buffer.text(&start, &end, false).to_string();
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

    // Register the ScrolledWindow as the widget (like macOS wraps NSTextView in NSScrollView)
    super::register_widget(scrolled.upcast())
}

/// Set the text content of a TextArea.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(widget) = super::get_widget(handle) {
        if let Some(scrolled) = widget.downcast_ref::<ScrolledWindow>() {
            if let Some(child) = scrolled.child() {
                if let Some(text_view) = child.downcast_ref::<TextView>() {
                    text_view.buffer().set_text(text);
                }
            }
        }
    }
}

/// Get the text content of a TextArea as a StringHeader pointer.
pub fn get_string(handle: i64) -> *const u8 {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(scrolled) = widget.downcast_ref::<ScrolledWindow>() {
            if let Some(child) = scrolled.child() {
                if let Some(text_view) = child.downcast_ref::<TextView>() {
                    let buffer = text_view.buffer();
                    let (start, end) = buffer.bounds();
                    let text = buffer.text(&start, &end, false).to_string();
                    let bytes = text.as_bytes();
                    return unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                }
            }
        }
    }
    // Return empty string
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}
