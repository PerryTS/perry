//! Geisterhand `apply_style` dispatcher (issue #185 Phase D step 2).
//!
//! Registered with `perry_geisterhand_register_apply_style` at app
//! init when the `geisterhand` Cargo feature is on. Called from the
//! main thread by the geisterhand pump after the HTTP server queues a
//! `POST /style/:h` request — the HTTP thread cannot call AppKit
//! directly because every NSView mutation must happen on the main
//! thread, so the marshaling goes through the existing pump queue.
//!
//! The `prop_id` namespace is defined in
//! `perry-runtime/src/geisterhand_registry.rs` (constants
//! `STYLE_BACKGROUND_COLOR` ... `STYLE_ENABLED`). Stays in lockstep
//! with the inspector UI's `prop` strings on the wire.

use crate::widgets;

/// Prop-id constants — must stay in sync with
/// `perry-runtime/src/geisterhand_registry.rs::STYLE_*`.
const BACKGROUND_COLOR: u32 = 1;
const COLOR: u32 = 2;
const BORDER_COLOR: u32 = 3;
const BORDER_WIDTH: u32 = 4;
const BORDER_RADIUS: u32 = 5;
const OPACITY: u32 = 6;
const PADDING_UNIFORM: u32 = 7;
const HIDDEN: u32 = 8;
const ENABLED: u32 = 9;

/// Dispatch a queued style edit to the right per-prop setter.
///
/// `args` is interpreted per-prop:
/// - color props use `(a0, a1, a2, a3)` as RGBA in `[0, 1]`
/// - scalar props read `a0` and ignore `a1` / `a2` / `a3`
/// - bool props read `a0 != 0.0`
#[no_mangle]
pub extern "C" fn apply_style(handle: i64, prop_id: u32, a0: f64, a1: f64, a2: f64, a3: f64) {
    match prop_id {
        BACKGROUND_COLOR => widgets::set_background_color(handle, a0, a1, a2, a3),
        COLOR => widgets::text::set_color(handle, a0, a1, a2, a3),
        BORDER_COLOR => widgets::set_border_color(handle, a0, a1, a2, a3),
        BORDER_WIDTH => widgets::set_border_width(handle, a0),
        BORDER_RADIUS => widgets::set_corner_radius(handle, a0),
        OPACITY => widgets::set_opacity(handle, a0),
        // Single-uniform padding maps to all four sides.
        PADDING_UNIFORM => widgets::set_edge_insets(handle, a0, a0, a0, a0),
        HIDDEN => widgets::set_hidden(handle, a0 != 0.0),
        ENABLED => {
            // The exported `perry_ui_widget_set_enabled` expects an `i64`
            // truthy/falsy flag (matching the FFI table); preserve that
            // shape here.
            crate::perry_ui_widget_set_enabled(handle, if a0 != 0.0 { 1 } else { 0 });
        }
        _ => {}
    }
}
