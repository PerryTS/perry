// FFI exports — these are the functions called from codegen-generated code.
// Split topically across sub-modules; every `#[no_mangle] pub extern "C" fn
// perry_ui_<...>` / `perry_system_<...>` / `perry_media_<...>` /
// `__wrapper_perry_<...>` symbol below is preserved exactly so the linker
// resolves callsites uniformly.

pub mod app_window;
pub mod bottom_nav_scroll;
pub mod canvas;
pub mod chart_cal_table_tree_combo_picker;
pub mod clipboard_dialog_events;
pub mod image_sheet_toolbar;
pub mod layout;
pub mod media;
pub mod menu_tray;
pub mod nav_rich_pdf_map_palette;
pub mod platform_audio_camera_toast;
pub mod scrollview_styling;
pub mod stubs_webview_attrtext_screenshot;
pub mod system_weather;
pub mod text_button;
pub mod widget_create;
pub mod widget_tree_state;
