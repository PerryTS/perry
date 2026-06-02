// FFI: widget visual styling — color, gradient, corner, shadow, opacity, border,
// context menu, control size, enabled, tooltip, rich tooltip, hidden.
use crate::{app, menu, widgets};

/// Set background color.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

/// Set background gradient.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64,
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
    widgets::set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

/// Set corner radius.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

/// Set drop shadow on a widget (issue #185 Phase B / #210 closure).
///
/// Wired via a parent-window WM_PAINT subclass that renders the shadow
/// onto the parent's surface using `AlphaBlend` against a 32bpp DIB
/// section. Per-pixel falloff is a quadratic Gaussian approximation —
/// see `widgets::paint_shadow_for_child` for the rendering math.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_shadow(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
    blur: f64,
    offset_x: f64,
    offset_y: f64,
) {
    widgets::set_shadow(handle, r, g, b, a, blur, offset_x, offset_y);
}

/// Set static opacity on a widget (issue #185 Phase B closure).
/// Currently stub-with-state; see `widgets::set_opacity` for rationale.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_opacity(handle: i64, opacity: f64) {
    widgets::set_opacity(handle, opacity);
}

/// Set border color (issue #185 Phase B closure). Stub-with-state.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::set_border_color(handle, r, g, b, a);
}

/// Set border width (issue #185 Phase B closure). Stub-with-state.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_border_width(handle: i64, width: f64) {
    widgets::set_border_width(handle, width);
}

/// Set context menu on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

/// Set control size.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set enabled/disabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Set tooltip.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    widgets::set_tooltip(handle, text_ptr as *const u8);
}

/// Rich tooltip — popup HWND hosting an arbitrary widget tree, shown
/// after the configured hover delay. Win32 ToolTip class is text-only,
/// so we roll our own popup that re-parents the content widget on show
/// and detaches it on hide. See `widgets::rich_tooltip` (#479 / #11).
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, hover_delay_ms);
}

/// Set hidden state. Triggers a layout pass so newly visible widgets get sized.
#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
    app::request_layout();
}
