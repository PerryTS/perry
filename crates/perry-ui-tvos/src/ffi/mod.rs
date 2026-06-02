//! FFI thunks for perry-ui-tvos.
//!
//! These submodules contain `#[no_mangle] pub extern "C" fn perry_ui_*` /
//! `perry_system_*` / `perry_media_*` / `hone_*` / `perry_background_*` /
//! `perry_get_*` thunks that the Perry codegen emits extern declarations
//! for. Each file is a topical slice of what was previously a single
//! ~2,700-line `lib.rs`; the split is purely organizational — every FFI
//! signature, ABI, and behavior is preserved exactly.
//!
//! Submodules `pub use` nothing — `lib.rs` re-exports them with a glob.

pub mod advanced_widgets;
pub mod app_keychain;
pub mod core_widgets;
pub mod cross_cutting;
pub mod dialogs_screen;
pub mod focus_scroll;
pub mod layout;
pub mod layout_insets;
pub mod media_extras;
pub mod menus_dialog;
pub mod notifs_window;
pub mod reactive_state;
pub mod scrollview_clipboard;
pub mod styling;
pub mod system_apis;
pub mod system_styling;
pub mod timer_canvas;
pub mod websocket_compat;
