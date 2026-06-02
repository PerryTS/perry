// FFI: TextField + ScrollView property setters.
use crate::widgets;

// =============================================================================
// TextField Ops
// =============================================================================

/// Focus a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

/// Set the text value of a TextField.
#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

// =============================================================================
// ScrollView
// =============================================================================

/// Set the child of a ScrollView.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

/// Scroll to make a child visible.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

/// Get scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

/// Set scroll offset.
#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// TextField — additional getters / setters
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(_handle: i64, _wraps: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_get_string(handle: i64) -> i64 {
    widgets::textfield::get_string(handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_submit(_handle: i64, _callback: f64) {
    #[cfg(feature = "geisterhand")]
    {
        extern "C" {
            fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
        }
        unsafe {
            perry_geisterhand_register(_handle, 1, 2, _callback, std::ptr::null());
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_next_key_view(_handle: i64, _next_handle: i64) {
    // Win32 handles tab navigation via WS_TABSTOP style (set by default)
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_borderless(handle: i64, borderless: f64) {
    widgets::textfield::set_borderless(handle, borderless);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_background_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::textfield::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_font_size(handle: i64, size: f64) {
    widgets::textfield::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::textfield::set_text_color(handle, r, g, b, a);
}

// =============================================================================
// TextField focus stubs
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_on_focus(_handle: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_blur_all() {}
