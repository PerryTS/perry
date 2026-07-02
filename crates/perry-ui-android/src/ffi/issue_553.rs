//! Issue #553 follow-up exports — bottom navigation, image gallery,
//! scroll-end callbacks, webview stubs, attributed text, and in-app
//! screen capture. Originally `lib.rs` lines 2549-2837.

use crate::{catch_panic, catch_panic_void, widgets};

// =============================================================================
// Issue #553 — Real Android implementations.
//
// BottomNavigation: horizontal LinearLayout of (ImageView + TextView) tabs
// with optional badge TextView, plain android.widget.* (no Material/AndroidX
// dependency, matching the existing tabbar.rs convention).
//
// ImageGallery: HorizontalScrollView containing a LinearLayout of equal-page
// ImageViews. `set_index` calls smoothScrollTo for animated paging; user
// swipe scrolls freely (true page-snapping requires ViewPager2 / AndroidX,
// which this crate intentionally avoids).
//
// onScrollEnd: View.OnScrollChangeListener via PerryBridge.setOnScrollEndCallback
// with backpressure (re-arms only when the user scrolls back up past the
// threshold).
//
// Pull-to-refresh on LazyVStack: stays no-op — SwipeRefreshLayout requires
// AndroidX, same constraint that limits the existing scrollview impl.
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_create(on_select: f64) -> i64 {
    catch_panic("perry_ui_bottom_nav_create", || {
        widgets::bottom_nav::create(on_select)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_add_item(handle: i64, icon_ptr: i64, label_ptr: i64) {
    catch_panic_void("perry_ui_bottom_nav_add_item", || {
        widgets::bottom_nav::add_item(handle, icon_ptr as *const u8, label_ptr as *const u8)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_badge(handle: i64, index: i64, badge_ptr: i64) {
    catch_panic_void("perry_ui_bottom_nav_set_badge", || {
        widgets::bottom_nav::set_badge(handle, index, badge_ptr as *const u8)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_selected(handle: i64, index: i64) {
    catch_panic_void("perry_ui_bottom_nav_set_selected", || {
        widgets::bottom_nav::set_selected(handle, index)
    })
}
/// Issue #706 — Android bottom-nav active-tab tint. Stored on
/// BottomNavState; applied via setColorFilter + setTextColor in
/// apply_styling.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_tint_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    catch_panic_void("perry_ui_bottom_nav_set_tint_color", || {
        widgets::bottom_nav::set_tint_color(handle, r, g, b, a)
    })
}
/// Issue #706 — Android bottom-nav inactive-tabs tint.
#[no_mangle]
pub extern "C" fn perry_ui_bottom_nav_set_unselected_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    catch_panic_void("perry_ui_bottom_nav_set_unselected_tint_color", || {
        widgets::bottom_nav::set_unselected_tint_color(handle, r, g, b, a)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_refresh_control(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_end_refreshing(_handle: i64) {}
// Matches the iOS stub (perry-ui-ios/src/lib.rs). Without this export, the
// Android UI lib is missing a symbol that the TS `lazyvstackSetRowHeight`
// API lowers to, and `dlopen(libperry_app.so)` fails at launch with an
// UnsatisfiedLinkError. No-op is fine: LinearLayout sizes children by their
// own measurement; per-row height isn't applied on Android today.
#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_set_row_height(_handle: i64, _height: f64) {}
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
    catch_panic_void("perry_ui_scrollview_set_scroll_end_callback", || {
        widgets::scrollview::set_scroll_end_callback(handle, callback, threshold_px as f32)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_create(on_index_change: f64) -> i64 {
    catch_panic("perry_ui_image_gallery_create", || {
        widgets::image_gallery::create(on_index_change)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_add_image(handle: i64, url_ptr: i64, alt_ptr: i64) {
    catch_panic_void("perry_ui_image_gallery_add_image", || {
        widgets::image_gallery::add_image(handle, url_ptr as *const u8, alt_ptr as *const u8)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_image_gallery_set_index(handle: i64, index: i64) {
    catch_panic_void("perry_ui_image_gallery_set_index", || {
        widgets::image_gallery::set_index(handle, index)
    })
}

// --- WebView (issue #658) — stub on this platform; link-stability shape
//     matching the macOS / iOS / visionOS surface. v1 returns a 0 handle
//     and the imperative ops are no-ops; user code that imports WebView
//     still compiles and runs but the widget is invisible. Real backend
//     deferred to a later phase per #658's roadmap.
#[no_mangle]
pub extern "C" fn perry_ui_webview_create(
    url_ptr: i64,
    width: f64,
    height: f64,
    ephemeral: f64,
) -> i64 {
    catch_panic("perry_ui_webview_create", || {
        widgets::webview::create(url_ptr as *const u8, width, height, ephemeral)
    })
}

/// Create a BloomView render-surface host (issue #2395).
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_create(width: f64, height: f64) -> i64 {
    catch_panic("perry_ui_bloomview_create", || {
        widgets::bloomview::create(width, height)
    })
}

/// Return the BloomView's native handle token (issue #2395).
#[no_mangle]
pub extern "C" fn perry_ui_bloomview_get_hwnd(handle: i64) -> i64 {
    catch_panic("perry_ui_bloomview_get_hwnd", || {
        widgets::bloomview::get_native_handle(handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_user_agent(handle: i64, ua_ptr: i64) {
    catch_panic_void("perry_ui_webview_set_user_agent", || {
        widgets::webview::set_user_agent(handle, ua_ptr as *const u8)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_allowed_domains(handle: i64, arr_handle: i64) {
    catch_panic_void("perry_ui_webview_set_allowed_domains", || {
        widgets::webview::set_allowed_domains(handle, arr_handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_ephemeral(handle: i64, ephemeral: i64) {
    catch_panic_void("perry_ui_webview_set_ephemeral", || {
        widgets::webview::set_ephemeral(handle, ephemeral)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_should_navigate(handle: i64, closure: f64) {
    catch_panic_void("perry_ui_webview_set_on_should_navigate", || {
        widgets::webview::set_on_should_navigate(handle, closure)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_loaded(handle: i64, closure: f64) {
    catch_panic_void("perry_ui_webview_set_on_loaded", || {
        widgets::webview::set_on_loaded(handle, closure)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_set_on_error(handle: i64, closure: f64) {
    catch_panic_void("perry_ui_webview_set_on_error", || {
        widgets::webview::set_on_error(handle, closure)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_load_url(handle: i64, url_ptr: i64) {
    catch_panic_void("perry_ui_webview_load_url", || {
        widgets::webview::load_url(handle, url_ptr as *const u8)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_reload(handle: i64) {
    catch_panic_void("perry_ui_webview_reload", || {
        widgets::webview::reload(handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_back(handle: i64) {
    catch_panic_void("perry_ui_webview_go_back", || {
        widgets::webview::go_back(handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_go_forward(handle: i64) {
    catch_panic_void("perry_ui_webview_go_forward", || {
        widgets::webview::go_forward(handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_can_go_back(handle: i64) -> i64 {
    catch_panic("perry_ui_webview_can_go_back", || {
        widgets::webview::can_go_back(handle)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_evaluate_js(handle: i64, js_ptr: i64, callback: f64) {
    catch_panic_void("perry_ui_webview_evaluate_js", || {
        widgets::webview::evaluate_js(handle, js_ptr as *const u8, callback)
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_webview_clear_cookies(handle: i64) {
    catch_panic_void("perry_ui_webview_clear_cookies", || {
        widgets::webview::clear_cookies(handle)
    })
}

// AttributedText (Issue #710) — Android SpannableStringBuilder-backed.
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_create() -> i64 {
    catch_panic("perry_ui_attributed_text_create", || {
        widgets::attributed_text::create()
    })
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
    catch_panic_void("perry_ui_attributed_text_append", || {
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
        )
    })
}
#[no_mangle]
pub extern "C" fn perry_ui_attributed_text_clear(h: i64) {
    catch_panic_void("perry_ui_attributed_text_clear", || {
        widgets::attributed_text::clear(h)
    })
}

// ---- In-app screen capture (issue #918) ----
/// Capture the root View as a PNG and return a base64-encoded string.
/// Returns an empty string if capture is unavailable (e.g. the geisterhand
/// feature is OFF, the Activity is not attached, or the JNI call fails).
#[cfg(feature = "geisterhand")]
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

// Screenshot capture lives in the geisterhand renderer. Under the default
// Android Views (JNI) backend that module is configured out, so honor the
// documented contract and return an empty string instead of referencing
// the absent `crate::screenshot` (which broke the default Android build).
#[cfg(not(feature = "geisterhand"))]
#[no_mangle]
pub extern "C" fn perry_system_take_screenshot() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) as i64 }
}

/// #1475 — safe-area insets. Reports all-zero for now. A real implementation
/// reads `decorView.getRootWindowInsets().getInsets(WindowInsets.Type.systemBars())`
/// over JNI (top status bar, bottom navigation/gesture bar) and converts px→dp;
/// tracked as a follow-up so the host-tested iOS path can land first. The
/// symbol is defined here so `getSafeAreaInsets()` links on Android.
#[no_mangle]
pub extern "C" fn perry_system_get_safe_area_insets() -> f64 {
    extern "C" {
        fn perry_safe_area_insets_make(top: f64, right: f64, bottom: f64, left: f64) -> f64;
    }
    unsafe { perry_safe_area_insets_make(0.0, 0.0, 0.0, 0.0) }
}
