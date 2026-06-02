// FFI: BottomNavigation (#553), LazyVStack refresh/scroll-end (#553),
// ImageGallery (#553), WebView (#658 Phase 2), AttributedText (#710).
//
// Issue #553 — Windows.
//
// onScrollEnd: real impl — wired into ScrollView's existing WM_VSCROLL +
// WM_MOUSEWHEEL handlers via `check_scroll_end(handle)` after each offset
// update. Backpressure matches the macOS / iOS / GTK4 contract.
//
// BottomNavigation + ImageGallery stay stubbed on Windows: Win32 has no
// native primitives for either (no equivalent of UITabBar /
// BottomNavigationView, no UIPageViewController). A real impl would need
// either custom owner-drawn child windows (~300 lines / widget) OR a
// transition to WinUI 3 — both deferred per the existing tabbar.rs Win32
// stub convention. The symbols exist so cross-platform code links cleanly
// today and the call paths flip to real impls when WinUI lands.
//
// Pull-to-refresh on LazyVStack stays no-op: no native idiom on desktop
// Windows; explicit refresh button is the convention.
use crate::widgets;

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

/// Issue #706 — Windows bottom-nav active-tab tint. State is persisted
/// on NavEntry; visual rendering waits on a future owner-drawn button
/// rewrite (Win32 standard BUTTON controls ignore WM_CTLCOLORBTN).
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::bottom_nav::set_tint_color(handle, r, g, b, a);
}

/// Issue #706 — Windows bottom-nav inactive-tabs tint.
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
    widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px)
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

// --- WebView (issue #658 Phase 2) — real Win32 backend via WebView2.
//     CoreWebView2 controller hosted in a STATIC parent HWND. Async init
//     pumps the message loop synchronously so create() blocks until the
//     widget is ready. See widgets::webview for the full impl.
#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    url_ptr: i64,
    width: f64,
    height: f64,
    ephemeral: f64,
) -> i64 {
    widgets::webview::create(url_ptr as *const u8, width, height, ephemeral)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(handle: i64, ua_ptr: i64) {
    widgets::webview::set_user_agent(handle, ua_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(handle: i64, arr_handle: i64) {
    widgets::webview::set_allowed_domains(handle, arr_handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(handle: i64, ephemeral: i64) {
    widgets::webview::set_ephemeral(handle, ephemeral)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(handle: i64, closure: f64) {
    widgets::webview::set_on_should_navigate(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(handle: i64, closure: f64) {
    widgets::webview::set_on_loaded(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(handle: i64, closure: f64) {
    widgets::webview::set_on_error(handle, closure)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(handle: i64, url_ptr: i64) {
    widgets::webview::load_url(handle, url_ptr as *const u8)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(handle: i64) {
    widgets::webview::reload(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(handle: i64) {
    widgets::webview::go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(handle: i64) {
    widgets::webview::go_forward(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(handle: i64) -> i64 {
    widgets::webview::can_go_back(handle)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(handle: i64, js_ptr: i64, callback: f64) {
    widgets::webview::evaluate_js(handle, js_ptr as *const u8, callback)
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(handle: i64) {
    widgets::webview::clear_cookies(handle)
}

// AttributedText (Issue #710) — Windows RichEdit-backed.
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
