//! Geisterhand `apply_style` dispatcher for iOS (issue #185 Phase D
//! step 2 follow-up — platform sweep). UIKit twin of the macOS
//! dispatcher; routes by `prop_id` to the existing `perry_ui_*` FFI
//! exports (which are the stable cross-platform surface — calling them
//! gives us iOS / tvOS / visionOS parity for free since each platform
//! re-exports the same names).
//!
//! Called on the main thread by the geisterhand pump after the HTTP
//! server's `POST /style/:h` queues a `PendingAction::ApplyStyle`.

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
        BACKGROUND_COLOR => crate::perry_ui_widget_set_background_color(handle, a0, a1, a2, a3),
        COLOR => crate::perry_ui_text_set_color(handle, a0, a1, a2, a3),
        BORDER_COLOR => crate::perry_ui_widget_set_border_color(handle, a0, a1, a2, a3),
        BORDER_WIDTH => crate::perry_ui_widget_set_border_width(handle, a0),
        BORDER_RADIUS => crate::perry_ui_widget_set_corner_radius(handle, a0),
        OPACITY => crate::perry_ui_widget_set_opacity(handle, a0),
        PADDING_UNIFORM => crate::perry_ui_widget_set_edge_insets(handle, a0, a0, a0, a0),
        HIDDEN => crate::perry_ui_set_widget_hidden(handle, if a0 != 0.0 { 1 } else { 0 }),
        ENABLED => crate::perry_ui_widget_set_enabled(handle, if a0 != 0.0 { 1 } else { 0 }),
        _ => {}
    }
}
