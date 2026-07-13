//! Text mutation / layout insets / ScrollView / Clipboard / Keyboard
//! shortcuts / text & button styling / focus & scroll-to. Originally
//! `lib.rs` lines 402-682 ("Phase A.1" – "Phase A.4" sections).

use crate::{app, catch_panic_void, clipboard, widgets};

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

// =============================================================================
// Phase A.1: Text Mutation & Layout Control
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(
    spacing: f64,
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
) -> i64 {
    widgets::hstack::create_with_insets(spacing, top, left, bottom, right)
}

// =============================================================================
// Phase A.2: ScrollView, Clipboard & Keyboard Shortcuts
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"perry_ui_scrollview_create: called\0".as_ptr(),
        );
    }
    let h = widgets::scrollview::create();
    unsafe {
        __android_log_print(
            3,
            b"PerryJNI\0".as_ptr(),
            b"perry_ui_scrollview_create: returned handle=%lld\0".as_ptr(),
            h,
        );
    }
    h
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(_key: i64, _mods: f64, _cb: f64) {}

// Continuous keyboard events (issue #1864). PerryActivity.dispatchKeyEvent
// bridges via `nativeDispatchKey` (see crate::keyboard); these setters just
// store handlers in the shared dispatcher.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_down(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_down(handle, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_key_up(handle: i64, cb: f64) {
    crate::keyboard::set_on_key_up(handle, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_down(cb: f64) {
    crate::keyboard::set_on_key_down(0, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_app_set_on_key_up(cb: f64) {
    crate::keyboard::set_on_key_up(0, cb);
}
#[no_mangle]
pub extern "C" fn perry_ui_focus_widget(handle: i64) {
    crate::keyboard::focus_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_blur_widget(handle: i64) {
    crate::keyboard::blur_widget(handle);
}
#[no_mangle]
pub extern "C" fn perry_ui_is_key_down(code: f64) -> i32 {
    let raw = code as i32;
    if !(0..=u16::MAX as i32).contains(&raw) {
        return 0;
    }
    if crate::keyboard::is_key_down(raw as u16) {
        1
    } else {
        0
    }
}
#[no_mangle]
pub extern "C" fn perry_ui_current_modifiers() -> i32 {
    crate::keyboard::current_modifiers() as i32
}

#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(_path: i64) -> i64 {
    0
}

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: f64, r: f64, g: f64, b: f64, a: f64) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::text::set_color(h, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: f64, size: f64) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::text::set_font_size(h, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: f64, size: f64, weight: f64) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::text::set_font_weight(h, size, weight);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Text decoration (issue #185 Phase B). 0=none, 1=underline, 2=strikethrough.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

/// Issue #707 — cap visible lines on a Text widget (TextView.setMaxLines).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    catch_panic_void("perry_ui_text_set_number_of_lines", || {
        widgets::text::set_number_of_lines(handle, lines)
    })
}
/// Issue #707 — truncation mode (TextView.setEllipsize).
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    catch_panic_void("perry_ui_text_set_truncation_mode", || {
        widgets::text::set_truncation_mode(handle, mode)
    })
}

/// Issue #3621 — horizontal text alignment (TextView.setGravity).
/// `alignment`: 0=left, 1=right, 2=center, 3=justified, 4=natural.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_text_alignment(handle: i64, alignment: i64) {
    catch_panic_void("perry_ui_text_set_text_alignment", || {
        widgets::text::set_text_alignment(handle, alignment)
    })
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: f64, bordered: f64) {
    let h = widgets::decode_js_handle_f64(handle);
    widgets::button::set_bordered(h, bordered != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}
