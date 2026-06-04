//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_wraps(handle: i64, max_width: f64) {
    widgets::text::set_wraps(handle, max_width);
}

/// Text decoration (issue #185 Phase B). 0=none, 1=underline, 2=strikethrough.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_decoration(handle: i64, decoration: i64) {
    widgets::text::set_decoration(handle, decoration);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

/// Issue #707 — cap visible lines on a Text widget. `lines = 0` is unlimited.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_number_of_lines(handle: i64, lines: i64) {
    widgets::text::set_number_of_lines(handle, lines);
}

/// Issue #707 — 0=word-wrap, 1=head, 2=middle, 3=tail.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_truncation_mode(handle: i64, mode: i64) {
    widgets::text::set_truncation_mode(handle, mode);
}

/// Issue #3621 — horizontal text alignment. `alignment`: 0=left, 1=right,
/// 2=center, 3=justified, 4=natural.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_text_alignment(handle: i64, alignment: i64) {
    widgets::text::set_text_alignment(handle, alignment);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image(handle: i64, name_ptr: i64) {
    widgets::button::set_image(handle, name_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_image_position(handle: i64, position: i64) {
    widgets::button::set_image_position(handle, position);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_content_tint_color(
    handle: i64,
    r: f64,
    g: f64,
    b: f64,
    a: f64,
) {
    widgets::button::set_content_tint_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_height(handle: i64, height: f64) {
    widgets::set_height(handle, height);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_remove_child(parent_handle: i64, child_handle: i64) {
    widgets::remove_child(parent_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_reorder_child(
    parent_handle: i64,
    from_index: f64,
    to_index: f64,
) {
    widgets::reorder_child(parent_handle, from_index as i64, to_index as i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_width(handle: i64) {
    widgets::match_parent_width(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_match_parent_height(handle: i64) {
    widgets::match_parent_height(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_detaches_hidden(handle: i64, flag: i64) {
    widgets::set_detaches_hidden_views(handle, flag != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_distribution(handle: i64, distribution: f64) {
    // UIStackView distribution: 0=Fill, 1=FillEqually, 2=FillProportionally, 3=EqualSpacing, 4=EqualCentering
    if let Some(view) = widgets::get_widget(handle) {
        let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
            use objc2_foundation::NSObjectProtocol;
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let dist = if distribution < 0.0 {
                0_i64
            } else {
                distribution as i64
            };
            unsafe {
                let _: () = objc2::msg_send![&*view, setDistribution: dist];
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_stack_set_alignment(handle: i64, alignment: f64) {
    // UIStackView alignment: 0=Fill, 1=Leading, 2=FirstBaseline, 3=Center, 4=Trailing, 5=LastBaseline
    if let Some(view) = widgets::get_widget(handle) {
        let is_stack = if let Some(cls) = objc2::runtime::AnyClass::get(c"UIStackView") {
            use objc2_foundation::NSObjectProtocol;
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let align = alignment as i64;
            unsafe {
                let _: () = objc2::msg_send![&*view, setAlignment: align];
            }
        }
    }
}
