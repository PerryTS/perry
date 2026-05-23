//! FFI exports for the Android UI surface.
//!
//! Each submodule groups `#[no_mangle] pub extern "C"` symbols by topic
//! (basic widgets, text/scroll/clipboard, menus & dialogs, state bindings,
//! canvas/picker, image/navigation, system APIs, layout, embed/audio/camera,
//! and the issue-#553 follow-ups). Splitting the original `lib.rs` (~2.8k
//! lines) keeps every file under the 2k-line ceiling without changing any
//! exported symbol or signature.

pub mod basic_widgets;
pub mod canvas_picker;
pub mod embed_misc;
pub mod image_nav;
pub mod issue_553;
pub mod menu_dialog;
pub mod state_widgets;
pub mod system_api;
pub mod tabbar_layout;
pub mod text_scroll;

// Re-export every `pub extern "C"` symbol at the `crate::ffi` level (and
// therefore — via `pub use ffi::*;` in lib.rs — at the crate root).
// Without this, code that references `crate::perry_ui_*` (notably
// `app.rs` and `geisterhand_style.rs`) fails to resolve.
pub use basic_widgets::*;
pub use canvas_picker::*;
pub use embed_misc::*;
pub use image_nav::*;
pub use issue_553::*;
pub use menu_dialog::*;
pub use state_widgets::*;
pub use system_api::*;
pub use tabbar_layout::*;
pub use text_scroll::*;
