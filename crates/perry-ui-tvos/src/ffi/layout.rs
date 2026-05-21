//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

/// Create a split view container (plain UIView with Auto Layout, not UIStackView).
/// Left panel gets fixed width; right panel fills remaining space.
#[no_mangle]
pub extern "C" fn perry_ui_splitview_create(left_width: f64) -> i64 {
    widgets::splitview::create(left_width)
}

/// Add a child to a split view. First call adds left panel, second adds right panel.
#[no_mangle]
pub extern "C" fn perry_ui_splitview_add_child(
    parent_handle: i64,
    child_handle: i64,
    child_index: f64,
) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::add_child(&parent, &child, child_index as usize);
    }
}

/// Create a vertical layout container (plain UIView, not UIStackView).
#[no_mangle]
pub extern "C" fn perry_ui_vbox_create() -> i64 {
    widgets::splitview::create_vbox()
}

/// Add a child to a vbox at a slot: 0=top, 1=middle(fills), 2=bottom.
#[no_mangle]
pub extern "C" fn perry_ui_vbox_add_child(parent_handle: i64, child_handle: i64, slot: f64) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::vbox_add_child(&parent, &child, slot as usize);
    }
}

/// Finalize vbox layout by connecting middle.bottom to bottom.top.
#[no_mangle]
pub extern "C" fn perry_ui_vbox_finalize(parent_handle: i64) {
    if let Some(parent) = widgets::get_widget(parent_handle) {
        widgets::splitview::vbox_finalize(&parent);
    }
}

/// Create a frame-based horizontal split container.
/// Uses layoutSubviews for child positioning (no Auto Layout on children).
/// This avoids constraint conflicts with embedded UIViews.
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_create(left_width: f64) -> i64 {
    widgets::splitview::create_frame_split(left_width)
}

/// Add a child to a frame-based split container.
/// Children use frame-based layout (translatesAutoresizingMaskIntoConstraints = true).
#[no_mangle]
pub extern "C" fn perry_ui_frame_split_add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (
        widgets::get_widget(parent_handle),
        widgets::get_widget(child_handle),
    ) {
        widgets::splitview::frame_split_add_child(&parent, &child);
    }
}
