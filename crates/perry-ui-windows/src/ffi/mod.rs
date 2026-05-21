// FFI exports — these are the functions called from codegen-generated code.
// Split topically across sub-modules; every `#[no_mangle] pub extern "C" fn
// perry_ui_<...>` symbol below is what the linker resolves.

pub mod app_window;
pub mod canvas;
pub mod clipboard_dialog;
pub mod cmd_chart_cal;
pub mod events_anim_nav;
pub mod image_sheet_toolbar_tab;
pub mod js_interop;
pub mod lsp_camera_misc;
pub mod media;
pub mod menu_tray;
pub mod nav_gallery_webview_attrtext;
pub mod rich_pdf_map;
pub mod screen_audio;
pub mod splitview_app_textarea;
pub mod styling;
pub mod system;
pub mod table_tree_combo_picker;
pub mod text_button;
pub mod textfield_scroll;
pub mod widget_create;
pub mod widget_layout_extras;
pub mod widget_tree_state;
