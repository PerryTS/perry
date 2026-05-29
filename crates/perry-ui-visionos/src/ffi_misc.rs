//! Bottom navigation, lazy-vstack/scrollview scroll-end stubs, image
//! gallery stubs, perry/background, attributed text, and in-app screen
//! capture FFI exports. Behavior is unchanged from the pre-split `lib.rs`.

use super::*;

// =============================================================================
// Issue #553 — BottomNavigation, pull-to-refresh on LazyVStack, onScrollEnd,
// ImageGallery. Stub block — these widgets aren't natively implemented on
// this platform yet; the symbols exist so cross-platform code compiles
// without conditional branching. Real macOS + iOS implementations live in
// perry-ui-macos and perry-ui-ios. Filling in the platform-specific
// equivalents (BottomNavigationView on Android, GtkBox+ToggleButton on
// GTK4, custom XAML-style strip on Windows, UIPageViewController flavors
// for tvOS/watchOS/visionOS) is tracked in the same issue.
// =============================================================================

/// Issue #553 + #706 — visionOS BottomNavigation backed by UITabBar.
/// visionOS uses UIKit too, so the same widget code as iOS works.
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
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(h: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(h, r, g, b, a);
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    h: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::bottom_nav::set_unselected_tint_color(h, r, g, b, a);
}

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
    _handle: i64,
    _callback: f64,
    _threshold_px: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(_on_index_change: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(_handle: i64, _url_ptr: i64, _alt_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(_handle: i64, _index: i64) {}

// ---- perry/background (issue #538) — BGTaskScheduler on visionOS 1.0+ ----
#[no_mangle]
pub extern "C" fn perry_background_register_task(identifier_ptr: i64, handler: f64) {
    background::register_task(identifier_ptr as *const u8, handler);
}
#[no_mangle]
pub extern "C" fn perry_background_schedule(
    identifier_ptr: i64,
    kind_ptr: i64,
    earliest_start_ms: f64,
    requires_network: f64,
    requires_charging: f64,
) {
    background::schedule(
        identifier_ptr as *const u8,
        kind_ptr as *const u8,
        earliest_start_ms,
        requires_network,
        requires_charging,
    );
}
#[no_mangle]
pub extern "C" fn perry_background_cancel(identifier_ptr: i64) {
    background::cancel(identifier_ptr as *const u8);
}

// AttributedText (Issue #710) — visionOS UIKit-backed impl, mirrors iOS.
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    widgets::attributed_text::create()
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_append(
    h: i64,
    t: i64,
    bold: i64,
    italic: i64,
    underline: i64,
    font_size: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::attributed_text::append(
        h,
        t as *const u8,
        bold,
        italic,
        underline,
        font_size,
        r,
        g,
        b,
        a,
    );
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_clear(h: i64) {
    widgets::attributed_text::clear(h);
}

// ---- In-app screen capture (issue #918) ----
/// Capture the key window as a PNG and return a base64-encoded string.
/// Returns an empty string if no key window is available or capture fails.
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    use base64::Engine as _;
    unsafe {
        let mut len: usize = 0;
        let ptr = crate::screenshot::perry_ui_screenshot_capture(&mut len as *mut usize);
        if ptr.is_null() || len == 0 {
            return js_string_from_bytes(std::ptr::null(), 0) as i64;
        }
        let bytes = std::slice::from_raw_parts(ptr, len);
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        libc::free(ptr as *mut libc::c_void);
        js_string_from_bytes(encoded.as_ptr(), encoded.len() as i64) as i64
    }
}

/// #1475 — safe-area insets. Not yet wired to visionOS ornaments/safe area;
/// report all-zero so the symbol links. (Follow-up can read the volume's
/// safe-area geometry.)
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    unsafe { perry_safe_area_insets_make(0.0, 0.0, 0.0, 0.0) }
}
