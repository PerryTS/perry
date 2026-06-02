//! Auto-split from `crates/perry-ui-tvos/src/lib.rs`. See `ffi/mod.rs`.

#![allow(clippy::missing_safety_doc)]

use crate::*;

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
