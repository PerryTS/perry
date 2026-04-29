use gtk4::prelude::*;
use gtk4::PasswordEntry;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SECUREFIELD_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_SECUREFIELD_ID: RefCell<usize> = RefCell::new(1);
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
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

/// Create a password entry field.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let placeholder = str_from_header(placeholder_ptr);
    let entry = PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some(placeholder));

    let callback_id = NEXT_SECUREFIELD_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    SECUREFIELD_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_change);
    });

    entry.connect_changed(move |entry| {
        let closure_f64 = SECUREFIELD_CALLBACKS.with(|cbs| cbs.borrow().get(&callback_id).copied());
        if let Some(closure_f64) = closure_f64 {
            let text = entry.text().to_string();
            let bytes = text.as_bytes();
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
