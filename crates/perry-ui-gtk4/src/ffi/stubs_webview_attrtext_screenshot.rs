// FFI parity stubs / impls — symbols that exist on macOS / Windows / Android /
// iOS so codegen-emitted programs link uniformly. Without these, a Linux user
// who calls `Button({image: "..."})`, `TextField({onSubmit, onFocus})`, `TabBar`,
// `QRCode`, `FrameSplit`, etc. would hit `Undefined symbols: ...` at link time.
// Mirrors the macOS pattern of stubbing iOS-only widgets (tabbar / vbox /
// frame_split / scrollview pull-to-refresh) for link stability.
//
// Also: WebView (issue #658 Phase 4) — real Linux backend via WebKitGTK 6.0;
// AttributedText (Issue #710) — GTK4 GtkLabel + Pango AttrList;
// In-app screen capture (issue #918).
use crate::widgets;

/// Set an icon on a button (e.g. `Button({label, image})`). GTK4: maps the icon
/// name to GtkButton::set_icon_name (icon-naming-spec / SF-Symbols-style names).
#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

/// Wire onSubmit (Enter key → callback with current text). Real GTK4 impl.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(handle: i64, on_submit: f64) {
    widgets::textfield::set_on_submit(handle, on_submit);
}

/// Wire onFocus (gain keyboard focus → callback). Real GTK4 impl via
/// EventControllerFocus.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(handle: i64, on_focus: f64) {
    widgets::textfield::set_on_focus(handle, on_focus);
}

/// Drop focus from every registered text field (mirrors macOS `blurAll()`).
#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {
    widgets::textfield::blur_all();
}

/// QRCode widget — stub on GTK4 (mirrors macOS pre-real-impl shape on iOS-only
/// widgets). Returns 0 for "not supported"; perry's widget chain handles a 0
/// handle gracefully. Real impl pending qrcodegen + GdkPixbuf wiring.
#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(_data_ptr: i64, _size: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(_handle: i64, _data_ptr: i64) {}

/// TabBar — iOS-shaped widget (UITabBarController equivalent). Stub on
/// desktop; matches macOS / Windows shape exactly. A real GTK4 impl would
/// use GtkStackSwitcher; tracked as a follow-up.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(_on_change: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(_handle: i64, _label_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(_handle: i64, _index: i64) {}

/// ScrollView pull-to-refresh — iOS / Android idiom, no native GTK4 equivalent.
/// Stub mirrors macOS (which also stubs because AppKit has no pull-to-refresh).
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_refresh_control(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_end_refreshing(_handle: i64) {}

/// VBox / FrameSplit — iOS-internal helpers (UIStackView vertical / split-view
/// layout primitive). Other desktops stub these and route real layout through
/// VStack / SplitView. Same here.
#[no_mangle]
pub extern "C" fn perry_ui_vbox_create() -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(_parent: i64, _child: i64, _slot: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(_parent: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(_left_width: f64) -> i64 {
    0
}

#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(_parent: i64, _child: i64) {}

// --- WebView (issue #658) — stub on this platform; link-stability shape
//     matching the macOS / iOS / visionOS surface. v1 returns a 0 handle
//     and the imperative ops are no-ops; user code that imports WebView
//     still compiles and runs but the widget is invisible. Real backend
//     deferred to a later phase per #658's roadmap.
// --- WebView (issue #658 Phase 4) — real Linux backend via WebKitGTK 6.0.
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

// AttributedText (Issue #710) — GTK4 GtkLabel + Pango AttrList.
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
/// Capture the active window as a PNG and return a base64-encoded string.
/// Returns an empty string if no window is available or capture fails.
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
