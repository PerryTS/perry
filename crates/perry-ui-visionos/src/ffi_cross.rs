//! Cross-cutting FFI exports: enabled, tooltip, control size, hover/click,
//! animations, and state on-change. Behavior is unchanged from the
//! pre-split `lib.rs`.

use super::*;

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

/// Set the enabled state of a widget. enabled: 0=disabled, 1=enabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Rich tooltip (issue #479). visionOS — long-press to show (same
/// UIKit primitive as iOS; the gaze/pinch interaction model is a
/// future native-spatial enhancement). `hover_delay_ms` is the
/// UILongPressGestureRecognizer minimumPressDuration.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_rich_tooltip(
    handle: i64,
    content_handle: i64,
    hover_delay_ms: f64,
) {
    let ms = if hover_delay_ms.is_finite() && hover_delay_ms > 0.0 {
        hover_delay_ms as u32
    } else {
        0
    };
    widgets::rich_tooltip::set_rich_tooltip(handle, content_handle, ms);
}

/// Set a tooltip on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    widgets::set_tooltip(handle, str_from_header(text_ptr as *const u8));
}

/// Set the control size of a widget. 0=regular, 1=small, 2=mini, 3=large.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set an on-hover callback for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

/// Set a single-tap handler for any widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_click(handle: i64, callback: f64) {
    widgets::set_on_click(handle, callback);
}

/// Set a double-click/tap handler for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

/// Animate the opacity of a widget. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_secs: f64) {
    widgets::animate_opacity(handle, target, duration_secs);
}

/// Animate the position of a widget by delta. `duration_secs` is in seconds.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(
    handle: i64,
    dx: f64,
    dy: f64,
    duration_secs: f64,
) {
    widgets::animate_position(handle, dx, dy, duration_secs);
}

/// Register an onChange callback for a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::state_on_change(state_handle, callback);
}
