//! Geisterhand `apply_style` dispatcher for Android (issue #185 Phase D
//! step 2 follow-up — closes #243). Per-widget setters live on the JVM
//! side; the `crate::perry_ui_widget_*` Rust FFI exports are thin JNI
//! bridges over them. We dispatch through those exports rather than
//! `widgets::*` directly so the existing `catch_panic_void` wrappers
//! still apply.
//!
//! Threading: the geisterhand pump fires from `nativePumpTick` on the
//! UI thread (PerryBridge.startPumpTimer schedules it via Android's
//! `Handler`), so calling JNI bridges from here is already on the right
//! thread — no extra `Handler.post()` marshaling needed.
//!
//! The `prop_id` namespace is defined in
//! `perry-runtime/src/geisterhand_registry.rs` (constants
//! `STYLE_BACKGROUND_COLOR` ... `STYLE_ENABLED`). Stays in lockstep
//! with the inspector UI's `prop` strings on the wire.

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
