//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// perry/media — streaming media playback (issue #351). AVPlayer-backed.
// See `media_playback.rs` for the implementation; everything below is a
// thin FFI thunk that the codegen-emitted `perry_media_*` declarations
// resolve to at link time.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_media_create_player(url_ptr: i64) -> i64 {
    media_playback::create_player(url_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_media_play(handle: f64) {
    media_playback::play(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_pause(handle: f64) {
    media_playback::pause(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_stop(handle: f64) {
    media_playback::stop(handle);
}

#[no_mangle]
pub extern "C" fn perry_media_seek(handle: f64, seconds: f64) {
    media_playback::seek(handle, seconds);
}

#[no_mangle]
pub extern "C" fn perry_media_set_volume(handle: f64, volume: f64) {
    media_playback::set_volume(handle, volume);
}

#[no_mangle]
pub extern "C" fn perry_media_set_rate(handle: f64, rate: f64) {
    media_playback::set_rate(handle, rate);
}

#[no_mangle]
pub extern "C" fn perry_media_get_current_time(handle: f64) -> f64 {
    media_playback::get_current_time(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_duration(handle: f64) -> f64 {
    media_playback::get_duration(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_get_state(handle: f64) -> i64 {
    media_playback::get_state(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_is_playing(handle: f64) -> f64 {
    media_playback::is_playing(handle)
}

#[no_mangle]
pub extern "C" fn perry_media_on_state_change(handle: f64, closure: f64) {
    media_playback::on_state_change(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_on_time_update(handle: f64, closure: f64) {
    media_playback::on_time_update(handle, closure);
}

#[no_mangle]
pub extern "C" fn perry_media_set_now_playing(
    handle: f64,
    title_ptr: i64,
    artist_ptr: i64,
    album_ptr: i64,
    artwork_ptr: i64,
) {
    media_playback::set_now_playing(
        handle,
        title_ptr as *const u8,
        artist_ptr as *const u8,
        album_ptr as *const u8,
        artwork_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_media_destroy(handle: f64) {
    media_playback::destroy(handle);
}

// =============================================================================
// Issue #553 — BottomNavigation, pull-to-refresh on LazyVStack, onScrollEnd,
// ImageGallery. Stub block — these widgets aren't natively implemented on
// this platform yet; the symbols exist so cross-platform code compiles
// without conditional branching. Real macOS + iOS implementations live in
// perry-ui-macos and perry-ui-ios. Filling in the platform-specific
// equivalents (BottomNavigationView on Android, GtkBox+ToggleButton on
// GTK4, custom XAML-style strip on Windows, UIPageViewController flavors
// for tvOS/watchOS/visionOS) is tracked in the same issue.

/// Issue #553 + #706 — tvOS BottomNavigation backed by UITabBar.
/// tvOS supports UITabBar natively; the same widget code as iOS works
/// (focus-engine navigation is layered on top by UIKit automatically).
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

// ---- perry/background (issue #538) — BGTaskScheduler on tvOS 13+ ----
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

// --- WebView (issue #658) — stub on this platform; link-stability shape
//     matching the macOS / iOS / visionOS surface. v1 returns a 0 handle
//     and the imperative ops are no-ops; user code that imports WebView
//     still compiles and runs but the widget is invisible. Real backend
//     deferred to a later phase per #658's roadmap.
#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    _url_ptr: i64,
    _width: f64,
    _height: f64,
    _ephemeral: f64,
) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(_handle: i64, _ua_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(_handle: i64, _arr_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(_handle: i64, _ephemeral: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(_handle: i64, _closure: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(_handle: i64, _url_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(_handle: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(_handle: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(_handle: i64, _js_ptr: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(_handle: i64) {}

// AttributedText (Issue #710) — tvOS UIKit-backed impl, mirrors iOS.
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

/// #1475 — safe-area insets. tvOS overscan safe area is not yet exposed here;
/// report all-zero so the symbol links. (Follow-up can read the focus
/// environment's safe-area layout guide.)
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    unsafe { perry_safe_area_insets_make(0.0, 0.0, 0.0, 0.0) }
}
