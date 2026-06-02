// FFI exports — these are the functions called from codegen-generated code.
// Split topically across sub-modules; every `#[no_mangle] pub extern "C" fn
// perry_ui_<...>` symbol below is what the linker resolves.

pub mod app_window;
pub mod canvas;
pub mod clipboard_dialog;
pub mod cmd_chart_cal;
pub mod events_anim_nav;
pub mod image_sheet_toolbar_tab;
// `js_interop` removed: it defined `js_create_callback` / `js_call_function`
// / `js_await_js_promise` / `js_load_module` / `js_new_from_handle` /
// `js_new_instance` / `js_runtime_init` / `js_set_property` / `js_get_export`
// as AOT stubs back when perry-runtime had stripped them. perry-runtime
// re-added them as V8 stubs in `closure/v8_stubs.rs`, so keeping the
// duplicates here produced LNK4006 warnings inside `perry_ui_windows.lib`
// (the V8 stubs from perry-runtime were pulled in via the Rust crate dep,
// AND the local AOT versions both lived in the same archive). The linker
// fell back to `/FORCE` and warned `LNK4088: image may not run` (#2169).
// Worse, the local copies had wrong arities (`js_call_function` had 4 args
// vs. codegen's 5; `js_load_module` had 1 vs. 2; `js_set_property` had 3
// vs. 4; `js_new_instance` had 4 vs. 5), so any callsite resolved against
// the local def would corrupt the stack. The perry-runtime versions match
// the codegen-declared signatures (`crates/perry-codegen/src/runtime_decls/
// stdlib_ffi.rs`) and are the canonical AOT stubs on Windows.
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
