use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static SHEETS: RefCell<HashMap<i64, gtk4::Window>> = RefCell::new(HashMap::new());
    static NEXT_SHEET_ID: RefCell<i64> = RefCell::new(1);
}

/// Create a modal window holding the given body widget. #1033: signature
/// is `(body_handle, width, height)` to match the perry-dispatch row
/// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
pub fn create(_body_handle: i64, width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();

    let window = gtk4::Window::new();
    window.set_default_size(width as i32, height as i32);
    window.set_modal(true);
    window.set_resizable(true);
    // GTK widget hand-off across the perry-ui-gtk4 widget registry is
    // tracked separately — for now we leave the body unattached, which
    // matches the pre-#1033 behavior here. The signature fix is the
    // load-bearing change for the macOS sheet bug.

    let id = NEXT_SHEET_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    SHEETS.with(|s| s.borrow_mut().insert(id, window));
    id
}

/// Present (show) a sheet.
pub fn present(sheet_handle: i64) {
    SHEETS.with(|s| {
        if let Some(window) = s.borrow().get(&sheet_handle) {
            // Try to set transient for the active GTK app window
            crate::app::GTK_APP.with(|ga| {
                if let Some(app) = ga.borrow().as_ref() {
                    if let Some(active) = app.active_window() {
                        window.set_transient_for(Some(&active));
                    }
                }
            });
            window.present();
        }
    });
}

/// Dismiss (close) a sheet.
pub fn dismiss(sheet_handle: i64) {
    SHEETS.with(|s| {
        if let Some(window) = s.borrow().get(&sheet_handle) {
            window.close();
        }
    });
}
