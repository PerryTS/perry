//! FFI surface for `perry-ui-ios`.
//!
//! Sub-modules contain `#[no_mangle] pub extern "C" fn perry_ui_*` exports
//! grouped by topic. Originally lived inline in `lib.rs` (~2,900 LOC). Split
//! purely for file-size hygiene — no behavior changes.

pub mod camera;
pub mod comms;
pub mod dialogs_lifecycle;
pub mod misc;
pub mod security_notifications;
pub mod system;
pub mod widgets_advanced;
pub mod widgets_basic;
