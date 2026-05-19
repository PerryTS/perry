//! HIR → WebAssembly bytecode emitter
//!
//! Translates HIR modules to WebAssembly binary format using wasm-encoder.
//! All JSValues are represented as i64 using NaN-boxing bit patterns.
//! Arithmetic operations temporarily convert to f64 and back.
//! Runtime operations (strings, console, objects) are imported from a JS bridge.

mod binary;
mod closures;
mod expr;
mod function;
mod js_fallback;
mod locals;
mod memcall;
mod method_call;
mod stmt;
mod string_collection;

use closures::{collect_closures_from_expr, collect_closures_from_stmts};
use locals::{collect_locals, collect_module_let_ids, resolve_source_module_idx};
use stmt::has_return;

use perry_hir::ir::*;
use perry_types::{FuncId, GlobalId, LocalId};
use std::collections::BTreeMap;
use wasm_encoder::{
    CodeSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, GlobalSection, GlobalType, Ieee64, ImportSection, Instruction,
    MemorySection, MemoryType, Module, RefType, TableSection, TableType, TypeSection, ValType,
};

#[derive(Clone)]
pub(super) enum EnumResolvedValue {
    Number(f64),
    String(String),
}

/// Helper: create an F64Const instruction from raw f64 bits
pub(super) fn f64_const(val: f64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(val))
}

/// Helper: create an F64Const instruction from NaN-boxed tag bits (kept for potential future use)
#[allow(dead_code)]
pub(super) fn f64_const_bits(bits: u64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(f64::from_bits(bits)))
}

// NaN-boxing constants (must match perry-runtime and wasm_runtime.js)
pub(super) const STRING_TAG: u64 = 0x7FFF;
pub(super) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(super) const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
pub(super) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(super) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Import function indices (must match the order imports are added)
/// Most fields are unused directly but their indices define the WASM import order.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(super) struct RuntimeImports {
    pub(super) string_new: u32,
    pub(super) console_log: u32,
    pub(super) console_warn: u32,
    pub(super) console_error: u32,
    pub(super) string_concat: u32,
    pub(super) js_add: u32,
    pub(super) string_eq: u32,
    pub(super) string_len: u32,
    pub(super) jsvalue_to_string: u32,
    pub(super) is_truthy: u32,
    pub(super) js_strict_eq: u32,
    pub(super) math_floor: u32,
    pub(super) math_ceil: u32,
    pub(super) math_round: u32,
    pub(super) math_abs: u32,
    pub(super) math_sqrt: u32,
    pub(super) math_pow: u32,
    pub(super) math_random: u32,
    pub(super) math_log: u32,
    pub(super) date_now: u32,
    pub(super) js_typeof: u32,
    pub(super) math_min: u32,
    pub(super) math_max: u32,
    pub(super) parse_int: u32,
    pub(super) parse_float: u32,
    // Phase 0 additions
    pub(super) js_mod: u32,
    pub(super) is_null_or_undefined: u32,
    // Phase 1: Object operations
    pub(super) object_new: u32,
    pub(super) object_set: u32,
    pub(super) object_get: u32,
    pub(super) object_get_dynamic: u32,
    pub(super) object_set_dynamic: u32,
    pub(super) object_delete: u32,
    pub(super) object_delete_dynamic: u32,
    pub(super) object_keys: u32,
    pub(super) object_values: u32,
    pub(super) object_entries: u32,
    pub(super) object_has_property: u32,
    pub(super) object_assign: u32,
    // Phase 1: Array operations
    pub(super) array_new: u32,
    pub(super) array_push: u32,
    pub(super) array_pop: u32,
    pub(super) array_get: u32,
    pub(super) array_set: u32,
    pub(super) array_length: u32,
    pub(super) array_slice: u32,
    pub(super) array_splice: u32,
    pub(super) array_shift: u32,
    pub(super) array_unshift: u32,
    pub(super) array_join: u32,
    pub(super) array_index_of: u32,
    pub(super) array_includes: u32,
    pub(super) array_concat: u32,
    pub(super) array_reverse: u32,
    pub(super) array_flat: u32,
    pub(super) array_is_array: u32,
    pub(super) array_from: u32,
    pub(super) array_push_spread: u32,
    // Phase 1: String methods
    pub(super) string_char_at: u32,
    pub(super) string_substring: u32,
    pub(super) string_index_of: u32,
    pub(super) string_slice: u32,
    pub(super) string_to_lower_case: u32,
    pub(super) string_to_upper_case: u32,
    pub(super) string_trim: u32,
    pub(super) string_includes: u32,
    pub(super) string_starts_with: u32,
    pub(super) string_ends_with: u32,
    pub(super) string_replace: u32,
    pub(super) string_split: u32,
    pub(super) string_from_char_code: u32,
    pub(super) string_pad_start: u32,
    pub(super) string_pad_end: u32,
    pub(super) string_repeat: u32,
    pub(super) string_match: u32,
    pub(super) math_log2: u32,
    pub(super) math_log10: u32,
    // Phase 2: Closure operations
    pub(super) closure_new: u32,
    pub(super) closure_set_capture: u32,
    pub(super) closure_call_0: u32,
    pub(super) closure_call_1: u32,
    pub(super) closure_call_2: u32,
    pub(super) closure_call_3: u32,
    pub(super) closure_call_spread: u32,
    // Phase 2: Array higher-order methods
    pub(super) array_map: u32,
    pub(super) array_filter: u32,
    pub(super) array_for_each: u32,
    pub(super) array_reduce: u32,
    pub(super) array_find: u32,
    pub(super) array_find_index: u32,
    pub(super) array_sort: u32,
    pub(super) array_some: u32,
    pub(super) array_every: u32,
    // Phase 3: Class operations
    pub(super) class_new: u32,
    pub(super) class_set_method: u32,
    pub(super) class_call_method: u32,
    pub(super) class_get_field: u32,
    pub(super) class_set_field: u32,
    pub(super) class_set_static: u32,
    pub(super) class_get_static: u32,
    pub(super) class_instanceof: u32,
    // Phase 4: JSON
    pub(super) json_parse: u32,
    pub(super) json_stringify: u32,
    // Phase 4: Map
    pub(super) map_new: u32,
    pub(super) map_set: u32,
    pub(super) map_get: u32,
    pub(super) map_has: u32,
    pub(super) map_delete: u32,
    pub(super) map_size: u32,
    pub(super) map_clear: u32,
    pub(super) map_entries: u32,
    pub(super) map_keys: u32,
    pub(super) map_values: u32,
    // Phase 4: Set
    pub(super) set_new: u32,
    pub(super) set_new_from_array: u32,
    pub(super) set_add: u32,
    pub(super) set_has: u32,
    pub(super) set_delete: u32,
    pub(super) set_size: u32,
    pub(super) set_clear: u32,
    pub(super) set_values: u32,
    // Phase 4: Date
    pub(super) date_new: u32,
    pub(super) date_get_time: u32,
    pub(super) date_to_iso_string: u32,
    pub(super) date_get_full_year: u32,
    pub(super) date_get_month: u32,
    pub(super) date_get_date: u32,
    pub(super) date_get_day: u32,
    pub(super) date_get_hours: u32,
    pub(super) date_get_minutes: u32,
    pub(super) date_get_seconds: u32,
    pub(super) date_get_milliseconds: u32,
    // Phase 4: Error
    pub(super) error_new: u32,
    pub(super) error_message: u32,
    // Phase 4: RegExp
    pub(super) regexp_new: u32,
    pub(super) regexp_test: u32,
    // Phase 4: Globals
    pub(super) number_coerce: u32,
    pub(super) is_nan: u32,
    pub(super) is_finite: u32,
    // Phase 5: Misc
    pub(super) console_log_multi: u32,
    // Phase 1 addition: Class inheritance
    pub(super) class_set_parent: u32,
    // Phase 3: Try/Catch
    pub(super) try_start: u32,
    pub(super) try_end: u32,
    pub(super) throw_value: u32,
    pub(super) has_exception: u32,
    pub(super) get_exception: u32,
    // Phase 4: URL
    pub(super) url_parse: u32,
    pub(super) url_get_href: u32,
    pub(super) url_get_pathname: u32,
    pub(super) url_get_hostname: u32,
    pub(super) url_get_port: u32,
    pub(super) url_get_search: u32,
    pub(super) url_get_hash: u32,
    pub(super) url_get_origin: u32,
    pub(super) url_get_protocol: u32,
    pub(super) url_get_search_params: u32,
    pub(super) searchparams_get: u32,
    pub(super) searchparams_has: u32,
    pub(super) searchparams_set: u32,
    pub(super) searchparams_append: u32,
    pub(super) searchparams_delete: u32,
    pub(super) searchparams_to_string: u32,
    // Phase 4: Crypto
    pub(super) crypto_random_uuid: u32,
    pub(super) crypto_random_bytes: u32,
    // Phase 4: Path
    pub(super) path_join: u32,
    pub(super) path_dirname: u32,
    pub(super) path_basename: u32,
    pub(super) path_extname: u32,
    pub(super) path_resolve: u32,
    // Phase 4: Process/OS
    pub(super) os_platform: u32,
    pub(super) process_argv: u32,
    pub(super) process_cwd: u32,
    // Phase 6: Buffer
    pub(super) buffer_alloc: u32,
    pub(super) buffer_from_string: u32,
    pub(super) buffer_to_string: u32,
    pub(super) buffer_get: u32,
    pub(super) buffer_set: u32,
    pub(super) buffer_length: u32,
    pub(super) buffer_slice: u32,
    pub(super) buffer_concat: u32,
    pub(super) uint8array_new: u32,
    pub(super) uint8array_from: u32,
    pub(super) uint8array_length: u32,
    pub(super) uint8array_get: u32,
    pub(super) uint8array_set: u32,
    // Timers
    pub(super) set_timeout: u32,
    pub(super) set_interval: u32,
    pub(super) clear_timeout: u32,
    pub(super) clear_interval: u32,
    // Response properties
    pub(super) response_status: u32,
    pub(super) response_ok: u32,
    pub(super) response_headers_get: u32,
    pub(super) response_url: u32,
    // Buffer extras
    pub(super) buffer_copy: u32,
    pub(super) buffer_write: u32,
    pub(super) buffer_equals: u32,
    pub(super) buffer_is_buffer: u32,
    pub(super) buffer_byte_length: u32,
    // Crypto extras
    pub(super) crypto_sha256: u32,
    pub(super) crypto_md5: u32,
    // Path extras
    pub(super) path_is_absolute: u32,
    // Phase 5: Async/Promise/Fetch
    pub(super) fetch_url: u32,
    pub(super) fetch_with_options: u32,
    pub(super) response_json: u32,
    pub(super) response_text: u32,
    pub(super) promise_new: u32,
    pub(super) promise_resolve: u32,
    pub(super) promise_then: u32,
    pub(super) await_promise: u32,
    // Bridge via WASM memory — Firefox canonicalizes NaN in function params,
    // so we write f64 args to memory (preserves NaN bits) and pass only plain numbers.
    pub(super) mem_call: u32, // (func_name_id, arg_count) -> result; args at mem[ARG_BASE..]
    pub(super) mem_call_i32: u32, // (func_name_id, arg_count) -> i32; for is_truthy, string_eq, etc.
}

/// Map perry/ui and perry/system method names to bridge function names.
/// Mirrors the mapping in perry-codegen-js's emit_ui_method_call.
pub(super) fn map_ui_method(method: &str, class_name: Option<&str>) -> &'static str {
    match method {
        // Widget creation
        "App" | "app_create" => "perry_ui_app_create",
        "VStack" | "vstack_create" => "perry_ui_vstack_create",
        "HStack" | "hstack_create" => "perry_ui_hstack_create",
        "ZStack" | "zstack_create" => "perry_ui_zstack_create",
        "Text" | "text_create" => "perry_ui_text_create",
        "Button" | "button_create" => "perry_ui_button_create",
        "TextField" | "textfield_create" => "perry_ui_textfield_create",
        "SecureField" | "securefield_create" => "perry_ui_securefield_create",
        "Toggle" | "toggle_create" => "perry_ui_toggle_create",
        "Slider" | "slider_create" => "perry_ui_slider_create",
        "ScrollView" | "scrollview_create" => "perry_ui_scrollview_create",
        "Spacer" | "spacer_create" => "perry_ui_spacer_create",
        "Divider" | "divider_create" => "perry_ui_divider_create",
        "ProgressView" | "progressview_create" => "perry_ui_progressview_create",
        "Image" | "image_create" => "perry_ui_image_create",
        "Picker" | "picker_create" => "perry_ui_picker_create",
        "Form" | "form_create" => "perry_ui_form_create",
        "Section" | "section_create" => "perry_ui_section_create",
        "NavigationStack" | "navigationstack_create" => "perry_ui_navigationstack_create",
        "Canvas" | "canvas_create" => "perry_ui_canvas_create",
        "Table" | "table_create" => "perry_ui_table_create",
        "LazyVStack" | "lazyvstack_create" => "perry_ui_lazyvstack_create",
        "TextArea" | "textarea_create" => "perry_ui_textarea_create",
        "VStackWithInsets" => "perry_ui_vstack_create_with_insets",
        "HStackWithInsets" => "perry_ui_hstack_create_with_insets",
        // Child management
        "addChild" | "widget_add_child" => "perry_ui_widget_add_child",
        "removeAllChildren" | "widget_remove_all_children" => "perry_ui_widget_remove_all_children",
        "widgetAddChild" => "perry_ui_widget_add_child",
        "widgetRemoveChild" => "perry_ui_widget_remove_child",
        "widgetReorderChild" => "perry_ui_widget_reorder_child",
        "widgetClearChildren" => "perry_ui_widget_remove_all_children",
        "widgetAddOverlay" => "perry_ui_widget_add_overlay",
        "widgetSetOverlayFrame" => "perry_ui_widget_set_overlay_frame",
        // Styling
        "setBackground" | "set_background" | "widgetSetBackgroundColor" => {
            "perry_ui_set_background"
        }
        "setForeground" | "set_foreground" | "textSetColor" => "perry_ui_set_foreground",
        "setFontSize" | "set_font_size" | "textSetFontSize" => "perry_ui_set_font_size",
        "setFontWeight" | "set_font_weight" | "textSetFontWeight" => "perry_ui_set_font_weight",
        "setFontFamily" | "set_font_family" | "textSetFontFamily" => "perry_ui_set_font_family",
        "setPadding" | "set_padding" => "perry_ui_set_padding",
        "setFrame" | "set_frame" => "perry_ui_set_frame",
        "setCornerRadius" | "set_corner_radius" => "perry_ui_set_corner_radius",
        "setBorder" | "set_border" => "perry_ui_set_border",
        // Apple-style split setters (issue #185 Phase B closure). Map
        // both to the new joint-state JS functions in wasm_runtime.js
        // that cache (color, width) and re-emit `el.style.border`.
        "widgetSetBorderColor" => "perry_ui_widget_set_border_color",
        "widgetSetBorderWidth" => "perry_ui_widget_set_border_width",
        // Issue #185 Phase B closure 11 — Web aliases that bring the
        // matrix Web column to full wired-on-non-Stub parity. Each
        // routes to an existing JS function (most simply reusing the
        // generic widget setters that already do the same DOM work).
        "widgetSetBackgroundGradient" => "perry_ui_widget_set_background_gradient",
        "textSetSelectable" => "perry_ui_text_set_selectable",
        // textfield-specific setters reuse the generic ones — DOM
        // <input> takes the same `el.style.*` props as a generic
        // element.
        "textfieldSetBackgroundColor" => "perry_ui_set_background",
        "textfieldSetTextColor" => "perry_ui_set_foreground",
        "textfieldSetFontSize" => "perry_ui_set_font_size",
        "textfieldSetBorderless" => "perry_ui_textfield_set_borderless",
        "stackSetAlignment" => "perry_ui_stack_set_alignment",
        // Text decoration (issue #185 Phase B). 0=none, 1=underline,
        // 2=strikethrough on the canonical FFI; CSS-side translates.
        "textSetDecoration" | "text_set_decoration" => "perry_ui_text_set_decoration",
        // Drop shadow (issue #185 Phase B closure 2). Mirrors the
        // canonical Apple-side name so the matrix has a single FFI
        // symbol per row. JS runtime maps to `el.style.boxShadow`.
        "widgetSetShadow" | "set_shadow" => "perry_ui_widget_set_shadow",
        "setOpacity" | "set_opacity" | "widgetSetOpacity" => "perry_ui_set_opacity",
        "setEnabled" | "set_enabled" => "perry_ui_set_enabled",
        "setTooltip" | "set_tooltip" => "perry_ui_set_tooltip",
        "setControlSize" | "set_control_size" => "perry_ui_set_control_size",
        "widgetSetWidth" => "perry_ui_widget_set_width",
        "widgetSetHeight" => "perry_ui_widget_set_height",
        "widgetSetHugging" => "perry_ui_widget_set_hugging",
        "widgetSetHidden" => "perry_ui_set_widget_hidden",
        "widgetMatchParentWidth" => "perry_ui_widget_match_parent_width",
        "widgetMatchParentHeight" => "perry_ui_widget_match_parent_height",
        "widgetSetEdgeInsets" => "perry_ui_widget_set_edge_insets",
        "stackSetDetachesHidden" => "perry_ui_stack_set_detaches_hidden",
        "stackSetDistribution" => "perry_ui_stack_set_distribution",
        // Animations
        "animateOpacity" | "animate_opacity" => "perry_ui_animate_opacity",
        "animatePosition" | "animate_position" => "perry_ui_animate_position",
        // widget-prefixed free-function forms (used by HIR reactive desugar)
        "widgetAnimateOpacity" => "perry_ui_animate_opacity",
        "widgetAnimatePosition" => "perry_ui_animate_position",
        // Events
        "setOnClick" | "set_on_click" => "perry_ui_set_on_click",
        "setOnHover" | "set_on_hover" => "perry_ui_set_on_hover",
        "setOnDoubleClick" | "set_on_double_click" => "perry_ui_set_on_double_click",
        // State
        "State" | "create" | "createState" | "state_create" => "perry_ui_state_create",
        "get" if class_name == Some("State") => "perry_ui_state_get",
        "set" if class_name == Some("State") => "perry_ui_state_set",
        "value" => "perry_ui_state_get",
        "onChange" | "state_on_change" | "stateOnChange" => "perry_ui_state_on_change",
        // State bindings
        "bindText" | "state_bind_text" => "perry_ui_state_bind_text",
        "bindTextNumeric" | "state_bind_text_numeric" => "perry_ui_state_bind_text_numeric",
        "bindSlider" | "state_bind_slider" => "perry_ui_state_bind_slider",
        "bindToggle" | "state_bind_toggle" => "perry_ui_state_bind_toggle",
        "bindVisibility" | "state_bind_visibility" => "perry_ui_state_bind_visibility",
        "bindForEach" | "state_bind_foreach" => "perry_ui_state_bind_foreach",
        // Text/Button/TextField ops
        "textSetString" => "perry_ui_text_set_string",
        "textSetWraps" => "perry_ui_text_set_wraps",
        "buttonSetBordered" => "perry_ui_button_set_bordered",
        "buttonSetTitle" => "perry_ui_button_set_title",
        "buttonSetTextColor" => "perry_ui_button_set_text_color",
        "buttonSetImage" => "perry_ui_button_set_image",
        "buttonSetContentTintColor" => "perry_ui_button_set_content_tint_color",
        "buttonSetImagePosition" => "perry_ui_button_set_image_position",
        "textfieldFocus" => "perry_ui_textfield_focus",
        "textfieldSetString" => "perry_ui_textfield_set_string",
        "textfieldGetString" => "perry_ui_textfield_get_string",
        "textfieldBlurAll" => "perry_ui_textfield_blur_all",
        "textfieldSetOnSubmit" => "perry_ui_textfield_set_on_submit",
        "textfieldSetOnFocus" => "perry_ui_textfield_set_on_focus",
        // ScrollView (accept both camelCase forms)
        "scrollViewSetChild" | "scrollviewSetChild" => "perry_ui_scrollview_set_child",
        "scrollViewScrollTo" | "scrollviewScrollTo" => "perry_ui_scrollview_scroll_to",
        "scrollViewGetOffset" | "scrollviewGetOffset" => "perry_ui_scrollview_get_offset",
        "scrollViewSetOffset" | "scrollviewSetOffset" => "perry_ui_scrollview_set_offset",
        // Canvas
        "fillRect" | "canvas_fill_rect" => "perry_ui_canvas_fill_rect",
        "strokeRect" | "canvas_stroke_rect" => "perry_ui_canvas_stroke_rect",
        "clearRect" | "canvas_clear_rect" => "perry_ui_canvas_clear_rect",
        "setFillColor" | "canvas_set_fill_color" => "perry_ui_canvas_set_fill_color",
        "setStrokeColor" | "canvas_set_stroke_color" => "perry_ui_canvas_set_stroke_color",
        "beginPath" | "canvas_begin_path" => "perry_ui_canvas_begin_path",
        "moveTo" | "canvas_move_to" => "perry_ui_canvas_move_to",
        "lineTo" | "canvas_line_to" => "perry_ui_canvas_line_to",
        "arc" | "canvas_arc" => "perry_ui_canvas_arc",
        "closePath" | "canvas_close_path" => "perry_ui_canvas_close_path",
        "fill" | "canvas_fill" => "perry_ui_canvas_fill",
        "stroke" | "canvas_stroke" => "perry_ui_canvas_stroke",
        "setLineWidth" | "canvas_set_line_width" => "perry_ui_canvas_set_line_width",
        "fillText" | "canvas_fill_text" => "perry_ui_canvas_fill_text",
        "setFont" | "canvas_set_font" => "perry_ui_canvas_set_font",
        // Navigation
        "navstackPush" => "perry_ui_navstack_push",
        "navstackPop" => "perry_ui_navstack_pop",
        // Picker
        "pickerAddItem" => "perry_ui_picker_add_item",
        "pickerSetSelected" => "perry_ui_picker_set_selected",
        "pickerGetSelected" => "perry_ui_picker_get_selected",
        // Camera (issue #191) — Web has no live-camera FFI yet, so the
        // wasm_runtime.js stubs return 0 / -1. The dispatch entries here
        // exist so user code calling `CameraView()` from a browser build
        // resolves rather than throwing "perry_ui_unknown".
        "CameraView" | "camera_create" => "perry_ui_camera_create",
        "cameraStart" => "perry_ui_camera_start",
        "cameraStop" => "perry_ui_camera_stop",
        "cameraFreeze" => "perry_ui_camera_freeze",
        "cameraUnfreeze" => "perry_ui_camera_unfreeze",
        "cameraSampleColor" => "perry_ui_camera_sample_color",
        "cameraSetOnTap" => "perry_ui_camera_set_on_tap",
        // Image
        "imageSetSize" => "perry_ui_image_set_size",
        "imageSetTint" => "perry_ui_image_set_tint",
        // Menu
        "menuCreate" | "menu_create" => "perry_ui_menu_create",
        "menuAddItem" | "menu_add_item" => "perry_ui_menu_add_item",
        "menuAddSeparator" | "menu_add_separator" => "perry_ui_menu_add_separator",
        "menuAddSubmenu" | "menu_add_submenu" => "perry_ui_menu_add_submenu",
        "menuBarCreate" | "menubar_create" => "perry_ui_menubar_create",
        "menuBarAddMenu" | "menubar_add_menu" => "perry_ui_menubar_add_menu",
        "menuBarAttach" | "menubar_attach" => "perry_ui_menubar_attach",
        "widgetSetContextMenu" => "perry_ui_widget_set_context_menu",
        // Dialog
        "openFileDialog" => "perry_ui_open_file_dialog",
        "openFolderDialog" => "perry_ui_open_folder_dialog",
        "saveFileDialog" => "perry_ui_save_file_dialog",
        "alert" => "perry_ui_alert",
        // Clipboard
        "clipboardRead" => "perry_ui_clipboard_read",
        "clipboardWrite" => "perry_ui_clipboard_write",
        // Keyboard
        "addKeyboardShortcut" => "perry_ui_add_keyboard_shortcut",
        // Sheet
        "sheetCreate" => "perry_ui_sheet_create",
        "sheetPresent" => "perry_ui_sheet_present",
        "sheetDismiss" => "perry_ui_sheet_dismiss",
        // Toolbar
        "toolbarCreate" => "perry_ui_toolbar_create",
        "toolbarAddItem" => "perry_ui_toolbar_add_item",
        "toolbarAttach" => "perry_ui_toolbar_attach",
        // Window
        "windowCreate" => "perry_ui_window_create",
        "windowSetBody" => "perry_ui_window_set_body",
        "windowShow" => "perry_ui_window_show",
        "windowClose" => "perry_ui_window_close",
        // App lifecycle
        "run" | "app_run" => "perry_ui_app_run",
        "appSetBody" => "perry_ui_app_set_body",
        "appSetMinSize" => "perry_ui_app_set_min_size",
        "appSetMaxSize" => "perry_ui_app_set_max_size",
        "appOnActivate" => "perry_ui_app_on_activate",
        "appOnTerminate" => "perry_ui_app_on_terminate",
        "appSetTimer" => "perry_ui_app_set_timer",
        // Table
        "tableSetColumnHeader" => "perry_ui_table_set_column_header",
        "tableSetColumnWidth" => "perry_ui_table_set_column_width",
        "tableUpdateRowCount" => "perry_ui_table_update_row_count",
        "tableSetOnRowSelect" => "perry_ui_table_set_on_row_select",
        "tableGetSelectedRow" => "perry_ui_table_get_selected_row",
        // System (perry/system module)
        "openURL" | "open_url" => "perry_system_open_url",
        "isDarkMode" | "is_dark_mode" => "perry_system_is_dark_mode",
        "preferencesGet" | "preferences_get" => "perry_system_preferences_get",
        "preferencesSet" | "preferences_set" => "perry_system_preferences_set",
        "keychainSave" | "keychain_save" => "perry_system_keychain_save",
        "keychainGet" | "keychain_get" => "perry_system_keychain_get",
        "keychainDelete" | "keychain_delete" => "perry_system_keychain_delete",
        "notificationSend" | "notification_send" => "perry_system_notification_send",
        // Default — try the centralised perry-dispatch tables first
        // (Tier 1.3, v0.5.332). New perry/ui or perry/system methods
        // added to PERRY_UI_TABLE / PERRY_SYSTEM_TABLE resolve on
        // `--target wasm` without a parallel edit here. The static arms
        // above stay for legacy snake_case aliases that aren't in those
        // tables.
        _ => perry_dispatch::ui_method_to_runtime(method).unwrap_or("perry_ui_unknown"),
    }
}

/// Output from WASM compilation: binary + extra JS for async functions.
pub struct WasmCompileOutput {
    pub wasm_bytes: Vec<u8>,
    pub async_js: String,
    /// FFI function names that must be provided as imports under the "ffi" namespace.
    pub ffi_imports: Vec<String>,
}

/// Compile HIR modules to a WebAssembly binary.
pub fn compile_to_wasm(modules: &[(String, perry_hir::ir::Module)]) -> Vec<u8> {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules).wasm_bytes
}

/// Compile HIR modules to WASM binary + generated JS for async functions.
pub fn compile_to_wasm_with_async(
    modules: &[(String, perry_hir::ir::Module)],
) -> WasmCompileOutput {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules)
}

pub(super) struct WasmModuleEmitter {
    /// String literal table: content → (string_id, offset, length)
    pub(super) string_table: Vec<(String, u32, u32)>, // (content, offset, len)
    pub(super) string_map: BTreeMap<String, u32>, // content → string_id
    pub(super) string_data: Vec<u8>,              // packed string bytes
    /// Type section entries: (params, results)
    pub(super) types: Vec<(Vec<ValType>, Vec<ValType>)>,
    pub(super) type_map: BTreeMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// Function index mapping: FuncId → wasm function index
    pub(super) func_map: BTreeMap<FuncId, u32>,
    /// Reverse table map: wasm function index → table index
    pub(super) func_to_table_idx: BTreeMap<u32, u32>,
    /// Import count (import functions come first in the index space)
    pub(super) num_imports: u32,
    /// Runtime import indices
    pub(super) rt: Option<RuntimeImports>,
    /// Global variable mapping: GlobalId → wasm global index
    pub(super) global_map: BTreeMap<GlobalId, u32>,
    pub(super) num_globals: u32,
    /// Module-level Let bindings promoted to WASM globals: (mod_idx, LocalId) → wasm global idx.
    /// Module-level `let`/`const` declarations live in module.init as Stmt::Let, but
    /// are accessed by functions in the same module via LocalGet. They need to be
    /// stored in WASM globals so cross-function references work, and so module-init
    /// LocalIds don't collide with other modules' identical LocalIds.
    pub(super) module_let_globals: BTreeMap<(usize, LocalId), u32>,
    /// Current module index when compiling functions/methods, so LocalGet can resolve
    /// module-level Lets to the correct WASM global.
    pub(super) current_mod_idx: usize,
    /// Class constructor map: class_name → wasm function index
    pub(super) class_ctor_map: BTreeMap<String, u32>,
    /// Class method map: class_name → {method_name → wasm function index}
    pub(super) class_method_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Class static method map: class_name → {method_name → wasm function index}
    pub(super) class_static_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Function name → wasm function index (for cross-module ExternFuncRef resolution)
    pub(super) func_name_map: BTreeMap<String, u32>,
    /// FFI imports: (name, param_count, has_return) — registered as WASM imports under "ffi" namespace
    pub(super) ffi_imports: Vec<(String, usize, bool)>,
    /// Class parent map: child_class_name → parent_class_name
    pub(super) class_parent_map: BTreeMap<String, String>,
    /// Enum member values: (enum_name, member_name) → numeric value or string
    pub(super) enum_values: BTreeMap<(String, String), EnumResolvedValue>,
    /// Global index for NaN-safe temp storage (global.set/get may preserve NaN in Firefox)
    pub(super) nan_temp_global: u32,
    /// Async function names (compiled to JS, not WASM)
    pub(super) async_func_imports: Vec<(String, u32, usize)>, // (name, import_idx, param_count)
    /// Generated JS code for async functions
    pub(super) async_js_code: Vec<String>,
    /// Per-module func_map snapshots: FuncRef(id) is only unique within a module,
    /// so each module needs its own FuncId→wasm_idx mapping.
    pub(super) module_func_maps: Vec<BTreeMap<FuncId, u32>>,
    /// Set of WASM function indices that return void (no return value).
    /// Used to push TAG_UNDEFINED after calling void functions via FuncRef.
    pub(super) void_funcs: std::collections::BTreeSet<u32>,
    /// WASM function index → expected parameter count.
    /// Used to pad missing arguments with TAG_UNDEFINED for optional params.
    pub(super) func_param_counts: BTreeMap<u32, usize>,
    /// Issue #1071: cross-module imported VARIABLE resolution.
    /// Maps `(consumer_mod_idx, imported_local_name)` → WASM global index
    /// of the source module's `Stmt::Let` that backs the export. Pre-fix
    /// `Expr::ExternFuncRef { name }` for an imported `const`/`let` (rather
    /// than a function) fell through to a `TAG_UNDEFINED` constant because
    /// `func_name_map` only carries function names. With this map populated
    /// from each consumer's `Import` × source's `Export::Named` × source's
    /// module-let globals, the ExternFuncRef value path resolves to a
    /// `GlobalGet(gidx)` reading the live module-let slot, matching the
    /// LLVM target's `perry_fn_<src>__<name>()` getter path.
    pub(super) imported_var_globals: BTreeMap<(usize, String), u32>,
}

impl WasmModuleEmitter {
    fn new() -> Self {
        Self {
            string_table: Vec::new(),
            string_map: BTreeMap::new(),
            string_data: Vec::new(),
            types: Vec::new(),
            type_map: BTreeMap::new(),
            func_map: BTreeMap::new(),
            func_to_table_idx: BTreeMap::new(),
            num_imports: 0,
            rt: None,
            global_map: BTreeMap::new(),
            num_globals: 0,
            module_let_globals: BTreeMap::new(),
            current_mod_idx: 0,
            class_ctor_map: BTreeMap::new(),
            class_method_map: BTreeMap::new(),
            class_static_map: BTreeMap::new(),
            func_name_map: BTreeMap::new(),
            ffi_imports: Vec::new(),
            class_parent_map: BTreeMap::new(),
            enum_values: BTreeMap::new(),
            nan_temp_global: 0, // set during compile()
            async_func_imports: Vec::new(),
            module_func_maps: Vec::new(),
            void_funcs: std::collections::BTreeSet::new(),
            func_param_counts: BTreeMap::new(),
            async_js_code: Vec::new(),
            imported_var_globals: BTreeMap::new(),
        }
    }

    /// Intern a string literal, returning its string_id.
    fn intern_string(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.string_map.get(s) {
            return id;
        }
        let id = self.string_table.len() as u32;
        let offset = self.string_data.len() as u32;
        let bytes = s.as_bytes();
        let len = bytes.len() as u32;
        self.string_data.extend_from_slice(bytes);
        self.string_table.push((s.to_string(), offset, len));
        self.string_map.insert(s.to_string(), id);
        id
    }

    /// Get or create a function type index for the given signature.
    fn get_type_idx(&mut self, params: Vec<ValType>, results: Vec<ValType>) -> u32 {
        let key = (params.clone(), results.clone());
        if let Some(&idx) = self.type_map.get(&key) {
            return idx;
        }
        let idx = self.types.len() as u32;
        self.types.push((params, results));
        self.type_map.insert(key, idx);
        idx
    }

    fn compile(&mut self, modules: &[(String, perry_hir::ir::Module)]) -> WasmCompileOutput {
        // First pass: collect all string literals
        for (_, module) in modules {
            self.collect_strings(module);
        }

        // Register runtime import types and get type indices
        // All imports use f64 for JSValues
        let t_void = self.get_type_idx(vec![], vec![]);
        let t_i32_i32_void = self.get_type_idx(vec![ValType::I32, ValType::I32], vec![]);
        let t_f64_void = self.get_type_idx(vec![ValType::I64], vec![]);
        let t_f64_f64_f64 = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![ValType::I64]);
        let t_f64_f64_i32 = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![ValType::I32]);
        let t_f64_f64 = self.get_type_idx(vec![ValType::I64], vec![ValType::I64]);
        let t_f64_i32 = self.get_type_idx(vec![ValType::I64], vec![ValType::I32]);
        let t_void_f64 = self.get_type_idx(vec![], vec![ValType::I64]);

        // Add runtime imports (order matters — defines function indices)
        let mut import_idx: u32 = 0;
        let mut next_import = || {
            let i = import_idx;
            import_idx += 1;
            i
        };

        // Additional type signatures needed for Phase 1+
        let t_f64_f64_void = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_void =
            self.get_type_idx(vec![ValType::I64, ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_f64 = self.get_type_idx(
            vec![ValType::I64, ValType::I64, ValType::I64],
            vec![ValType::I64],
        );
        let t_f64_f64_f64_f64_f64 = self.get_type_idx(
            vec![ValType::I64, ValType::I64, ValType::I64, ValType::I64],
            vec![ValType::I64],
        );

        let rt = RuntimeImports {
            string_new: next_import(),
            console_log: next_import(),
            console_warn: next_import(),
            console_error: next_import(),
            string_concat: next_import(),
            js_add: next_import(),
            string_eq: next_import(),
            string_len: next_import(),
            jsvalue_to_string: next_import(),
            is_truthy: next_import(),
            js_strict_eq: next_import(),
            math_floor: next_import(),
            math_ceil: next_import(),
            math_round: next_import(),
            math_abs: next_import(),
            math_sqrt: next_import(),
            math_pow: next_import(),
            math_random: next_import(),
            math_log: next_import(),
            date_now: next_import(),
            js_typeof: next_import(),
            math_min: next_import(),
            math_max: next_import(),
            parse_int: next_import(),
            parse_float: next_import(),
            // Phase 0
            js_mod: next_import(),
            is_null_or_undefined: next_import(),
            // Phase 1: Objects
            object_new: next_import(),
            object_set: next_import(),
            object_get: next_import(),
            object_get_dynamic: next_import(),
            object_set_dynamic: next_import(),
            object_delete: next_import(),
            object_delete_dynamic: next_import(),
            object_keys: next_import(),
            object_values: next_import(),
            object_entries: next_import(),
            object_has_property: next_import(),
            object_assign: next_import(),
            // Phase 1: Arrays
            array_new: next_import(),
            array_push: next_import(),
            array_pop: next_import(),
            array_get: next_import(),
            array_set: next_import(),
            array_length: next_import(),
            array_slice: next_import(),
            array_splice: next_import(),
            array_shift: next_import(),
            array_unshift: next_import(),
            array_join: next_import(),
            array_index_of: next_import(),
            array_includes: next_import(),
            array_concat: next_import(),
            array_reverse: next_import(),
            array_flat: next_import(),
            array_is_array: next_import(),
            array_from: next_import(),
            array_push_spread: next_import(),
            // Phase 1: Strings
            string_char_at: next_import(),
            string_substring: next_import(),
            string_index_of: next_import(),
            string_slice: next_import(),
            string_to_lower_case: next_import(),
            string_to_upper_case: next_import(),
            string_trim: next_import(),
            string_includes: next_import(),
            string_starts_with: next_import(),
            string_ends_with: next_import(),
            string_replace: next_import(),
            string_split: next_import(),
            string_from_char_code: next_import(),
            string_pad_start: next_import(),
            string_pad_end: next_import(),
            string_repeat: next_import(),
            string_match: next_import(),
            math_log2: next_import(),
            math_log10: next_import(),
            // Phase 2: Closures
            closure_new: next_import(),
            closure_set_capture: next_import(),
            closure_call_0: next_import(),
            closure_call_1: next_import(),
            closure_call_2: next_import(),
            closure_call_3: next_import(),
            closure_call_spread: next_import(),
            // Phase 2: Array higher-order
            array_map: next_import(),
            array_filter: next_import(),
            array_for_each: next_import(),
            array_reduce: next_import(),
            array_find: next_import(),
            array_find_index: next_import(),
            array_sort: next_import(),
            array_some: next_import(),
            array_every: next_import(),
            // Phase 3: Classes
            class_new: next_import(),
            class_set_method: next_import(),
            class_call_method: next_import(),
            class_get_field: next_import(),
            class_set_field: next_import(),
            class_set_static: next_import(),
            class_get_static: next_import(),
            class_instanceof: next_import(),
            // Phase 4: JSON
            json_parse: next_import(),
            json_stringify: next_import(),
            // Phase 4: Map
            map_new: next_import(),
            map_set: next_import(),
            map_get: next_import(),
            map_has: next_import(),
            map_delete: next_import(),
            map_size: next_import(),
            map_clear: next_import(),
            map_entries: next_import(),
            map_keys: next_import(),
            map_values: next_import(),
            // Phase 4: Set
            set_new: next_import(),
            set_new_from_array: next_import(),
            set_add: next_import(),
            set_has: next_import(),
            set_delete: next_import(),
            set_size: next_import(),
            set_clear: next_import(),
            set_values: next_import(),
            // Phase 4: Date
            date_new: next_import(),
            date_get_time: next_import(),
            date_to_iso_string: next_import(),
            date_get_full_year: next_import(),
            date_get_month: next_import(),
            date_get_date: next_import(),
            date_get_day: next_import(),
            date_get_hours: next_import(),
            date_get_minutes: next_import(),
            date_get_seconds: next_import(),
            date_get_milliseconds: next_import(),
            // Phase 4: Error
            error_new: next_import(),
            error_message: next_import(),
            // Phase 4: RegExp
            regexp_new: next_import(),
            regexp_test: next_import(),
            // Phase 4: Globals
            number_coerce: next_import(),
            is_nan: next_import(),
            is_finite: next_import(),
            // Phase 5: Misc
            console_log_multi: next_import(),
            // Phase 1 addition: Class inheritance
            class_set_parent: next_import(),
            // Phase 3: Try/Catch
            try_start: next_import(),
            try_end: next_import(),
            throw_value: next_import(),
            has_exception: next_import(),
            get_exception: next_import(),
            // Phase 4: URL
            url_parse: next_import(),
            url_get_href: next_import(),
            url_get_pathname: next_import(),
            url_get_hostname: next_import(),
            url_get_port: next_import(),
            url_get_search: next_import(),
            url_get_hash: next_import(),
            url_get_origin: next_import(),
            url_get_protocol: next_import(),
            url_get_search_params: next_import(),
            searchparams_get: next_import(),
            searchparams_has: next_import(),
            searchparams_set: next_import(),
            searchparams_append: next_import(),
            searchparams_delete: next_import(),
            searchparams_to_string: next_import(),
            // Phase 4: Crypto
            crypto_random_uuid: next_import(),
            crypto_random_bytes: next_import(),
            // Phase 4: Path
            path_join: next_import(),
            path_dirname: next_import(),
            path_basename: next_import(),
            path_extname: next_import(),
            path_resolve: next_import(),
            // Phase 4: Process/OS
            os_platform: next_import(),
            process_argv: next_import(),
            process_cwd: next_import(),
            // Phase 6: Buffer
            buffer_alloc: next_import(),
            buffer_from_string: next_import(),
            buffer_to_string: next_import(),
            buffer_get: next_import(),
            buffer_set: next_import(),
            buffer_length: next_import(),
            buffer_slice: next_import(),
            buffer_concat: next_import(),
            uint8array_new: next_import(),
            uint8array_from: next_import(),
            uint8array_length: next_import(),
            uint8array_get: next_import(),
            uint8array_set: next_import(),
            // Timers
            set_timeout: next_import(),
            set_interval: next_import(),
            clear_timeout: next_import(),
            clear_interval: next_import(),
            // Response properties
            response_status: next_import(),
            response_ok: next_import(),
            response_headers_get: next_import(),
            response_url: next_import(),
            // Buffer extras
            buffer_copy: next_import(),
            buffer_write: next_import(),
            buffer_equals: next_import(),
            buffer_is_buffer: next_import(),
            buffer_byte_length: next_import(),
            // Crypto extras
            crypto_sha256: next_import(),
            crypto_md5: next_import(),
            // Path extras
            path_is_absolute: next_import(),
            // Phase 5: Async/Promise/Fetch
            fetch_url: next_import(),
            fetch_with_options: next_import(),
            response_json: next_import(),
            response_text: next_import(),
            promise_new: next_import(),
            promise_resolve: next_import(),
            promise_then: next_import(),
            await_promise: next_import(),
            // Memory-based bridge (Firefox NaN canonicalization workaround)
            mem_call: next_import(),
            mem_call_i32: next_import(),
        };
        self.num_imports = import_idx;
        self.rt = Some(rt);

        // Additional types for new phases
        let t_void_i32 = self.get_type_idx(vec![], vec![ValType::I32]);

        // Build import tables dynamically from struct fields
        // Each entry: (name, type_idx)
        let import_entries: Vec<(&str, u32)> = vec![
            ("string_new", t_i32_i32_void),
            ("console_log", t_f64_void),
            ("console_warn", t_f64_void),
            ("console_error", t_f64_void),
            ("string_concat", t_f64_f64_f64),
            ("js_add", t_f64_f64_f64),
            ("string_eq", t_f64_f64_i32),
            ("string_len", t_f64_f64),
            ("jsvalue_to_string", t_f64_f64),
            ("is_truthy", t_f64_i32),
            ("js_strict_eq", t_f64_f64_i32),
            ("math_floor", t_f64_f64),
            ("math_ceil", t_f64_f64),
            ("math_round", t_f64_f64),
            ("math_abs", t_f64_f64),
            ("math_sqrt", t_f64_f64),
            ("math_pow", t_f64_f64_f64),
            ("math_random", t_void_f64),
            ("math_log", t_f64_f64),
            ("date_now", t_void_f64),
            ("js_typeof", t_f64_f64),
            ("math_min", t_f64_f64_f64),
            ("math_max", t_f64_f64_f64),
            ("parse_int", t_f64_f64),
            ("parse_float", t_f64_f64),
            // Phase 0
            ("js_mod", t_f64_f64_f64),
            ("is_null_or_undefined", t_f64_i32),
            // Phase 1: Objects (f64 handles)
            ("object_new", t_void_f64),                 // () -> handle
            ("object_set", t_f64_f64_f64_f64), // (handle, key_str, value) -> handle (chaining)
            ("object_get", t_f64_f64_f64),     // (handle, key_str) -> value
            ("object_get_dynamic", t_f64_f64_f64), // (handle, key) -> value
            ("object_set_dynamic", t_f64_f64_f64_void), // (handle, key, value) -> void
            ("object_delete", t_f64_f64_void), // (handle, key_str) -> void
            ("object_delete_dynamic", t_f64_f64_void), // (handle, key) -> void
            ("object_keys", t_f64_f64),        // (handle) -> array_handle
            ("object_values", t_f64_f64),      // (handle) -> array_handle
            ("object_entries", t_f64_f64),     // (handle) -> array_handle
            ("object_has_property", t_f64_f64_i32), // (handle, key) -> i32
            ("object_assign", t_f64_f64_f64),  // (target, source) -> target
            // Phase 1: Arrays
            ("array_new", t_void_f64),            // () -> handle
            ("array_push", t_f64_f64_f64),        // (handle, value) -> handle (chaining)
            ("array_pop", t_f64_f64),             // (handle) -> value
            ("array_get", t_f64_f64_f64),         // (handle, index) -> value
            ("array_set", t_f64_f64_f64_void),    // (handle, index, value) -> void
            ("array_length", t_f64_f64),          // (handle) -> length
            ("array_slice", t_f64_f64_f64_f64),   // (handle, start, end) -> new_handle
            ("array_splice", t_f64_f64_f64_f64),  // (handle, start, deleteCount) -> removed_handle
            ("array_shift", t_f64_f64),           // (handle) -> value
            ("array_unshift", t_f64_f64_void),    // (handle, value) -> void
            ("array_join", t_f64_f64_f64),        // (handle, separator) -> string
            ("array_index_of", t_f64_f64_f64),    // (handle, value) -> index
            ("array_includes", t_f64_f64_i32),    // (handle, value) -> i32
            ("array_concat", t_f64_f64_f64),      // (handle1, handle2) -> new_handle
            ("array_reverse", t_f64_f64),         // (handle) -> handle
            ("array_flat", t_f64_f64),            // (handle) -> new_handle
            ("array_is_array", t_f64_i32),        // (value) -> i32
            ("array_from", t_f64_f64),            // (value) -> handle
            ("array_push_spread", t_f64_f64_f64), // (target, source) -> handle (chaining)
            // Phase 1: Strings
            ("string_charAt", t_f64_f64_f64), // (str, idx) -> str
            ("string_substring", t_f64_f64_f64_f64), // (str, start, end) -> str
            ("string_indexOf", t_f64_f64_f64), // (str, search) -> number
            ("string_slice", t_f64_f64_f64_f64), // (str, start, end) -> str
            ("string_toLowerCase", t_f64_f64),
            ("string_toUpperCase", t_f64_f64),
            ("string_trim", t_f64_f64),
            ("string_includes", t_f64_f64_i32),
            ("string_startsWith", t_f64_f64_i32),
            ("string_endsWith", t_f64_f64_i32),
            ("string_replace", t_f64_f64_f64_f64), // (str, pat, repl) -> str
            ("string_split", t_f64_f64_f64),       // (str, delim) -> array_handle
            ("string_fromCharCode", t_f64_f64),    // (code) -> str
            ("string_padStart", t_f64_f64_f64_f64), // (str, len, fill) -> str
            ("string_padEnd", t_f64_f64_f64_f64),
            ("string_repeat", t_f64_f64_f64), // (str, count) -> str
            ("string_match", t_f64_f64_f64),  // (str, regex) -> array_handle
            ("math_log2", t_f64_f64),
            ("math_log10", t_f64_f64),
            // Phase 2: Closures
            ("closure_new", t_f64_f64_f64), // (func_table_idx, capture_count) -> handle
            ("closure_set_capture", t_f64_f64_f64_f64), // (handle, idx, value) -> handle (chaining)
            ("closure_call_0", t_f64_f64),  // (handle) -> result
            ("closure_call_1", t_f64_f64_f64), // (handle, arg0) -> result
            ("closure_call_2", t_f64_f64_f64_f64), // (handle, arg0, arg1) -> result
            ("closure_call_3", t_f64_f64_f64_f64_f64), // (handle, arg0, arg1, arg2) -> result
            ("closure_call_spread", t_f64_f64_f64), // (handle, args_array) -> result
            // Phase 2: Array higher-order
            ("array_map", t_f64_f64_f64), // (handle, closure) -> new_handle
            ("array_filter", t_f64_f64_f64),
            ("array_forEach", t_f64_f64_void), // (handle, closure) -> void
            ("array_reduce", t_f64_f64_f64_f64), // (handle, closure, initial) -> value
            ("array_find", t_f64_f64_f64),     // (handle, closure) -> value
            ("array_find_index", t_f64_f64_f64), // (handle, closure) -> number
            ("array_sort", t_f64_f64_f64),     // (handle, closure) -> handle
            ("array_some", t_f64_f64_i32),     // (handle, closure) -> i32
            ("array_every", t_f64_f64_i32),    // (handle, closure) -> i32
            // Phase 3: Classes
            ("class_new", t_f64_f64_f64), // (class_id, field_count) -> handle
            ("class_set_method", t_f64_f64_f64_void), // (class_id, name_str, func_table_idx) -> void
            ("class_call_method", t_f64_f64_f64_f64), // (handle, name_str, args_array) -> result
            ("class_get_field", t_f64_f64_f64),       // (handle, name_str) -> value
            ("class_set_field", t_f64_f64_f64_void),  // (handle, name_str, value) -> void
            ("class_set_static", t_f64_f64_f64_void), // (class_id, name_str, value) -> void
            ("class_get_static", t_f64_f64_f64),      // (class_id, name_str) -> value
            ("class_instanceof", t_f64_f64_i32),      // (handle, class_id) -> i32
            // Phase 4: JSON
            ("json_parse", t_f64_f64),     // (str) -> handle
            ("json_stringify", t_f64_f64), // (value) -> str
            // Phase 4: Map
            ("map_new", t_void_f64),
            ("map_set", t_f64_f64_f64_void), // (handle, key, value) -> void
            ("map_get", t_f64_f64_f64),
            ("map_has", t_f64_f64_i32),
            ("map_delete", t_f64_f64_void),
            ("map_size", t_f64_f64),
            ("map_clear", t_f64_void),
            ("map_entries", t_f64_f64),
            ("map_keys", t_f64_f64),
            ("map_values", t_f64_f64),
            // Phase 4: Set
            ("set_new", t_void_f64),
            ("set_new_from_array", t_f64_f64),
            ("set_add", t_f64_f64_void),
            ("set_has", t_f64_f64_i32),
            ("set_delete", t_f64_f64_void),
            ("set_size", t_f64_f64),
            ("set_clear", t_f64_void),
            ("set_values", t_f64_f64),
            // Phase 4: Date
            ("date_new_val", t_f64_f64), // (opt_arg) -> handle
            ("date_get_time", t_f64_f64),
            ("date_to_iso_string", t_f64_f64),
            ("date_get_full_year", t_f64_f64),
            ("date_get_month", t_f64_f64),
            ("date_get_date", t_f64_f64),
            ("date_get_day", t_f64_f64),
            ("date_get_hours", t_f64_f64),
            ("date_get_minutes", t_f64_f64),
            ("date_get_seconds", t_f64_f64),
            ("date_get_milliseconds", t_f64_f64),
            // Phase 4: Error
            ("error_new", t_f64_f64),     // (message) -> handle
            ("error_message", t_f64_f64), // (handle) -> string
            // Phase 4: RegExp
            ("regexp_new", t_f64_f64_f64), // (pattern, flags) -> handle
            ("regexp_test", t_f64_f64_i32), // (regex, str) -> i32
            // Phase 4: Globals
            ("number_coerce", t_f64_f64),
            ("is_nan", t_f64_i32),
            ("is_finite", t_f64_i32),
            // Phase 5
            ("console_log_multi", t_f64_void), // (args_array) -> void
            // Phase 1 addition: Class inheritance
            ("class_set_parent", t_f64_f64_void), // (child_str, parent_str) -> void
            // Phase 3: Try/Catch
            ("try_start", t_void),         // () -> void
            ("try_end", t_void),           // () -> void
            ("throw_value", t_f64_void),   // (val) -> void
            ("has_exception", t_void_i32), // () -> i32
            ("get_exception", t_void_f64), // () -> f64
            // Phase 4: URL
            ("url_parse", t_f64_f64), // (url_str) -> handle
            ("url_get_href", t_f64_f64),
            ("url_get_pathname", t_f64_f64),
            ("url_get_hostname", t_f64_f64),
            ("url_get_port", t_f64_f64),
            ("url_get_search", t_f64_f64),
            ("url_get_hash", t_f64_f64),
            ("url_get_origin", t_f64_f64),
            ("url_get_protocol", t_f64_f64),
            ("url_get_search_params", t_f64_f64),
            ("searchparams_get", t_f64_f64_f64), // (handle, key) -> str
            ("searchparams_has", t_f64_f64_i32), // (handle, key) -> i32
            ("searchparams_set", t_f64_f64_f64_void), // (handle, key, val) -> void
            ("searchparams_append", t_f64_f64_f64_void),
            ("searchparams_delete", t_f64_f64_void),
            ("searchparams_to_string", t_f64_f64),
            // Phase 4: Crypto
            ("crypto_random_uuid", t_void_f64),
            ("crypto_random_bytes", t_f64_f64),
            // Phase 4: Path
            ("path_join", t_f64_f64_f64), // (a, b) -> str
            ("path_dirname", t_f64_f64),
            ("path_basename", t_f64_f64),
            ("path_extname", t_f64_f64),
            ("path_resolve", t_f64_f64),
            // Phase 4: Process/OS
            ("os_platform", t_void_f64),
            ("process_argv", t_void_f64),
            ("process_cwd", t_void_f64),
            // Phase 6: Buffer
            ("buffer_alloc", t_f64_f64),
            ("buffer_from_string", t_f64_f64_f64),
            ("buffer_to_string", t_f64_f64_f64),
            ("buffer_get", t_f64_f64_f64),
            ("buffer_set", t_f64_f64_f64_void),
            ("buffer_length", t_f64_f64),
            ("buffer_slice", t_f64_f64_f64_f64),
            ("buffer_concat", t_f64_f64),
            ("uint8array_new", t_f64_f64),
            ("uint8array_from", t_f64_f64),
            ("uint8array_length", t_f64_f64),
            ("uint8array_get", t_f64_f64_f64),
            ("uint8array_set", t_f64_f64_f64_void),
            // Timers
            ("set_timeout", t_f64_f64_f64), // (closure, delay) -> timer_id
            ("set_interval", t_f64_f64_f64), // (closure, delay) -> timer_id
            ("clear_timeout", t_f64_void),  // (id) -> void
            ("clear_interval", t_f64_void), // (id) -> void
            // Response properties
            ("response_status", t_f64_f64), // (handle) -> number
            ("response_ok", t_f64_i32),     // (handle) -> i32
            ("response_headers_get", t_f64_f64_f64), // (handle, name) -> str
            ("response_url", t_f64_f64),    // (handle) -> str
            // Buffer extras
            ("buffer_copy", {
                self.get_type_idx(vec![ValType::I64; 5], vec![ValType::I64])
            }),
            ("buffer_write", t_f64_f64_f64_f64), // (handle, str, offset, encoding) -> number
            ("buffer_equals", t_f64_f64_i32),    // (handle, other) -> i32
            ("buffer_is_buffer", t_f64_i32),     // (val) -> i32
            ("buffer_byte_length", t_f64_f64),   // (val) -> number
            // Crypto extras
            ("crypto_sha256", t_f64_f64), // (data) -> promise_handle
            ("crypto_md5", t_f64_f64),    // (data) -> undefined
            // Path extras
            ("path_is_absolute", t_f64_i32), // (str) -> i32
            // Phase 5: Async/Promise/Fetch
            ("fetch_url", t_f64_f64), // (url_str) -> promise_handle
            ("fetch_with_options", t_f64_f64_f64_f64), // (url, method, body, headers_obj) -> promise_handle
            ("response_json", t_f64_f64),              // (response_handle) -> promise_handle
            ("response_text", t_f64_f64),              // (response_handle) -> promise_handle
            ("promise_new", t_void_f64),               // () -> promise_handle
            ("promise_resolve", t_f64_f64_void),       // (promise_handle, value) -> void
            ("promise_then", t_f64_f64_f64), // (promise_handle, closure_handle) -> promise_handle
            ("await_promise", t_f64_f64),    // (value) -> resolved_value_or_value
            // Memory-based bridge: args written to WASM memory at 0xFF00, only plain numbers as params
            ("mem_call", {
                self.get_type_idx(
                    vec![ValType::F64, ValType::F64, ValType::I32],
                    vec![ValType::F64],
                )
            }), // (func_name_id, arg_count, base_addr) -> f64 dummy
            ("mem_call_i32", {
                self.get_type_idx(
                    vec![ValType::F64, ValType::F64, ValType::I32],
                    vec![ValType::I32],
                )
            }), // (func_name_id, arg_count, base_addr) -> i32
        ];

        // Collect all closures from all modules (they need function indices too).
        // Track the module index so closures can be associated with their parent module's func_map.
        let mut closure_funcs: Vec<(
            FuncId,
            Vec<Param>,
            Vec<Stmt>,
            Vec<LocalId>,
            Vec<LocalId>,
            usize,
        )> = Vec::new();
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            let mut module_closures: Vec<(
                FuncId,
                Vec<Param>,
                Vec<Stmt>,
                Vec<LocalId>,
                Vec<LocalId>,
            )> = Vec::new();
            collect_closures_from_stmts(&module.init, &mut module_closures);
            for func in &module.functions {
                collect_closures_from_stmts(&func.body, &mut module_closures);
            }
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    collect_closures_from_stmts(&ctor.body, &mut module_closures);
                }
                for method in &class.methods {
                    collect_closures_from_stmts(&method.body, &mut module_closures);
                }
                for method in &class.static_methods {
                    collect_closures_from_stmts(&method.body, &mut module_closures);
                }
                for (_, getter) in &class.getters {
                    collect_closures_from_stmts(&getter.body, &mut module_closures);
                }
                for (_, setter) in &class.setters {
                    collect_closures_from_stmts(&setter.body, &mut module_closures);
                }
                for field in &class.fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut module_closures);
                    }
                }
                for field in &class.static_fields {
                    if let Some(init) = &field.init {
                        collect_closures_from_expr(init, &mut module_closures);
                    }
                }
            }
            for (fid, params, body, caps, mut_caps) in module_closures {
                closure_funcs.push((fid, params, body, caps, mut_caps, mod_idx));
            }
        }

        // Register async functions as additional bridge imports (Phase 1: assign import indices).
        // JS code generation is deferred to Phase 2 after per-module func_maps are built.
        let mut async_import_idx = self.num_imports;
        let mut per_module_async: Vec<Vec<(FuncId, u32)>> = Vec::new();
        for (_, module) in modules.iter() {
            let mut module_async_entries = Vec::new();
            for func in &module.functions {
                if func.is_async {
                    let param_count = func.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64]; // returns promise handle
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    module_async_entries.push((func.id, async_import_idx));
                    self.func_name_map
                        .insert(func.name.clone(), async_import_idx);
                    self.async_func_imports.push((
                        func.name.clone(),
                        async_import_idx,
                        param_count,
                    ));
                    async_import_idx += 1;
                }
            }
            per_module_async.push(module_async_entries);
        }
        self.num_imports = async_import_idx;

        // Register external FFI functions as WASM imports under the "ffi" namespace.
        // These are `declare function` statements with no body (e.g., bloom_init_window).
        // Deduplicate by name since the same extern can appear in multiple modules.
        let mut ffi_import_idx = self.num_imports;
        let mut seen_ffi: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for (_, module) in modules {
            for (name, param_types, return_type) in &module.extern_funcs {
                if seen_ffi.contains(name) {
                    continue;
                }
                seen_ffi.insert(name.clone());
                let param_count = param_types.len();
                let has_return = !matches!(return_type, perry_types::Type::Void);
                let params = vec![ValType::I64; param_count];
                let results = if has_return {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                self.func_name_map.insert(name.clone(), ffi_import_idx);
                self.ffi_imports
                    .push((name.clone(), param_count, has_return));
                ffi_import_idx += 1;
            }
        }
        self.num_imports = ffi_import_idx;

        // Now set user_func_idx AFTER all imports (including async and FFI) are registered
        let mut user_func_idx = self.num_imports;

        // __init_strings function
        let init_strings_idx = user_func_idx;
        let init_strings_type = t_void;
        user_func_idx += 1;

        // Register user functions from all modules (skip async ones).
        // FuncId is only unique within a module, so we build per-module func_maps
        // to avoid cross-module FuncId collisions (e.g., module A's FuncId(2) != module B's FuncId(2)).
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            let mut module_fm: BTreeMap<FuncId, u32> = BTreeMap::new();
            // Include async function mappings for this module
            for &(fid, idx) in &per_module_async[mod_idx] {
                module_fm.insert(fid, idx);
            }
            for func in &module.functions {
                if func.is_async {
                    continue; // already registered as bridge import
                }
                let param_count = func.params.len();
                let params = vec![ValType::I64; param_count];
                let results = if func.body.iter().any(has_return) || func.name == "main" {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let is_void = results.is_empty();
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                module_fm.insert(func.id, user_func_idx);
                if is_void {
                    self.void_funcs.insert(user_func_idx);
                }
                self.func_param_counts.insert(user_func_idx, param_count);
                // Build func_name_map for ExternFuncRef resolution (name is globally unique)
                self.func_name_map.insert(func.name.clone(), user_func_idx);
                user_func_idx += 1;
            }
            self.module_func_maps.push(module_fm);
        }

        // Register class constructors, methods, and static methods
        for (_, module) in modules {
            for class in &module.classes {
                // Record parent class relationship
                if let Some(parent) = &class.extends_name {
                    self.class_parent_map
                        .insert(class.name.clone(), parent.clone());
                }
                // Constructor: params = this + declared params, returns f64 (this)
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_ctor_map
                        .insert(class.name.clone(), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Instance methods: params = this + declared params
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(method.name.clone(), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Static methods: no this param
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_static_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(method.name.clone(), user_func_idx);
                    // Also register in func_name_map for cross-module resolution
                    self.func_name_map
                        .insert(format!("{}_{}", class.name, method.name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, param_count);
                    user_func_idx += 1;
                }
                // Getters: like methods with 0 params + this
                for (name, getter) in &class.getters {
                    let params = vec![ValType::I64]; // just this
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(format!("__get_{}", name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, 1);
                    let _ = getter;
                    user_func_idx += 1;
                }
                // Setters: this + value
                for (name, setter) in &class.setters {
                    let params = vec![ValType::I64; 2]; // this + value
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_method_map
                        .entry(class.name.clone())
                        .or_default()
                        .insert(format!("__set_{}", name), user_func_idx);
                    self.func_param_counts.insert(user_func_idx, 2);
                    let _ = setter;
                    user_func_idx += 1;
                }
            }
        }

        // Register closure functions into their per-module func_maps
        for (func_id, params, body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if !self.module_func_maps[*mod_idx].contains_key(func_id) {
                // Closure params: captures first (as f64), then declared params
                let total_params = captures.len() + mutable_captures.len() + params.len();
                let wasm_params = vec![ValType::I64; total_params];
                let results = if body.iter().any(has_return) {
                    vec![ValType::I64]
                } else {
                    vec![ValType::I64] // closures always return i64
                };
                let type_idx = self.get_type_idx(wasm_params, results);
                let _ = type_idx;
                self.module_func_maps[*mod_idx].insert(*func_id, user_func_idx);
                user_func_idx += 1;
            }
        }

        // Async function JS generation (Phase 2): now that per-module func_maps are complete,
        // generate the JS code for async functions with correct FuncRef resolution.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            for func in &module.functions {
                if func.is_async {
                    let js_code = self.emit_js_async_function(func);
                    self.async_js_code.push(js_code);
                }
            }
        }

        // _start function (entry point). #854: the trailing
        // `user_func_idx += 1` was dead — nothing later in this function
        // reads the counter.
        let start_idx = user_func_idx;
        let start_type = t_void;

        // Register globals from all modules
        for (_, module) in modules {
            for global in &module.globals {
                self.global_map.insert(global.id, self.num_globals);
                self.num_globals += 1;
            }
        }

        // Promote module-level Let bindings to WASM globals so cross-function
        // references work and so different modules' identical LocalIds don't collide.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            collect_module_let_ids(
                &module.init,
                mod_idx,
                &mut self.module_let_globals,
                &mut self.num_globals,
            );
        }

        // Issue #1071: build cross-module imported-variable → WASM global map.
        // The HIR lowers `import { FOO } from './m'` value reads (where FOO is
        // an exported `const`/`let`, not a function) to `Expr::ExternFuncRef {
        // name: "FOO" }`. Pre-fix this hit `TAG_UNDEFINED` because `name`
        // wasn't in `func_name_map` (it's a variable, not a function). Now we
        // resolve `name` to the source module's `Stmt::Let` and reuse its
        // wasm-global slot from `module_let_globals`. Same flow the LLVM target
        // achieves via per-export `perry_fn_<src>__<name>()` getter functions.
        //
        // Module-path lookup: each `Import` carries `resolved_path` (set by
        // the driver) and `Module.name` is a relative-from-project-root path.
        // We compare paths by file-stem match against `Module.name` (which is
        // a leaf "name.ts" or "subdir/name.ts" string), falling back to a
        // basename match. Re-exports (`Export::ReExport`) point at another
        // module by `source`; we don't chase those here — a one-hop re-export
        // is handled by the source's own exports list (the re-export pass
        // typically flattens through), and complex chains can be added later
        // with a visited-set on demand.
        {
            // module.name → source module index
            let name_to_idx: std::collections::HashMap<&str, usize> = modules
                .iter()
                .enumerate()
                .map(|(i, (_, m))| (m.name.as_str(), i))
                .collect();
            // For each source module, build a name → wasm global lookup over
            // its top-level Lets so we can resolve `Export::Named { local }`.
            let mut src_let_names: Vec<std::collections::HashMap<String, u32>> =
                Vec::with_capacity(modules.len());
            for (src_idx, (_, module)) in modules.iter().enumerate() {
                let mut map: std::collections::HashMap<String, u32> = Default::default();
                for stmt in &module.init {
                    if let perry_hir::Stmt::Let { id, name, .. } = stmt {
                        if let Some(&gidx) = self.module_let_globals.get(&(src_idx, *id)) {
                            map.insert(name.clone(), gidx);
                        }
                    }
                }
                src_let_names.push(map);
            }
            for (consumer_idx, (_, module)) in modules.iter().enumerate() {
                for import in &module.imports {
                    if import.type_only {
                        continue;
                    }
                    // Resolve source module index. Prefer matching `resolved_path`
                    // against `(path, module)` pairs by stem; fall back to a
                    // suffix/basename match on `import.source`.
                    let src_idx_opt = resolve_source_module_idx(modules, import, &name_to_idx);
                    let Some(src_idx) = src_idx_opt else { continue };
                    let src_lets = &src_let_names[src_idx];
                    for spec in &import.specifiers {
                        if let perry_hir::ir::ImportSpecifier::Named { imported, local } = spec {
                            // Walk the source module's exports to map the
                            // public `imported` name back to a source-local
                            // identifier, then look up that identifier's let.
                            let src_module = &modules[src_idx].1;
                            let mut resolved_local: Option<&str> = None;
                            for export in &src_module.exports {
                                if let perry_hir::ir::Export::Named {
                                    local: src_local,
                                    exported,
                                } = export
                                {
                                    if exported == imported {
                                        resolved_local = Some(src_local.as_str());
                                        break;
                                    }
                                }
                            }
                            // Direct fall-through: if no Export::Named matched
                            // but a Let with the imported name exists, use it.
                            // (Some HIR lowering shapes register exports out-of-
                            // band; this keeps `export const X = ...` robust.)
                            let key = resolved_local.unwrap_or(imported.as_str());
                            if let Some(&gidx) = src_lets.get(key) {
                                self.imported_var_globals
                                    .insert((consumer_idx, local.clone()), gidx);
                            }
                        }
                    }
                }
            }
        }

        // Add a NaN-safe temp global for mem_store_slot (Firefox canonicalizes locals)
        self.nan_temp_global = self.num_globals;
        self.num_globals += 1;

        // Build the WASM module
        let mut wasm_module = Module::new();

        // --- Type section ---
        let mut type_section = TypeSection::new();
        for (params, results) in &self.types {
            type_section
                .ty()
                .function(params.iter().copied(), results.iter().copied());
        }
        wasm_module.section(&type_section);

        // --- Import section ---
        let mut import_section = ImportSection::new();
        for (name, type_idx) in &import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add async function imports
        let async_import_entries: Vec<(String, u32)> = self
            .async_func_imports
            .iter()
            .map(|(name, _idx, param_count)| {
                let import_name = format!("__async_{}", name);
                let params = vec![ValType::I64; *param_count];
                let results = vec![ValType::I64];
                let key = (params, results);
                let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
                (import_name, type_idx)
            })
            .collect();
        for (name, type_idx) in &async_import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add FFI function imports under "ffi" namespace
        for (name, param_count, has_return) in &self.ffi_imports {
            let params = vec![ValType::I64; *param_count];
            let results = if *has_return {
                vec![ValType::I64]
            } else {
                vec![]
            };
            let key = (params, results);
            let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
            import_section.import("ffi", name, EntityType::Function(type_idx));
        }
        wasm_module.section(&import_section);

        // --- Function section (declares type indices for each defined function) ---
        let mut func_section = FunctionSection::new();
        // __init_strings
        func_section.function(init_strings_type);
        // User functions (skip async — they are imports)
        for (_, module) in modules {
            for func in &module.functions {
                if func.is_async {
                    continue;
                }
                let param_count = func.params.len();
                let params = vec![ValType::I64; param_count];
                let results = if func.body.iter().any(has_return) || func.name == "main" {
                    vec![ValType::I64]
                } else {
                    vec![]
                };
                let type_idx = self.get_type_idx(params, results);
                func_section.function(type_idx);
            }
        }
        // Class constructors, methods, static methods, getters, setters
        for (_, module) in modules {
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.methods {
                    let param_count = 1 + method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for method in &class.static_methods {
                    let param_count = method.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _getter) in &class.getters {
                    let params = vec![ValType::I64];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
                for (_name, _setter) in &class.setters {
                    let params = vec![ValType::I64; 2];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    func_section.function(type_idx);
                }
            }
        }
        // Closure functions
        for (func_id, _params, _body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if self.module_func_maps[*mod_idx].contains_key(func_id) {
                let total_params = captures.len() + mutable_captures.len() + _params.len();
                let wasm_params = vec![ValType::I64; total_params];
                let results = vec![ValType::I64]; // closures always return f64
                let type_idx = self.get_type_idx(wasm_params, results);
                func_section.function(type_idx);
            }
        }
        // _start
        func_section.function(start_type);
        wasm_module.section(&func_section);

        // --- Table section (for indirect calls / closures) ---
        // Must come after Function section but before Memory section (WASM spec ordering)
        let all_func_indices: Vec<u32> = {
            let mut indices = vec![init_strings_idx]; // placeholder at index 0
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                for func in &module.functions {
                    if let Some(&idx) = self.module_func_maps[mod_idx].get(&func.id) {
                        indices.push(idx);
                    }
                }
            }
            // Add class constructor/method/static indices
            for idx in self.class_ctor_map.values() {
                if !indices.contains(idx) {
                    indices.push(*idx);
                }
            }
            for methods in self.class_method_map.values() {
                for idx in methods.values() {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for statics in self.class_static_map.values() {
                for idx in statics.values() {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for (func_id, _, _, _, _, mod_idx) in &closure_funcs {
                if let Some(&idx) = self.module_func_maps[*mod_idx].get(func_id) {
                    if !indices.contains(&idx) {
                        indices.push(idx);
                    }
                }
            }
            indices.push(start_idx);
            indices
        };
        // Build reverse map: wasm func index → table position
        for (table_idx, &func_idx) in all_func_indices.iter().enumerate() {
            self.func_to_table_idx.insert(func_idx, table_idx as u32);
        }

        let table_size = all_func_indices.len() as u32;
        {
            let mut table_section = TableSection::new();
            table_section.table(TableType {
                element_type: RefType::FUNCREF,
                minimum: table_size as u64,
                maximum: Some(table_size as u64),
                table64: false,
                shared: false,
            });
            wasm_module.section(&table_section);
        }

        // --- Memory section ---
        let mut mem_section = MemorySection::new();
        let pages = self.string_data.len().div_ceil(65536).max(2) as u64; // min 2 pages for 0xFF00 mem_call region
        mem_section.memory(MemoryType {
            minimum: pages,
            maximum: None,
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        wasm_module.section(&mem_section);

        // --- Global section ---
        if self.num_globals > 0 {
            let mut global_section = GlobalSection::new();
            for g in 0..self.num_globals {
                if g == self.nan_temp_global {
                    // Stack pointer for arg buffer (i32, initialized to 0x10000)
                    global_section.global(
                        GlobalType {
                            val_type: ValType::I32,
                            mutable: true,
                            shared: false,
                        },
                        &wasm_encoder::ConstExpr::i32_const(0x10000),
                    );
                } else {
                    // Regular i64 global for module-level variables (NaN-boxed)
                    global_section.global(
                        GlobalType {
                            val_type: ValType::I64,
                            mutable: true,
                            shared: false,
                        },
                        &wasm_encoder::ConstExpr::i64_const(TAG_UNDEFINED as i64),
                    );
                }
            }
            wasm_module.section(&global_section);
        }

        // --- Export section ---
        let mut export_section = ExportSection::new();
        export_section.export("_start", ExportKind::Func, start_idx);
        export_section.export("memory", ExportKind::Memory, 0);
        export_section.export("__indirect_function_table", ExportKind::Table, 0);
        // Export all user functions so async JS code can call them by index.
        for idx in self.num_imports..start_idx {
            export_section.export(&format!("__wasm_func_{}", idx), ExportKind::Func, idx);
        }
        // Issue #1071: export the globals that back cross-module imported
        // variables so the async/JS-context emission path (which can't issue
        // a `global.get` instruction) can read them via
        // `wasmInstance.exports.__wasm_global_<idx>.value`. We export ALL
        // module-let globals since they're already named by index and the
        // export cost is negligible; future asynchrony work that needs the
        // same boundary read won't have to wire a separate index.
        {
            let mut exported_globals: std::collections::BTreeSet<u32> =
                std::collections::BTreeSet::new();
            for &gidx in self.module_let_globals.values() {
                exported_globals.insert(gidx);
            }
            for &gidx in self.global_map.values() {
                exported_globals.insert(gidx);
            }
            for gidx in exported_globals {
                export_section.export(&format!("__wasm_global_{}", gidx), ExportKind::Global, gidx);
            }
        }
        wasm_module.section(&export_section);

        // --- Element section (populate the indirect call table) ---
        {
            let mut elem_section = ElementSection::new();
            elem_section.active(
                Some(0),                                // table index
                &wasm_encoder::ConstExpr::i32_const(0), // offset
                Elements::Functions(std::borrow::Cow::Borrowed(&all_func_indices)),
            );
            wasm_module.section(&elem_section);
        }

        // --- DataCount section (required before Code when Data section exists) ---
        if !self.string_data.is_empty() {
            wasm_module.section(&wasm_encoder::DataCountSection { count: 1 });
        }

        // --- Code section ---
        let mut code_section = CodeSection::new();

        // __init_strings: register all string literals with the JS runtime
        {
            let mut func = Function::new(vec![]);
            for (_content, offset, len) in &self.string_table {
                func.instruction(&Instruction::I32Const(*offset as i32));
                func.instruction(&Instruction::I32Const(*len as i32));
                func.instruction(&Instruction::Call(rt.string_new));
            }
            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        // User functions (skip async — they are JS bridge imports).
        // Swap in the per-module func_map so FuncRef(id) resolves correctly within each module.
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            self.current_mod_idx = mod_idx;
            for hir_func in &module.functions {
                if hir_func.is_async {
                    continue;
                }
                let func = self.compile_function(hir_func);
                code_section.function(&func);
            }
        }

        // Class constructors, methods, static methods, getters, setters
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            self.func_map = self.module_func_maps[mod_idx].clone();
            self.current_mod_idx = mod_idx;
            for class in &module.classes {
                if let Some(ctor) = &class.constructor {
                    let func = self.compile_class_constructor(class, ctor);
                    code_section.function(&func);
                }
                for method in &class.methods {
                    let func = self.compile_class_method(method);
                    code_section.function(&func);
                }
                for method in &class.static_methods {
                    let func = self.compile_function(method);
                    code_section.function(&func);
                }
                for (_name, getter) in &class.getters {
                    let func = self.compile_class_method(getter);
                    code_section.function(&func);
                }
                for (_name, setter) in &class.setters {
                    let func = self.compile_class_method(setter);
                    code_section.function(&func);
                }
            }
        }

        // Closure functions — swap in the parent module's func_map for each closure
        for (func_id, params, body, captures, mutable_captures, mod_idx) in &closure_funcs {
            if self.module_func_maps[*mod_idx].contains_key(func_id) {
                self.func_map = self.module_func_maps[*mod_idx].clone();
                self.current_mod_idx = *mod_idx;
                let func = self.compile_closure(params, body, captures, mutable_captures);
                code_section.function(&func);
            }
        }

        // _start: call __init_strings, then execute module init code
        {
            // Collect locals PER-MODULE so LocalIds don't collide across modules.
            // Each module declares Lets starting from id 0, so without per-module maps
            // module B's `let id=1` would alias module A's `let id=1`.
            let mut per_module_init_locals: Vec<BTreeMap<LocalId, u32>> =
                Vec::with_capacity(modules.len());
            let mut total_count = 0u32;
            for (_, module) in modules {
                let mut mod_map = BTreeMap::new();
                collect_locals(&module.init, &mut mod_map, &mut total_count, 0);
                per_module_init_locals.push(mod_map);
            }
            // Empty fallback map for global initializers and class field inits that
            // shouldn't reference module-level lets.
            let init_locals: BTreeMap<LocalId, u32> = BTreeMap::new();

            let num_locals = total_count;
            let start_temp_local = num_locals;
            let start_temp_i32 = num_locals + 2;
            let locals = vec![(num_locals + 2, ValType::I64), (1, ValType::I32)];
            let mut func = Function::new(locals);

            // Call __init_strings first
            func.instruction(&Instruction::Call(init_strings_idx));

            // Initialize globals — swap in per-module func_map for correct FuncRef resolution
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                for global in &module.globals {
                    if let Some(init) = &global.init {
                        let mut ctx =
                            FuncEmitCtx::new(self, &init_locals, start_temp_local, start_temp_i32);
                        ctx.emit_expr(&mut func, init);
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    } else if global.name == "__platform__" {
                        // Web platform ID = 5
                        func.instruction(&f64_const(5.0));
                        func.instruction(&Instruction::I64ReinterpretF64);
                        let gidx = self.global_map[&global.id];
                        func.instruction(&Instruction::GlobalSet(gidx));
                    }
                }
            }

            // Register class methods with the bridge and set up inheritance
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                for class in &module.classes {
                    let class_name_id = self
                        .string_map
                        .get(class.name.as_str())
                        .copied()
                        .unwrap_or(0);
                    let class_bits = (STRING_TAG << 48) | (class_name_id as u64);

                    // Register instance methods in classMethodTable (including getters/setters)
                    if let Some(methods) = self.class_method_map.get(&class.name) {
                        for (method_name, &func_idx) in methods {
                            let real_name = method_name.as_str();
                            let method_name_id =
                                self.string_map.get(real_name).copied().unwrap_or(0);
                            let method_bits = (STRING_TAG << 48) | (method_name_id as u64);
                            let table_idx = self
                                .func_to_table_idx
                                .get(&func_idx)
                                .copied()
                                .unwrap_or(func_idx);
                            // Store args to memory for mem_call (Firefox NaN-safe: use I64Store)
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(method_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::I64Const(
                                (table_idx as f64).to_bits() as i64
                            ));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            let csm_id = self
                                .string_map
                                .get("class_set_method")
                                .copied()
                                .unwrap_or(0);
                            func.instruction(&f64_const(csm_id as f64));
                            func.instruction(&f64_const(3.0));
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::Call(rt.mem_call));
                            func.instruction(&Instruction::Drop);
                        }
                    }

                    // Set up inheritance
                    if let Some(parent_name) = &class.extends_name {
                        let parent_name_id = self
                            .string_map
                            .get(parent_name.as_str())
                            .copied()
                            .unwrap_or(0);
                        let parent_bits = (STRING_TAG << 48) | (parent_name_id as u64);
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::I64Const(class_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        func.instruction(&Instruction::I32Const(0xFF08));
                        func.instruction(&Instruction::I64Const(parent_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                            offset: 0,
                            align: 3,
                            memory_index: 0,
                        }));
                        let csp_id = self
                            .string_map
                            .get("class_set_parent")
                            .copied()
                            .unwrap_or(0);
                        func.instruction(&f64_const(csp_id as f64));
                        func.instruction(&f64_const(2.0));
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::Call(rt.mem_call));
                        func.instruction(&Instruction::Drop);
                    }

                    // Register static fields
                    for field in &class.static_fields {
                        if let Some(init) = &field.init {
                            let field_name_id = self
                                .string_map
                                .get(field.name.as_str())
                                .copied()
                                .unwrap_or(0);
                            let field_bits = (STRING_TAG << 48) | (field_name_id as u64);
                            // Store class name
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            // Store field name
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(field_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            // Store value
                            let mut ctx = FuncEmitCtx::new(
                                self,
                                &init_locals,
                                start_temp_local,
                                start_temp_i32,
                            );
                            ctx.emit_expr(&mut func, init);
                            // Use temp local to store the value
                            func.instruction(&Instruction::LocalSet(start_temp_local));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::LocalGet(start_temp_local));
                            // Value is already i64, no conversion needed
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg {
                                offset: 0,
                                align: 3,
                                memory_index: 0,
                            }));
                            let css_id = self
                                .string_map
                                .get("class_set_static")
                                .copied()
                                .unwrap_or(0);
                            func.instruction(&f64_const(css_id as f64));
                            func.instruction(&f64_const(3.0));
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::Call(rt.mem_call));
                            func.instruction(&Instruction::Drop);
                        }
                    }
                }
            }

            // Execute init statements from all modules — swap in per-module func_map
            // and per-module local map so LocalGets resolve to the correct WASM local
            // (or fall back to module_let_globals via current_mod_idx).
            for (mod_idx, (_, module)) in modules.iter().enumerate() {
                self.func_map = self.module_func_maps[mod_idx].clone();
                self.current_mod_idx = mod_idx;
                let mod_locals = &per_module_init_locals[mod_idx];
                let mut ctx = FuncEmitCtx::new(self, mod_locals, start_temp_local, start_temp_i32);
                for stmt in &module.init {
                    ctx.emit_stmt(&mut func, stmt, false);
                }
            }

            func.instruction(&Instruction::End);
            code_section.function(&func);
        }

        wasm_module.section(&code_section);

        // --- Data section (string literal bytes, must come after Code) ---
        if !self.string_data.is_empty() {
            let mut data_section = DataSection::new();
            data_section.active(
                0,
                &wasm_encoder::ConstExpr::i32_const(0),
                self.string_data.iter().copied(),
            );
            wasm_module.section(&data_section);
        }

        let wasm_bytes = wasm_module.finish();
        let async_js = self.async_js_code.join("\n");
        let ffi_import_names = self
            .ffi_imports
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect();
        WasmCompileOutput {
            wasm_bytes,
            async_js,
            ffi_imports: ffi_import_names,
        }
    }
}

/// Context for emitting a single function body
pub(super) struct FuncEmitCtx<'a> {
    pub(super) emitter: &'a WasmModuleEmitter,
    pub(super) local_map: &'a BTreeMap<LocalId, u32>,
    /// Block nesting depth for break/continue
    pub(super) break_depth: Vec<u32>,
    pub(super) loop_depth: Vec<u32>,
    pub(super) block_depth: u32,
    /// Stack of (label, break_depth, continue_depth) for labeled break/continue.
    /// When `Labeled { label, body }` is a loop, this ties the label to the loop's blocks.
    pub(super) label_stack: Vec<(String, u32, u32)>,
    /// Pending label to attach to the next loop encountered.
    pub(super) pending_label: Option<String>,
    /// Current class name (set when compiling class methods/constructors)
    pub(super) current_class: Option<String>,
    /// Index of a temp i64 local
    pub(super) temp_local: u32,
    /// Index of a temp i32 local (for mem_call base address)
    pub(super) temp_local_i32: u32,
    /// Index of a second temp i64 local for emit_store_arg
    pub(super) temp_store_local: u32,
    /// Current frame size for emit_store_arg address computation
    pub(super) current_frame_size: u32,
    /// Stack of saved frame sizes for nested frame support
    pub(super) frame_stack: Vec<u32>,
}

impl<'a> FuncEmitCtx<'a> {
    fn new(
        emitter: &'a WasmModuleEmitter,
        local_map: &'a BTreeMap<LocalId, u32>,
        temp_local: u32,
        temp_local_i32: u32,
    ) -> Self {
        Self {
            emitter,
            local_map,
            break_depth: Vec::new(),
            loop_depth: Vec::new(),
            block_depth: 0,
            label_stack: Vec::new(),
            pending_label: None,
            current_class: None,
            temp_local,
            temp_local_i32,
            temp_store_local: temp_local + 1,
            current_frame_size: 0,
            frame_stack: Vec::new(),
        }
    }
}
