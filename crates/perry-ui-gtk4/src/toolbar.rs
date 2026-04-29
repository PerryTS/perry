use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TOOLBARS: RefCell<HashMap<i64, gtk4::HeaderBar>> = RefCell::new(HashMap::new());
    static NEXT_TOOLBAR_ID: RefCell<i64> = RefCell::new(1);
    static TOOLBAR_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_TB_CB_ID: RefCell<usize> = RefCell::new(1);
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

/// Create a toolbar (HeaderBar).
pub fn create() -> i64 {
    crate::app::ensure_gtk_init();
    let header = gtk4::HeaderBar::new();

    let id = NEXT_TOOLBAR_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    TOOLBARS.with(|t| t.borrow_mut().insert(id, header));
    id
}

/// Add a button item to the toolbar. icon_ptr is a named icon (or empty).
pub fn add_item(toolbar_handle: i64, label_ptr: *const u8, icon_ptr: *const u8, callback: f64) {
    let label = str_from_header(label_ptr);
    let icon = str_from_header(icon_ptr);

    TOOLBARS.with(|t| {
        if let Some(header) = t.borrow().get(&toolbar_handle) {
            let button = if !icon.is_empty() {
                gtk4::Button::from_icon_name(icon)
            } else {
                gtk4::Button::with_label(label)
            };

            let cb_id = NEXT_TB_CB_ID.with(|id| {
                let mut id = id.borrow_mut();
                let current = *id;
                *id += 1;
                current
            });

            TOOLBAR_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(cb_id, callback);
            });

            button.connect_clicked(move |_| {
                let closure_f64 = TOOLBAR_CALLBACKS.with(|cbs| cbs.borrow().get(&cb_id).copied());
                if let Some(closure_f64) = closure_f64 {
                    let ptr = unsafe { js_nanbox_get_pointer(closure_f64) } as *const u8;
                    unsafe {
                        js_closure_call0(ptr);
                    }
                }
            });

            header.pack_end(&button);
        }
    });
}

/// Attach a toolbar to the current app window (set as titlebar).
pub fn attach(toolbar_handle: i64) {
    TOOLBARS.with(|t| {
        if let Some(header) = t.borrow().get(&toolbar_handle) {
            crate::app::GTK_APP.with(|ga| {
                if let Some(app) = ga.borrow().as_ref() {
                    if let Some(window) = app.active_window() {
                        window.set_titlebar(Some(header));
                    }
                }
            });
        }
    });
}
