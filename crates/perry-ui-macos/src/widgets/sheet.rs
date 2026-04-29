use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;

thread_local! {
    static SHEETS: RefCell<Vec<Retained<NSWindow>>> = RefCell::new(Vec::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create a sheet (NSPanel). Returns 1-based handle.
pub fn create(width: f64, height: f64, title_ptr: *const u8) -> i64 {
    let title = str_from_header(title_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let style =
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable;
        let frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width, height));
        let panel = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        let ns_title = NSString::from_str(title);
        panel.setTitle(&ns_title);

        SHEETS.with(|s| {
            let mut sheets = s.borrow_mut();
            sheets.push(panel);
            sheets.len() as i64
        })
    }
}

/// Present a sheet on the key window.
pub fn present(sheet_handle: i64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    SHEETS.with(|s| {
        let sheets = s.borrow();
        let idx = (sheet_handle - 1) as usize;
        if idx < sheets.len() {
            let sheet = &sheets[idx];
            unsafe {
                if let Some(key_window) = app.keyWindow() {
                    let _: () = msg_send![&*key_window, beginSheet: &**sheet, completionHandler: std::ptr::null::<AnyObject>()];
                }
            }
        }
    });
}

/// Dismiss a sheet.
pub fn dismiss(sheet_handle: i64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    SHEETS.with(|s| {
        let sheets = s.borrow();
        let idx = (sheet_handle - 1) as usize;
        if idx < sheets.len() {
            let sheet = &sheets[idx];
            unsafe {
                if let Some(key_window) = app.keyWindow() {
                    let _: () = msg_send![&*key_window, endSheet: &**sheet];
                }
            }
        }
    });
}

/// Set the body of a sheet (set content view to a widget).
pub fn set_body(sheet_handle: i64, widget_handle: i64) {
    SHEETS.with(|s| {
        let sheets = s.borrow();
        let idx = (sheet_handle - 1) as usize;
        if idx < sheets.len() {
            if let Some(view) = super::get_widget(widget_handle) {
                sheets[idx].setContentView(Some(&view));
            }
        }
    });
}
