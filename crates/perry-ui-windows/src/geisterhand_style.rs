//! Geisterhand `apply_style` dispatcher for the Win32 backend (issue
//! #185 Phase D step 2 follow-up — platform sweep). Same shape as the
//! macOS / GTK4 dispatchers; routes by `prop_id` to the matching
//! `widgets::set_*` helper. Called on the main thread by the
//! geisterhand pump.
//!
//! Win32 caveat: `set_opacity` / `set_border_color` / `set_border_width`
//! are stub-with-state on Windows today (#210 wired the storage but
//! the apply paths flow through layered-window / WM_PAINT subclassing
//! that's still partial — see `widgets::mod.rs::apply_opacity` /
//! `ensure_border_subclass`). So an inspector edit on Windows will
//! update the per-handle state and trigger the layout/paint pipeline,
//! but the rendered effect mirrors however much the underlying setter
//! has wired up. Border / opacity are visible per #210; shadow is
//! still tracked via #230.

use crate::widgets;

const BACKGROUND_COLOR: u32 = 1;
const COLOR: u32 = 2;
const BORDER_COLOR: u32 = 3;
const BORDER_WIDTH: u32 = 4;
const BORDER_RADIUS: u32 = 5;
const OPACITY: u32 = 6;
const PADDING_UNIFORM: u32 = 7;
const HIDDEN: u32 = 8;
const ENABLED: u32 = 9;

#[no_mangle]
pub extern "C" fn apply_style(handle: i64, prop_id: u32, a0: f64, a1: f64, a2: f64, a3: f64) {
    match prop_id {
        BACKGROUND_COLOR => widgets::set_background_color(handle, a0, a1, a2, a3),
        COLOR => widgets::text::set_color(handle, a0, a1, a2, a3),
        BORDER_COLOR => widgets::set_border_color(handle, a0, a1, a2, a3),
        BORDER_WIDTH => widgets::set_border_width(handle, a0),
        BORDER_RADIUS => widgets::set_corner_radius(handle, a0),
        OPACITY => widgets::set_opacity(handle, a0),
        // Windows uses `set_insets` (vs `set_edge_insets` on macOS / GTK4) —
        // same FFI shape, different helper name internal to the crate.
        PADDING_UNIFORM => widgets::set_insets(handle, a0, a0, a0, a0),
        HIDDEN => crate::perry_ui_set_widget_hidden(handle, if a0 != 0.0 { 1 } else { 0 }),
        ENABLED => crate::perry_ui_widget_set_enabled(handle, if a0 != 0.0 { 1 } else { 0 }),
        _ => {}
    }
}
