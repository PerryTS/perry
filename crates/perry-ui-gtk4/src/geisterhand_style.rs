//! Geisterhand `apply_style` dispatcher for the GTK4 backend (issue
//! #185 Phase D step 2 follow-up — platform sweep). Mirrors
//! `perry-ui-macos/src/geisterhand_style.rs`; routes by `prop_id` to
//! the matching `widgets::set_*` helper. Called from the main thread
//! via the geisterhand pump after the HTTP server queues a
//! `POST /style/:h` request.
//!
//! Prop-id namespace stays in lockstep with
//! `perry-runtime/src/geisterhand_registry.rs::STYLE_*` and the
//! server-side `style_prop_id` map.

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
        PADDING_UNIFORM => widgets::set_edge_insets(handle, a0, a0, a0, a0),
        HIDDEN => widgets::set_hidden(handle, a0 != 0.0),
        ENABLED => crate::perry_ui_widget_set_enabled(handle, if a0 != 0.0 { 1 } else { 0 }),
        _ => {}
    }
}
