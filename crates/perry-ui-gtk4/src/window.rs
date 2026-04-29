use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static WINDOWS: RefCell<HashMap<i64, gtk4::Window>> = RefCell::new(HashMap::new());
    static NEXT_WINDOW_ID: RefCell<i64> = RefCell::new(1);
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

/// Create a new window (multi-window support).
pub fn create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let title = str_from_header(title_ptr);
    let window = gtk4::Window::new();
    window.set_title(Some(title));
    window.set_default_size(width as i32, height as i32);

    // Attach to the existing GTK application if available
    crate::app::GTK_APP.with(|ga| {
        if let Some(app) = ga.borrow().as_ref() {
            window.set_application(Some(app));
        }
    });

    let id = NEXT_WINDOW_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    WINDOWS.with(|w| w.borrow_mut().insert(id, window));
    id
}

/// Set the body (root widget) of a window.
pub fn set_body(window_handle: i64, widget_handle: i64) {
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            if let Some(widget) = crate::widgets::get_widget(widget_handle) {
                window.set_child(Some(&widget));
            }
        }
    });
}

/// Show a window.
pub fn show(window_handle: i64) {
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            window.present();
        }
    });
}

/// Close a window.
pub fn close(window_handle: i64) {
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            window.close();
        }
    });
}

/// Hide a window without destroying it.
pub fn hide(window_handle: i64) {
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            window.set_visible(false);
        }
    });
}

/// Set window size.
pub fn set_size(window_handle: i64, width: f64, height: f64) {
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            window.set_default_size(width as i32, height as i32);
        }
    });
}

/// Register a callback for focus loss.
pub fn on_focus_lost(window_handle: i64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
        fn js_closure_call0(closure: *const u8) -> f64;
    }
    WINDOWS.with(|w| {
        if let Some(window) = w.borrow().get(&window_handle) {
            let win = window.clone();
            win.connect_notify(Some("is-active"), move |window, _| {
                if !window.is_active() {
                    let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                    unsafe {
                        js_closure_call0(ptr);
                    }
                }
            });
        }
    });
}
