use gtk4::gdk;
use gtk4::prelude::*;

use std::cell::RefCell;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    /// Cached clipboard text from the last read (since GDK4 clipboard is async).
    /// We use a synchronous workaround: store the last known clipboard text.
    static LAST_CLIPBOARD_TEXT: RefCell<Option<String>> = RefCell::new(None);
}

/// Extract a &str from a *const StringHeader pointer.
use perry_ffi::copy_string_from_raw as str_from_header;

/// Read the current text from the system clipboard.
/// Returns a NaN-boxed string (f64) or TAG_UNDEFINED if empty.
///
/// Note: GDK4's clipboard API is async. We use a synchronous workaround
/// by running the main loop until the read completes.
pub fn read() -> f64 {
    let display = gdk::Display::default().expect("No default display");
    let clipboard = display.clipboard();

    // Use a synchronous approach: read_text_future and block on it
    let result: RefCell<Option<String>> = RefCell::new(None);
    let result_ref = &result;

    let main_context = gtk4::glib::MainContext::default();
    main_context.block_on(async {
        if let Ok(Some(text)) = clipboard.read_text_future().await {
            *result_ref.borrow_mut() = Some(text.to_string());
        }
    });

    let text = result.into_inner();
    if let Some(text) = text {
        let bytes = text.as_bytes();
        let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
        unsafe { js_nanbox_string(str_ptr as i64) }
    } else {
        // Return TAG_UNDEFINED
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// Write text to the system clipboard.
pub fn write(text_ptr: *const u8) {
    let text = unsafe { str_from_header(text_ptr) };
    let display = gdk::Display::default().expect("No default display");
    let clipboard = display.clipboard();
    clipboard.set_text(&text);
}
