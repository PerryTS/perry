use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSApplication, NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::RefCell;

thread_local! {
    static SHEETS: RefCell<Vec<Retained<NSWindow>>> = const { RefCell::new(Vec::new()) };
}

/// Create a sheet (NSPanel) and install `body_handle` as its content view.
/// Returns the 1-based sheet handle.
///
/// #1033: the TS surface is `sheetCreate(body, width, height): Widget` and
/// the perry-dispatch row matches (`[Widget, F64, F64]`), but the FFI
/// previously took `(width, height, title_ptr)`. On AArch64 the dispatch
/// passed the body handle in X0 and the dimensions in D0/D1, so the
/// dimensions landed in the right registers by luck and the body handle
/// was silently dropped — producing a blank sheet at the requested size.
/// Aligning the FFI signature with the dispatch closes the gap.
pub fn create(body_handle: i64, width: f64, height: f64) -> i64 {
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
        // Sheets in the TS surface take no title arg; AppKit still
        // requires an NSString, so set the empty string — modern macOS
        // sheets typically render without titlebar text anyway.
        let ns_title = NSString::from_str("");
        panel.setTitle(&ns_title);

        if let Some(view) = super::get_widget(body_handle) {
            panel.setContentView(Some(&view));
        }

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
