//! Image widget, navigation stack, widget styling (enabled/tooltip/control
//! size/corner radius/shadow/background/border/insets/opacity), events
//! (hover/double-click), animation, dialog, sheet, multi-window and
//! toolbar exports. Originally `lib.rs` lines 1095-1409.

use crate::{catch_panic_void, dialog, sheet, toolbar, widgets, window};

// =============================================================================
// Image
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// Create an AdBanner (#867). `unit_id_ptr` / `size_ptr` are string
/// headers. Default build is a layout placeholder sized to the banner
/// slot; a live `AdView` (play-services-ads) is a follow-up. Returns
/// widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_adbanner_create(unit_id_ptr: i64, size_ptr: i64) -> i64 {
    widgets::adbanner::create(unit_id_ptr as *const u8, size_ptr as *const u8)
}

/// #635 stub: remote URL images aren't fetched on Android yet —
/// register an empty image widget so layout still works.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_url(_url_ptr: i64, _alt_ptr: i64) -> i64 {
    widgets::image::create_symbol(0 as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

// =============================================================================
// Navigation
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_navstack_create() -> i64 {
    // Matches the 0-arg dispatch in perry-dispatch::PERRY_UI_TABLE.
    widgets::navstack::create(std::ptr::null(), 0)
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

// =============================================================================
// Styling (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    widgets::set_tooltip(handle, text_ptr as *const u8);
}

/// Rich tooltip (issue #479) — long-press on `handle` pops up a
/// `PopupWindow` hosting the subtree at `content_handle`. `hover_delay_ms`
/// is ignored on Android (touch devices have no hover model); the system
/// long-press duration is used instead.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, hover_delay_ms);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: f64, radius: f64) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::set_corner_radius(h, radius);
}

/// Set drop shadow via Material `setElevation` + (API 28+)
/// `setOutlineSpotShadowColor` / `setOutlineAmbientShadowColor`. See
/// `widgets::set_shadow` for the full mapping rationale (issue #185 Phase B).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::set_shadow(h, r, g, b, a, blur, offset_x, offset_y);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: f64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::set_background_color(h, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: f64,
    r1: f64,
    g1: f64,
    b1: f64,
    a1: f64,
    r2: f64,
    g2: f64,
    b2: f64,
    a2: f64,
    direction: f64,
) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::set_background_gradient(h, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: f64, r: f64, g: f64, b: f64, a: f64) {
    catch_panic_void("perry_ui_widget_set_border_color", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_border_color(h, r, g, b, a)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: f64, width: f64) {
    catch_panic_void("perry_ui_widget_set_border_width", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_border_width(h, width)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_edge_insets(
    handle: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) {
    catch_panic_void("perry_ui_widget_set_edge_insets", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_edge_insets(h, top, left, bottom, right)
    });
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: f64, alpha: f64) {
    catch_panic_void("perry_ui_widget_set_opacity", || {
        let h = widgets::decode_js_handle_f64(handle);
        widgets::set_opacity(h, alpha)
    });
}

// =============================================================================
// Events (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

/// Continuous pointer events (issue #1868). Backed by a
/// `View.OnTouchListener` installed through `PerryBridge.setOnPointerCallbacks`.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_down(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_down(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_up(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_up(handle, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_mouse_move(handle: i64, callback: f64) {
    crate::pointer::set_on_mouse_move(handle, callback);
}

// =============================================================================
// Animation (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

// =============================================================================
// Dialog (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    dialog::save_file_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_alert(
    title_ptr: i64,
    message_ptr: i64,
    buttons_ptr: i64,
    callback: f64,
) {
    dialog::alert(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr as *const u8,
        callback,
    );
}

// =============================================================================
// Sheet (new)
// =============================================================================

// #1033: signature aligned with the perry-dispatch row
// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(body_handle: i64, width: f64, height: f64) -> i64 {
    sheet::create(body_handle, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(sheet_handle: i64) {
    sheet::present(sheet_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(sheet_handle: i64) {
    sheet::dismiss(sheet_handle);
}

// =============================================================================
// Multi-Window (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    window::create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(window_handle: i64, widget_handle: i64) {
    window::set_body(window_handle, widget_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(window_handle: i64) {
    window::show(window_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(window_handle: i64) {
    window::close(window_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_window_hide(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_size(_window: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_on_focus_lost(_window: i64, _callback: f64) {}

// =============================================================================
// Toolbar (new)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    toolbar::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    toolbar_handle: i64,
    label_ptr: i64,
    icon_ptr: i64,
    callback: f64,
) {
    toolbar::add_item(
        toolbar_handle,
        label_ptr as *const u8,
        icon_ptr as *const u8,
        callback,
    );
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(toolbar_handle: i64) {
    toolbar::attach(toolbar_handle);
}

/// Load an image asset for Canvas.drawImage. Native backends expose the FFI
/// symbol now; platform decoding/drawing support can fill this handle in.
#[no_mangle]
pub extern "C" fn perry_ui_load_image(url_ptr: i64) -> i64 {
    widgets::canvas::load_image(url_ptr as *const u8)
}
