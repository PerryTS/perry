//! FFI exports: multi-window, lazy-vstack, bottom nav, image gallery, table stub, iOS docs
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

// =============================================================================
// Multi-Window
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(_title: i64, _width: f64, _height: f64) -> i64 {
    0 // stub — iOS uses UIScene for multi-window
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(_window: i64, _widget: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}

// =============================================================================
// LazyVStack
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(_count: i64, _render: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(_handle: i64, _count: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}

// =============================================================================
// Issue #553 — BottomNavigation, ImageGallery, scroll-end + pull-to-refresh.
// LazyVStack itself is still a stub on iOS (UITableView wiring is a follow-up
// in its own issue), so its pull-to-refresh + scroll-end FFIs are no-ops
// here too. The ScrollView scroll-end callback IS implemented below — that's
// the version production apps actually reach for today.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_scroll_end_callback(
    _handle: i64,
    _callback: f64,
    _threshold_items: i64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_scroll_end_callback(
    handle: i64,
    callback: f64,
    threshold_px: f64,
) {
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    widgets::bottom_nav::create(on_select)
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    widgets::bottom_nav::set_selected(handle, index);
}

/// Issue #706 — set the tint color of the active tab (RGBA 0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — set the tint color of inactive tabs (RGBA 0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::bottom_nav::set_unselected_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    widgets::image_gallery::create(on_index_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    widgets::image_gallery::set_index(handle, index);
}

// =============================================================================
// Table (stub — not yet implemented on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0 // stub
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_handle: i64, _col: i64, _title_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_handle: i64, _col: i64, _width: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_handle: i64) -> i64 {
    -1
}

// =============================================================================
// iOS Documents directory (for persistent storage)
// =============================================================================

/// Returns the app's Documents directory path as a NaN-boxed string.
/// Used by hone-ide's paths.ts for persistent storage on iOS.
#[no_mangle]
pub extern "C" fn hone_get_documents_dir() -> f64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    unsafe {
        let file_manager: *const objc2::runtime::AnyObject = objc2::msg_send![
            objc2::runtime::AnyClass::get(c"NSFileManager").unwrap(),
            defaultManager
        ];
        // NSDocumentDirectory = 9, NSUserDomainMask = 1
        let urls: objc2::rc::Retained<objc2_foundation::NSArray<objc2_foundation::NSURL>> =
            objc2::msg_send![file_manager, URLsForDirectory: 9u64, inDomains: 1u64];
        let count: usize = objc2::msg_send![&*urls, count];
        if count > 0 {
            let url: *const objc2::runtime::AnyObject =
                objc2::msg_send![&*urls, objectAtIndex: 0usize];
            let path: objc2::rc::Retained<objc2_foundation::NSString> = objc2::msg_send![url, path];
            let rust_str = path.to_string();
            let bytes = rust_str.as_bytes();
            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(str_ptr as i64)
        } else {
            // Return empty string
            let str_ptr = js_string_from_bytes(std::ptr::null(), 0);
            js_nanbox_string(str_ptr as i64)
        }
    }
}

/// Wrapper for Perry codegen (some declare functions use __wrapper_ prefix).
#[no_mangle]
pub extern "C" fn __wrapper_hone_get_documents_dir() -> f64 {
    hone_get_documents_dir()
}
