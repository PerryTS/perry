//! HIR → WebAssembly bytecode emitter
//!
//! Translates HIR modules to WebAssembly binary format using wasm-encoder.
//! All JSValues are represented as i64 using NaN-boxing bit patterns.
//! Arithmetic operations temporarily convert to f64 and back.
//! Runtime operations (strings, console, objects) are imported from a JS bridge.

use perry_hir::ir::*;
use perry_types::{FuncId, LocalId, GlobalId};
use std::collections::BTreeMap;
use wasm_encoder::{
    CodeSection, DataSection, ElementSection, Elements, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, Ieee64, ImportSection, Instruction, MemorySection, MemoryType,
    Module, RefType, TableSection, TableType, TypeSection, ValType, GlobalSection, GlobalType,
};

#[derive(Clone)]
enum EnumResolvedValue {
    Number(f64),
    String(String),
}

/// Helper: create an F64Const instruction from raw f64 bits
fn f64_const(val: f64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(val))
}

/// Helper: create an F64Const instruction from NaN-boxed tag bits (kept for potential future use)
#[allow(dead_code)]
fn f64_const_bits(bits: u64) -> Instruction<'static> {
    Instruction::F64Const(Ieee64::from(f64::from_bits(bits)))
}

// NaN-boxing constants (must match perry-runtime and wasm_runtime.js)
const STRING_TAG: u64 = 0x7FFF;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

/// Import function indices (must match the order imports are added)
/// Most fields are unused directly but their indices define the WASM import order.
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct RuntimeImports {
    string_new: u32,
    console_log: u32,
    console_warn: u32,
    console_error: u32,
    string_concat: u32,
    js_add: u32,
    string_eq: u32,
    string_len: u32,
    jsvalue_to_string: u32,
    is_truthy: u32,
    js_strict_eq: u32,
    math_floor: u32,
    math_ceil: u32,
    math_round: u32,
    math_abs: u32,
    math_sqrt: u32,
    math_pow: u32,
    math_random: u32,
    math_log: u32,
    date_now: u32,
    js_typeof: u32,
    math_min: u32,
    math_max: u32,
    parse_int: u32,
    parse_float: u32,
    // Phase 0 additions
    js_mod: u32,
    is_null_or_undefined: u32,
    // Phase 1: Object operations
    object_new: u32,
    object_set: u32,
    object_get: u32,
    object_get_dynamic: u32,
    object_set_dynamic: u32,
    object_delete: u32,
    object_delete_dynamic: u32,
    object_keys: u32,
    object_values: u32,
    object_entries: u32,
    object_has_property: u32,
    object_assign: u32,
    // Phase 1: Array operations
    array_new: u32,
    array_push: u32,
    array_pop: u32,
    array_get: u32,
    array_set: u32,
    array_length: u32,
    array_slice: u32,
    array_splice: u32,
    array_shift: u32,
    array_unshift: u32,
    array_join: u32,
    array_index_of: u32,
    array_includes: u32,
    array_concat: u32,
    array_reverse: u32,
    array_flat: u32,
    array_is_array: u32,
    array_from: u32,
    array_push_spread: u32,
    // Phase 1: String methods
    string_char_at: u32,
    string_substring: u32,
    string_index_of: u32,
    string_slice: u32,
    string_to_lower_case: u32,
    string_to_upper_case: u32,
    string_trim: u32,
    string_includes: u32,
    string_starts_with: u32,
    string_ends_with: u32,
    string_replace: u32,
    string_split: u32,
    string_from_char_code: u32,
    string_pad_start: u32,
    string_pad_end: u32,
    string_repeat: u32,
    string_match: u32,
    math_log2: u32,
    math_log10: u32,
    // Phase 2: Closure operations
    closure_new: u32,
    closure_set_capture: u32,
    closure_call_0: u32,
    closure_call_1: u32,
    closure_call_2: u32,
    closure_call_3: u32,
    closure_call_spread: u32,
    // Phase 2: Array higher-order methods
    array_map: u32,
    array_filter: u32,
    array_for_each: u32,
    array_reduce: u32,
    array_find: u32,
    array_find_index: u32,
    array_sort: u32,
    array_some: u32,
    array_every: u32,
    // Phase 3: Class operations
    class_new: u32,
    class_set_method: u32,
    class_call_method: u32,
    class_get_field: u32,
    class_set_field: u32,
    class_set_static: u32,
    class_get_static: u32,
    class_instanceof: u32,
    // Phase 4: JSON
    json_parse: u32,
    json_stringify: u32,
    // Phase 4: Map
    map_new: u32,
    map_set: u32,
    map_get: u32,
    map_has: u32,
    map_delete: u32,
    map_size: u32,
    map_clear: u32,
    map_entries: u32,
    map_keys: u32,
    map_values: u32,
    // Phase 4: Set
    set_new: u32,
    set_new_from_array: u32,
    set_add: u32,
    set_has: u32,
    set_delete: u32,
    set_size: u32,
    set_clear: u32,
    set_values: u32,
    // Phase 4: Date
    date_new: u32,
    date_get_time: u32,
    date_to_iso_string: u32,
    date_get_full_year: u32,
    date_get_month: u32,
    date_get_date: u32,
    date_get_hours: u32,
    date_get_minutes: u32,
    date_get_seconds: u32,
    date_get_milliseconds: u32,
    // Phase 4: Error
    error_new: u32,
    error_message: u32,
    // Phase 4: RegExp
    regexp_new: u32,
    regexp_test: u32,
    // Phase 4: Globals
    number_coerce: u32,
    is_nan: u32,
    is_finite: u32,
    // Phase 5: Misc
    console_log_multi: u32,
    // Phase 1 addition: Class inheritance
    class_set_parent: u32,
    // Phase 3: Try/Catch
    try_start: u32,
    try_end: u32,
    throw_value: u32,
    has_exception: u32,
    get_exception: u32,
    // Phase 4: URL
    url_parse: u32,
    url_get_href: u32,
    url_get_pathname: u32,
    url_get_hostname: u32,
    url_get_port: u32,
    url_get_search: u32,
    url_get_hash: u32,
    url_get_origin: u32,
    url_get_protocol: u32,
    url_get_search_params: u32,
    searchparams_get: u32,
    searchparams_has: u32,
    searchparams_set: u32,
    searchparams_append: u32,
    searchparams_delete: u32,
    searchparams_to_string: u32,
    // Phase 4: Crypto
    crypto_random_uuid: u32,
    crypto_random_bytes: u32,
    // Phase 4: Path
    path_join: u32,
    path_dirname: u32,
    path_basename: u32,
    path_extname: u32,
    path_resolve: u32,
    // Phase 4: Process/OS
    os_platform: u32,
    process_argv: u32,
    process_cwd: u32,
    // Phase 6: Buffer
    buffer_alloc: u32,
    buffer_from_string: u32,
    buffer_to_string: u32,
    buffer_get: u32,
    buffer_set: u32,
    buffer_length: u32,
    buffer_slice: u32,
    buffer_concat: u32,
    uint8array_new: u32,
    uint8array_from: u32,
    uint8array_length: u32,
    uint8array_get: u32,
    uint8array_set: u32,
    // Timers
    set_timeout: u32,
    set_interval: u32,
    clear_timeout: u32,
    clear_interval: u32,
    // Response properties
    response_status: u32,
    response_ok: u32,
    response_headers_get: u32,
    response_url: u32,
    // Buffer extras
    buffer_copy: u32,
    buffer_write: u32,
    buffer_equals: u32,
    buffer_is_buffer: u32,
    buffer_byte_length: u32,
    // Crypto extras
    crypto_sha256: u32,
    crypto_md5: u32,
    // Path extras
    path_is_absolute: u32,
    // Phase 5: Async/Promise/Fetch
    fetch_url: u32,
    fetch_with_options: u32,
    response_json: u32,
    response_text: u32,
    promise_new: u32,
    promise_resolve: u32,
    promise_then: u32,
    await_promise: u32,
    // Bridge via WASM memory — Firefox canonicalizes NaN in function params,
    // so we write f64 args to memory (preserves NaN bits) and pass only plain numbers.
    mem_call: u32,   // (func_name_id, arg_count) -> result; args at mem[ARG_BASE..]
    mem_call_i32: u32, // (func_name_id, arg_count) -> i32; for is_truthy, string_eq, etc.
}

/// Map perry/ui and perry/system method names to bridge function names.
/// Mirrors the mapping in perry-codegen-js's emit_ui_method_call.
fn map_ui_method(method: &str, class_name: Option<&str>) -> &'static str {
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
        "setBackground" | "set_background" | "widgetSetBackgroundColor" => "perry_ui_set_background",
        "setForeground" | "set_foreground" | "textSetColor" => "perry_ui_set_foreground",
        "setFontSize" | "set_font_size" | "textSetFontSize" => "perry_ui_set_font_size",
        "setFontWeight" | "set_font_weight" | "textSetFontWeight" => "perry_ui_set_font_weight",
        "setFontFamily" | "set_font_family" | "textSetFontFamily" => "perry_ui_set_font_family",
        "setPadding" | "set_padding" => "perry_ui_set_padding",
        "setFrame" | "set_frame" => "perry_ui_set_frame",
        "setCornerRadius" | "set_corner_radius" => "perry_ui_set_corner_radius",
        "setBorder" | "set_border" => "perry_ui_set_border",
        "setOpacity" | "set_opacity" => "perry_ui_set_opacity",
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
        "get" if class_name.map_or(false, |c| c == "State") => "perry_ui_state_get",
        "set" if class_name.map_or(false, |c| c == "State") => "perry_ui_state_set",
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
        // Fallback: prefix with perry_ui_
        _ => return "perry_ui_unknown",
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
pub fn compile_to_wasm_with_async(modules: &[(String, perry_hir::ir::Module)]) -> WasmCompileOutput {
    let mut emitter = WasmModuleEmitter::new();
    emitter.compile(modules)
}

struct WasmModuleEmitter {
    /// String literal table: content → (string_id, offset, length)
    string_table: Vec<(String, u32, u32)>, // (content, offset, len)
    string_map: BTreeMap<String, u32>,      // content → string_id
    string_data: Vec<u8>,                   // packed string bytes
    /// Type section entries: (params, results)
    types: Vec<(Vec<ValType>, Vec<ValType>)>,
    type_map: BTreeMap<(Vec<ValType>, Vec<ValType>), u32>,
    /// Function index mapping: FuncId → wasm function index
    func_map: BTreeMap<FuncId, u32>,
    /// Reverse table map: wasm function index → table index
    func_to_table_idx: BTreeMap<u32, u32>,
    /// Import count (import functions come first in the index space)
    num_imports: u32,
    /// Runtime import indices
    rt: Option<RuntimeImports>,
    /// Global variable mapping: GlobalId → wasm global index
    global_map: BTreeMap<GlobalId, u32>,
    num_globals: u32,
    /// Module-level Let bindings promoted to WASM globals: (mod_idx, LocalId) → wasm global idx.
    /// Module-level `let`/`const` declarations live in module.init as Stmt::Let, but
    /// are accessed by functions in the same module via LocalGet. They need to be
    /// stored in WASM globals so cross-function references work, and so module-init
    /// LocalIds don't collide with other modules' identical LocalIds.
    module_let_globals: BTreeMap<(usize, LocalId), u32>,
    /// Current module index when compiling functions/methods, so LocalGet can resolve
    /// module-level Lets to the correct WASM global.
    current_mod_idx: usize,
    /// Class constructor map: class_name → wasm function index
    class_ctor_map: BTreeMap<String, u32>,
    /// Class method map: class_name → {method_name → wasm function index}
    class_method_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Class static method map: class_name → {method_name → wasm function index}
    class_static_map: BTreeMap<String, BTreeMap<String, u32>>,
    /// Function name → wasm function index (for cross-module ExternFuncRef resolution)
    func_name_map: BTreeMap<String, u32>,
    /// FFI imports: (name, param_count, has_return) — registered as WASM imports under "ffi" namespace
    ffi_imports: Vec<(String, usize, bool)>,
    /// Class parent map: child_class_name → parent_class_name
    class_parent_map: BTreeMap<String, String>,
    /// Enum member values: (enum_name, member_name) → numeric value or string
    enum_values: BTreeMap<(String, String), EnumResolvedValue>,
    /// Global index for NaN-safe temp storage (global.set/get may preserve NaN in Firefox)
    nan_temp_global: u32,
    /// Async function names (compiled to JS, not WASM)
    async_func_imports: Vec<(String, u32, usize)>, // (name, import_idx, param_count)
    /// Generated JS code for async functions
    async_js_code: Vec<String>,
    /// Per-module func_map snapshots: FuncRef(id) is only unique within a module,
    /// so each module needs its own FuncId→wasm_idx mapping.
    module_func_maps: Vec<BTreeMap<FuncId, u32>>,
    /// Set of WASM function indices that return void (no return value).
    /// Used to push TAG_UNDEFINED after calling void functions via FuncRef.
    void_funcs: std::collections::BTreeSet<u32>,
    /// WASM function index → expected parameter count.
    /// Used to pad missing arguments with TAG_UNDEFINED for optional params.
    func_param_counts: BTreeMap<u32, usize>,
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
        let mut next_import = || { let i = import_idx; import_idx += 1; i };

        // Additional type signatures needed for Phase 1+
        let t_f64_f64_void = self.get_type_idx(vec![ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_void = self.get_type_idx(vec![ValType::I64, ValType::I64, ValType::I64], vec![]);
        let t_f64_f64_f64_f64 = self.get_type_idx(vec![ValType::I64, ValType::I64, ValType::I64], vec![ValType::I64]);
        let t_f64_f64_f64_f64_f64 = self.get_type_idx(vec![ValType::I64, ValType::I64, ValType::I64, ValType::I64], vec![ValType::I64]);

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
            ("object_new", t_void_f64),                    // () -> handle
            ("object_set", t_f64_f64_f64_f64),              // (handle, key_str, value) -> handle (chaining)
            ("object_get", t_f64_f64_f64),                 // (handle, key_str) -> value
            ("object_get_dynamic", t_f64_f64_f64),         // (handle, key) -> value
            ("object_set_dynamic", t_f64_f64_f64_void),    // (handle, key, value) -> void
            ("object_delete", t_f64_f64_void),             // (handle, key_str) -> void
            ("object_delete_dynamic", t_f64_f64_void),     // (handle, key) -> void
            ("object_keys", t_f64_f64),                    // (handle) -> array_handle
            ("object_values", t_f64_f64),                  // (handle) -> array_handle
            ("object_entries", t_f64_f64),                 // (handle) -> array_handle
            ("object_has_property", t_f64_f64_i32),        // (handle, key) -> i32
            ("object_assign", t_f64_f64_f64),              // (target, source) -> target
            // Phase 1: Arrays
            ("array_new", t_void_f64),                     // () -> handle
            ("array_push", t_f64_f64_f64),                  // (handle, value) -> handle (chaining)
            ("array_pop", t_f64_f64),                      // (handle) -> value
            ("array_get", t_f64_f64_f64),                  // (handle, index) -> value
            ("array_set", t_f64_f64_f64_void),             // (handle, index, value) -> void
            ("array_length", t_f64_f64),                   // (handle) -> length
            ("array_slice", t_f64_f64_f64_f64),            // (handle, start, end) -> new_handle
            ("array_splice", t_f64_f64_f64_f64),           // (handle, start, deleteCount) -> removed_handle
            ("array_shift", t_f64_f64),                    // (handle) -> value
            ("array_unshift", t_f64_f64_void),             // (handle, value) -> void
            ("array_join", t_f64_f64_f64),                 // (handle, separator) -> string
            ("array_index_of", t_f64_f64_f64),             // (handle, value) -> index
            ("array_includes", t_f64_f64_i32),             // (handle, value) -> i32
            ("array_concat", t_f64_f64_f64),               // (handle1, handle2) -> new_handle
            ("array_reverse", t_f64_f64),                  // (handle) -> handle
            ("array_flat", t_f64_f64),                     // (handle) -> new_handle
            ("array_is_array", t_f64_i32),                 // (value) -> i32
            ("array_from", t_f64_f64),                     // (value) -> handle
            ("array_push_spread", t_f64_f64_f64),            // (target, source) -> handle (chaining)
            // Phase 1: Strings
            ("string_charAt", t_f64_f64_f64),              // (str, idx) -> str
            ("string_substring", t_f64_f64_f64_f64),       // (str, start, end) -> str
            ("string_indexOf", t_f64_f64_f64),             // (str, search) -> number
            ("string_slice", t_f64_f64_f64_f64),           // (str, start, end) -> str
            ("string_toLowerCase", t_f64_f64),
            ("string_toUpperCase", t_f64_f64),
            ("string_trim", t_f64_f64),
            ("string_includes", t_f64_f64_i32),
            ("string_startsWith", t_f64_f64_i32),
            ("string_endsWith", t_f64_f64_i32),
            ("string_replace", t_f64_f64_f64_f64),         // (str, pat, repl) -> str
            ("string_split", t_f64_f64_f64),               // (str, delim) -> array_handle
            ("string_fromCharCode", t_f64_f64),             // (code) -> str
            ("string_padStart", t_f64_f64_f64_f64),         // (str, len, fill) -> str
            ("string_padEnd", t_f64_f64_f64_f64),
            ("string_repeat", t_f64_f64_f64),               // (str, count) -> str
            ("string_match", t_f64_f64_f64),                // (str, regex) -> array_handle
            ("math_log2", t_f64_f64),
            ("math_log10", t_f64_f64),
            // Phase 2: Closures
            ("closure_new", t_f64_f64_f64),                // (func_table_idx, capture_count) -> handle
            ("closure_set_capture", t_f64_f64_f64_f64),     // (handle, idx, value) -> handle (chaining)
            ("closure_call_0", t_f64_f64),                 // (handle) -> result
            ("closure_call_1", t_f64_f64_f64),             // (handle, arg0) -> result
            ("closure_call_2", t_f64_f64_f64_f64),         // (handle, arg0, arg1) -> result
            ("closure_call_3", t_f64_f64_f64_f64_f64),     // (handle, arg0, arg1, arg2) -> result
            ("closure_call_spread", t_f64_f64_f64),        // (handle, args_array) -> result
            // Phase 2: Array higher-order
            ("array_map", t_f64_f64_f64),                  // (handle, closure) -> new_handle
            ("array_filter", t_f64_f64_f64),
            ("array_forEach", t_f64_f64_void),             // (handle, closure) -> void
            ("array_reduce", t_f64_f64_f64_f64),           // (handle, closure, initial) -> value
            ("array_find", t_f64_f64_f64),                 // (handle, closure) -> value
            ("array_find_index", t_f64_f64_f64),           // (handle, closure) -> number
            ("array_sort", t_f64_f64_f64),                 // (handle, closure) -> handle
            ("array_some", t_f64_f64_i32),                 // (handle, closure) -> i32
            ("array_every", t_f64_f64_i32),                // (handle, closure) -> i32
            // Phase 3: Classes
            ("class_new", t_f64_f64_f64),                  // (class_id, field_count) -> handle
            ("class_set_method", t_f64_f64_f64_void),      // (class_id, name_str, func_table_idx) -> void
            ("class_call_method", t_f64_f64_f64_f64),      // (handle, name_str, args_array) -> result
            ("class_get_field", t_f64_f64_f64),            // (handle, name_str) -> value
            ("class_set_field", t_f64_f64_f64_void),       // (handle, name_str, value) -> void
            ("class_set_static", t_f64_f64_f64_void),      // (class_id, name_str, value) -> void
            ("class_get_static", t_f64_f64_f64),           // (class_id, name_str) -> value
            ("class_instanceof", t_f64_f64_i32),           // (handle, class_id) -> i32
            // Phase 4: JSON
            ("json_parse", t_f64_f64),                     // (str) -> handle
            ("json_stringify", t_f64_f64),                 // (value) -> str
            // Phase 4: Map
            ("map_new", t_void_f64),
            ("map_set", t_f64_f64_f64_void),               // (handle, key, value) -> void
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
            ("date_new_val", t_f64_f64),                   // (opt_arg) -> handle
            ("date_get_time", t_f64_f64),
            ("date_to_iso_string", t_f64_f64),
            ("date_get_full_year", t_f64_f64),
            ("date_get_month", t_f64_f64),
            ("date_get_date", t_f64_f64),
            ("date_get_hours", t_f64_f64),
            ("date_get_minutes", t_f64_f64),
            ("date_get_seconds", t_f64_f64),
            ("date_get_milliseconds", t_f64_f64),
            // Phase 4: Error
            ("error_new", t_f64_f64),                      // (message) -> handle
            ("error_message", t_f64_f64),                  // (handle) -> string
            // Phase 4: RegExp
            ("regexp_new", t_f64_f64_f64),                 // (pattern, flags) -> handle
            ("regexp_test", t_f64_f64_i32),                // (regex, str) -> i32
            // Phase 4: Globals
            ("number_coerce", t_f64_f64),
            ("is_nan", t_f64_i32),
            ("is_finite", t_f64_i32),
            // Phase 5
            ("console_log_multi", t_f64_void),             // (args_array) -> void
            // Phase 1 addition: Class inheritance
            ("class_set_parent", t_f64_f64_void),          // (child_str, parent_str) -> void
            // Phase 3: Try/Catch
            ("try_start", t_void),                         // () -> void
            ("try_end", t_void),                           // () -> void
            ("throw_value", t_f64_void),                   // (val) -> void
            ("has_exception", t_void_i32),                 // () -> i32
            ("get_exception", t_void_f64),                 // () -> f64
            // Phase 4: URL
            ("url_parse", t_f64_f64),                      // (url_str) -> handle
            ("url_get_href", t_f64_f64),
            ("url_get_pathname", t_f64_f64),
            ("url_get_hostname", t_f64_f64),
            ("url_get_port", t_f64_f64),
            ("url_get_search", t_f64_f64),
            ("url_get_hash", t_f64_f64),
            ("url_get_origin", t_f64_f64),
            ("url_get_protocol", t_f64_f64),
            ("url_get_search_params", t_f64_f64),
            ("searchparams_get", t_f64_f64_f64),           // (handle, key) -> str
            ("searchparams_has", t_f64_f64_i32),           // (handle, key) -> i32
            ("searchparams_set", t_f64_f64_f64_void),      // (handle, key, val) -> void
            ("searchparams_append", t_f64_f64_f64_void),
            ("searchparams_delete", t_f64_f64_void),
            ("searchparams_to_string", t_f64_f64),
            // Phase 4: Crypto
            ("crypto_random_uuid", t_void_f64),
            ("crypto_random_bytes", t_f64_f64),
            // Phase 4: Path
            ("path_join", t_f64_f64_f64),                  // (a, b) -> str
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
            ("set_timeout", t_f64_f64_f64),                // (closure, delay) -> timer_id
            ("set_interval", t_f64_f64_f64),               // (closure, delay) -> timer_id
            ("clear_timeout", t_f64_void),                 // (id) -> void
            ("clear_interval", t_f64_void),                // (id) -> void
            // Response properties
            ("response_status", t_f64_f64),                // (handle) -> number
            ("response_ok", t_f64_i32),                    // (handle) -> i32
            ("response_headers_get", t_f64_f64_f64),       // (handle, name) -> str
            ("response_url", t_f64_f64),                   // (handle) -> str
            // Buffer extras
            ("buffer_copy", {
                let t = self.get_type_idx(vec![ValType::I64; 5], vec![ValType::I64]);
                t
            }),
            ("buffer_write", t_f64_f64_f64_f64),           // (handle, str, offset, encoding) -> number
            ("buffer_equals", t_f64_f64_i32),              // (handle, other) -> i32
            ("buffer_is_buffer", t_f64_i32),               // (val) -> i32
            ("buffer_byte_length", t_f64_f64),             // (val) -> number
            // Crypto extras
            ("crypto_sha256", t_f64_f64),                  // (data) -> promise_handle
            ("crypto_md5", t_f64_f64),                     // (data) -> undefined
            // Path extras
            ("path_is_absolute", t_f64_i32),               // (str) -> i32
            // Phase 5: Async/Promise/Fetch
            ("fetch_url", t_f64_f64),                      // (url_str) -> promise_handle
            ("fetch_with_options", t_f64_f64_f64_f64),     // (url, method, body, headers_obj) -> promise_handle
            ("response_json", t_f64_f64),                  // (response_handle) -> promise_handle
            ("response_text", t_f64_f64),                  // (response_handle) -> promise_handle
            ("promise_new", t_void_f64),                   // () -> promise_handle
            ("promise_resolve", t_f64_f64_void),           // (promise_handle, value) -> void
            ("promise_then", t_f64_f64_f64),               // (promise_handle, closure_handle) -> promise_handle
            ("await_promise", t_f64_f64),                  // (value) -> resolved_value_or_value
            // Memory-based bridge: args written to WASM memory at 0xFF00, only plain numbers as params
            ("mem_call", {
                self.get_type_idx(vec![ValType::F64, ValType::F64, ValType::I32], vec![ValType::F64])
            }),                                            // (func_name_id, arg_count, base_addr) -> f64 dummy
            ("mem_call_i32", {
                self.get_type_idx(vec![ValType::F64, ValType::F64, ValType::I32], vec![ValType::I32])
            }),                                            // (func_name_id, arg_count, base_addr) -> i32
        ];

        // Collect all closures from all modules (they need function indices too).
        // Track the module index so closures can be associated with their parent module's func_map.
        let mut closure_funcs: Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>, usize)> = Vec::new();
        for (mod_idx, (_, module)) in modules.iter().enumerate() {
            let mut module_closures: Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)> = Vec::new();
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
                    self.func_name_map.insert(func.name.clone(), async_import_idx);
                    self.async_func_imports.push((func.name.clone(), async_import_idx, param_count));
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
                let results = if has_return { vec![ValType::I64] } else { vec![] };
                let type_idx = self.get_type_idx(params, results);
                let _ = type_idx;
                self.func_name_map.insert(name.clone(), ffi_import_idx);
                self.ffi_imports.push((name.clone(), param_count, has_return));
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
                let results = if func.body.iter().any(|s| has_return(s)) || func.name == "main" {
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
                    self.class_parent_map.insert(class.name.clone(), parent.clone());
                }
                // Constructor: params = this + declared params, returns f64 (this)
                if let Some(ctor) = &class.constructor {
                    let param_count = 1 + ctor.params.len();
                    let params = vec![ValType::I64; param_count];
                    let results = vec![ValType::I64];
                    let type_idx = self.get_type_idx(params, results);
                    let _ = type_idx;
                    self.class_ctor_map.insert(class.name.clone(), user_func_idx);
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
                        .or_insert_with(BTreeMap::new)
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
                        .or_insert_with(BTreeMap::new)
                        .insert(method.name.clone(), user_func_idx);
                    // Also register in func_name_map for cross-module resolution
                    self.func_name_map.insert(format!("{}_{}", class.name, method.name), user_func_idx);
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
                        .or_insert_with(BTreeMap::new)
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
                        .or_insert_with(BTreeMap::new)
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
                let results = if body.iter().any(|s| has_return(s)) {
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

        // _start function (entry point)
        let start_idx = user_func_idx;
        let start_type = t_void;
        user_func_idx += 1;

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
            collect_module_let_ids(&module.init, mod_idx, &mut self.module_let_globals, &mut self.num_globals);
        }

        // Add a NaN-safe temp global for mem_store_slot (Firefox canonicalizes locals)
        self.nan_temp_global = self.num_globals;
        self.num_globals += 1;

        // Build the WASM module
        let mut wasm_module = Module::new();

        // --- Type section ---
        let mut type_section = TypeSection::new();
        for (params, results) in &self.types {
            type_section.ty().function(
                params.iter().copied(),
                results.iter().copied(),
            );
        }
        wasm_module.section(&type_section);

        // --- Import section ---
        let mut import_section = ImportSection::new();
        for (name, type_idx) in &import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add async function imports
        let async_import_entries: Vec<(String, u32)> = self.async_func_imports.iter().map(|(name, _idx, param_count)| {
            let import_name = format!("__async_{}", name);
            let params = vec![ValType::I64; *param_count];
            let results = vec![ValType::I64];
            let key = (params, results);
            let type_idx = self.type_map.get(&key).copied().unwrap_or(0);
            (import_name, type_idx)
        }).collect();
        for (name, type_idx) in &async_import_entries {
            import_section.import("rt", name, EntityType::Function(*type_idx));
        }
        // Add FFI function imports under "ffi" namespace
        for (name, param_count, has_return) in &self.ffi_imports {
            let params = vec![ValType::I64; *param_count];
            let results = if *has_return { vec![ValType::I64] } else { vec![] };
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
                if func.is_async { continue; }
                let param_count = func.params.len();
                let params = vec![ValType::I64; param_count];
                let results = if func.body.iter().any(|s| has_return(s)) || func.name == "main" {
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
            for (_, idx) in &self.class_ctor_map {
                if !indices.contains(idx) {
                    indices.push(*idx);
                }
            }
            for (_, methods) in &self.class_method_map {
                for (_, idx) in methods {
                    if !indices.contains(idx) {
                        indices.push(*idx);
                    }
                }
            }
            for (_, statics) in &self.class_static_map {
                for (_, idx) in statics {
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
        let pages = ((self.string_data.len() + 65535) / 65536).max(2) as u64; // min 2 pages for 0xFF00 mem_call region
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
                        GlobalType { val_type: ValType::I32, mutable: true, shared: false },
                        &wasm_encoder::ConstExpr::i32_const(0x10000),
                    );
                } else {
                    // Regular i64 global for module-level variables (NaN-boxed)
                    global_section.global(
                        GlobalType { val_type: ValType::I64, mutable: true, shared: false },
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
        wasm_module.section(&export_section);

        // --- Element section (populate the indirect call table) ---
        {
            let mut elem_section = ElementSection::new();
            elem_section.active(
                Some(0), // table index
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
                if hir_func.is_async { continue; }
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
            let mut per_module_init_locals: Vec<BTreeMap<LocalId, u32>> = Vec::with_capacity(modules.len());
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
                        let mut ctx = FuncEmitCtx::new(self, &init_locals, start_temp_local, start_temp_i32);
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
                    let class_name_id = self.string_map.get(class.name.as_str()).copied().unwrap_or(0);
                    let class_bits = (STRING_TAG << 48) | (class_name_id as u64);

                    // Register instance methods in classMethodTable (including getters/setters)
                    if let Some(methods) = self.class_method_map.get(&class.name) {
                        for (method_name, &func_idx) in methods {
                            let real_name = method_name.as_str();
                            let method_name_id = self.string_map.get(real_name).copied().unwrap_or(0);
                            let method_bits = (STRING_TAG << 48) | (method_name_id as u64);
                            let table_idx = self.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                            // Store args to memory for mem_call (Firefox NaN-safe: use I64Store)
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(method_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::I64Const((table_idx as f64).to_bits() as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            let csm_id = self.string_map.get("class_set_method").copied().unwrap_or(0);
                            func.instruction(&f64_const(csm_id as f64));
                            func.instruction(&f64_const(3.0));
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::Call(rt.mem_call));
                            func.instruction(&Instruction::Drop);
                        }
                    }

                    // Set up inheritance
                    if let Some(parent_name) = &class.extends_name {
                        let parent_name_id = self.string_map.get(parent_name.as_str()).copied().unwrap_or(0);
                        let parent_bits = (STRING_TAG << 48) | (parent_name_id as u64);
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::I64Const(class_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        func.instruction(&Instruction::I32Const(0xFF08));
                        func.instruction(&Instruction::I64Const(parent_bits as i64));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        let csp_id = self.string_map.get("class_set_parent").copied().unwrap_or(0);
                        func.instruction(&f64_const(csp_id as f64));
                        func.instruction(&f64_const(2.0));
                        func.instruction(&Instruction::I32Const(0xFF00));
                        func.instruction(&Instruction::Call(rt.mem_call));
                        func.instruction(&Instruction::Drop);
                    }

                    // Register static fields
                    for field in &class.static_fields {
                        if let Some(init) = &field.init {
                            let field_name_id = self.string_map.get(field.name.as_str()).copied().unwrap_or(0);
                            let field_bits = (STRING_TAG << 48) | (field_name_id as u64);
                            // Store class name
                            func.instruction(&Instruction::I32Const(0xFF00));
                            func.instruction(&Instruction::I64Const(class_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            // Store field name
                            func.instruction(&Instruction::I32Const(0xFF08));
                            func.instruction(&Instruction::I64Const(field_bits as i64));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            // Store value
                            let mut ctx = FuncEmitCtx::new(self, &init_locals, start_temp_local, start_temp_i32);
                            ctx.emit_expr(&mut func, init);
                            // Use temp local to store the value
                            func.instruction(&Instruction::LocalSet(start_temp_local));
                            func.instruction(&Instruction::I32Const(0xFF10));
                            func.instruction(&Instruction::LocalGet(start_temp_local));
                            // Value is already i64, no conversion needed
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            let css_id = self.string_map.get("class_set_static").copied().unwrap_or(0);
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
            data_section.active(0, &wasm_encoder::ConstExpr::i32_const(0), self.string_data.iter().copied());
            wasm_module.section(&data_section);
        }

        let wasm_bytes = wasm_module.finish();
        let async_js = self.async_js_code.join("\n");
        let ffi_import_names = self.ffi_imports.iter().map(|(name, _, _)| name.clone()).collect();
        WasmCompileOutput { wasm_bytes, async_js, ffi_imports: ffi_import_names }
    }

    fn compile_function(&self, hir_func: &perry_hir::ir::Function) -> Function {
        // Build local map: param locals come first, then body locals
        let mut local_map = BTreeMap::new();
        for (i, param) in hir_func.params.iter().enumerate() {
            local_map.insert(param.id, i as u32);
        }

        // Scan body for local variable declarations
        let param_count = hir_func.params.len() as u32;
        let mut extra_locals = 0u32;
        collect_locals(&hir_func.body, &mut local_map, &mut extra_locals, param_count);

        let temp_local_idx = param_count + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);

        let has_ret = hir_func.body.iter().any(|s| has_return(s));
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);

        for stmt in &hir_func.body {
            ctx.emit_stmt(&mut func, stmt, has_ret);
        }

        // If function should return but doesn't always, add a default return
        if has_ret {
            // Push undefined as default return
            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        }

        func.instruction(&Instruction::End);
        func
    }

    fn compile_closure(&self, params: &[Param], body: &[Stmt], captures: &[LocalId], mutable_captures: &[LocalId]) -> Function {
        // Closure parameters: captures first, then declared params
        let mut local_map = BTreeMap::new();
        let mut param_idx = 0u32;
        for cap in captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for cap in mutable_captures {
            local_map.insert(*cap, param_idx);
            param_idx += 1;
        }
        for param in params {
            local_map.insert(param.id, param_idx);
            param_idx += 1;
        }

        // Scan body for additional locals
        let mut extra_locals = 0u32;
        collect_locals(body, &mut local_map, &mut extra_locals, param_idx);

        let temp_local_idx = param_idx + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);

        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
        let has_ret = body.iter().any(|s| has_return(s));

        for stmt in body {
            ctx.emit_stmt(&mut func, stmt, true); // closures always "return"
        }

        // Default return undefined
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        func.instruction(&Instruction::End);
        func
    }

    fn compile_class_constructor(&self, class: &perry_hir::ir::Class, ctor: &perry_hir::ir::Function) -> Function {
        // Local 0 = this (the instance handle)
        // Params start at local index 1
        let mut local_map = BTreeMap::new();
        // Don't insert this into local_map — Expr::This emits LocalGet(0) directly
        for (i, param) in ctor.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + ctor.params.len();
        let mut extra_locals = 0u32;
        collect_locals(&ctor.body, &mut local_map, &mut extra_locals, param_count as u32);

        let temp_local_idx = param_count as u32 + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);
        let _rt = self.rt.as_ref().unwrap();

        // Emit field initializers: class_set_field(this, field_name, value) via mem_call
        for field in &class.fields {
            if let Some(init) = &field.init {
                let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
                ctx.emit_frame_begin(&mut func, 3);
                // Compute base address (sp - 24) and save to temp_i32 local
                let sp = self.nan_temp_global;
                func.instruction(&Instruction::GlobalGet(sp));
                func.instruction(&Instruction::I32Const(24));
                func.instruction(&Instruction::I32Sub);
                func.instruction(&Instruction::LocalSet(temp_i32_idx));
                // Store this handle to slot 0
                func.instruction(&Instruction::LocalGet(temp_i32_idx));
                func.instruction(&Instruction::LocalGet(0)); // this
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                // Store field name to slot 1
                let field_id = self.string_map.get(field.name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                func.instruction(&Instruction::LocalGet(temp_i32_idx));
                func.instruction(&Instruction::I32Const(8));
                func.instruction(&Instruction::I32Add);
                func.instruction(&Instruction::I64Const(field_bits as i64));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                // Store value to slot 2
                ctx.emit_store_arg(&mut func, 2, init);
                // Call via mem_call
                ctx.emit_memcall_void(&mut func, "class_set_field", 3);
            }
        }

        // Emit constructor body
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);
        ctx.current_class = Some(class.name.clone());
        for stmt in &ctor.body {
            ctx.emit_stmt(&mut func, stmt, false);
        }

        // Return this
        func.instruction(&Instruction::LocalGet(0));
        func.instruction(&Instruction::End);
        func
    }

    fn compile_class_method(&self, method: &perry_hir::ir::Function) -> Function {
        // Local 0 = this, params start at 1
        let mut local_map = BTreeMap::new();
        for (i, param) in method.params.iter().enumerate() {
            local_map.insert(param.id, (i + 1) as u32);
        }

        let param_count = 1 + method.params.len();
        let mut extra_locals = 0u32;
        collect_locals(&method.body, &mut local_map, &mut extra_locals, param_count as u32);

        let temp_local_idx = param_count as u32 + extra_locals;
        let temp_i32_idx = temp_local_idx + 2;
        let locals = vec![(extra_locals + 2, ValType::I64), (1, ValType::I32)];
        let mut func = Function::new(locals);
        let has_ret = method.body.iter().any(|s| has_return(s));
        let mut ctx = FuncEmitCtx::new(self, &local_map, temp_local_idx, temp_i32_idx);

        for stmt in &method.body {
            ctx.emit_stmt(&mut func, stmt, true); // methods always return f64
        }

        // Always push default return (method type is always -> f64)
        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        func.instruction(&Instruction::End);
        func
    }

    /// Generate JavaScript code for an async function body.
    /// The generated function uses NaN-boxed f64 values and bridge helper functions.
    fn emit_js_async_function(&self, func: &perry_hir::ir::Function) -> String {
        let params: Vec<String> = func.params.iter().enumerate()
            .map(|(i, _)| format!("__p{}", i))
            .collect();
        let params_str = params.join(", ");

        let mut body = String::new();
        // Map param names to local IDs for the JS emitter
        let mut local_names: BTreeMap<u32, String> = BTreeMap::new();
        for (i, param) in func.params.iter().enumerate() {
            local_names.insert(param.id, format!("__p{}", i));
        }

        for stmt in &func.body {
            self.emit_js_stmt(&mut body, stmt, &mut local_names, 2);
        }

        format!(
            "  __async_{name}: ({params}) => {{\n\
             \x20   const __p = (async () => {{\n\
             {body}\
             \x20     return {undef};\n\
             \x20   }})();\n\
             \x20   return nanboxPointer(allocHandle(__p));\n\
             \x20 }},",
            name = func.name,
            params = params_str,
            body = body,
            undef = "u64ToF64(TAG_UNDEFINED)",
        )
    }

    fn emit_js_stmt(&self, out: &mut String, stmt: &Stmt, locals: &mut BTreeMap<u32, String>, indent: usize) {
        let pad = "  ".repeat(indent);
        match stmt {
            Stmt::Let { id, init, .. } => {
                let name = format!("__l{}", id);
                locals.insert(*id, name.clone());
                if let Some(init_expr) = init {
                    let val = self.emit_js_expr(init_expr, locals);
                    out.push_str(&format!("{pad}    let {name} = {val};\n"));
                } else {
                    out.push_str(&format!("{pad}    let {name} = u64ToF64(TAG_UNDEFINED);\n"));
                }
            }
            Stmt::Expr(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    {val};\n"));
            }
            Stmt::Return(Some(e)) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    return {val};\n"));
            }
            Stmt::Return(None) => {
                out.push_str(&format!("{pad}    return u64ToF64(TAG_UNDEFINED);\n"));
            }
            Stmt::If { condition, then_branch, else_branch } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    if (toJsValue({cond})) {{\n"));
                for s in then_branch {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(eb) = else_branch {
                    out.push_str(&format!("{pad}    }} else {{\n"));
                    for s in eb {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::While { condition, body } => {
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::For { init, condition, update, body } => {
                out.push_str(&format!("{pad}    {{\n"));
                if let Some(init_stmt) = init {
                    self.emit_js_stmt(out, init_stmt, locals, indent + 1);
                }
                let cond = condition.as_ref().map(|c| self.emit_js_expr(c, locals)).unwrap_or_else(|| "1".to_string());
                out.push_str(&format!("{pad}      while (toJsValue({cond})) {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 2);
                }
                if let Some(upd) = update {
                    let u = self.emit_js_expr(upd, locals);
                    out.push_str(&format!("{pad}        {u};\n"));
                }
                out.push_str(&format!("{pad}      }}\n"));
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Try { body, catch, finally } => {
                out.push_str(&format!("{pad}    try {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                if let Some(c) = catch {
                    if let Some((param_id, _)) = &c.param {
                        let name = format!("__l{}", param_id);
                        locals.insert(*param_id, name.clone());
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                        out.push_str(&format!("{pad}      let {name} = fromJsValue(__e);\n"));
                    } else {
                        out.push_str(&format!("{pad}    }} catch (__e) {{\n"));
                    }
                    for s in &c.body {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                if let Some(f) = finally {
                    out.push_str(&format!("{pad}    }} finally {{\n"));
                    for s in f {
                        self.emit_js_stmt(out, s, locals, indent + 1);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Throw(e) => {
                let val = self.emit_js_expr(e, locals);
                out.push_str(&format!("{pad}    throw toJsValue({val});\n"));
            }
            Stmt::Break => { out.push_str(&format!("{pad}    break;\n")); }
            Stmt::Continue => { out.push_str(&format!("{pad}    continue;\n")); }
            Stmt::LabeledBreak(label) => { out.push_str(&format!("{pad}    break {};\n", label)); }
            Stmt::LabeledContinue(label) => { out.push_str(&format!("{pad}    continue {};\n", label)); }
            Stmt::DoWhile { body, condition } => {
                out.push_str(&format!("{pad}    do {{\n"));
                for s in body {
                    self.emit_js_stmt(out, s, locals, indent + 1);
                }
                let cond = self.emit_js_expr(condition, locals);
                out.push_str(&format!("{pad}    }} while (isTruthy({cond}));\n"));
            }
            Stmt::Labeled { label, body } => {
                out.push_str(&format!("{pad}    {}: {{\n", label));
                self.emit_js_stmt(out, body, locals, indent + 1);
                out.push_str(&format!("{pad}    }}\n"));
            }
            Stmt::Switch { discriminant, cases } => {
                let disc = self.emit_js_expr(discriminant, locals);
                out.push_str(&format!("{pad}    switch (toJsValue({disc})) {{\n"));
                for case in cases {
                    if let Some(test) = &case.test {
                        let t = self.emit_js_expr(test, locals);
                        out.push_str(&format!("{pad}      case toJsValue({t}):\n"));
                    } else {
                        out.push_str(&format!("{pad}      default:\n"));
                    }
                    for s in &case.body {
                        self.emit_js_stmt(out, s, locals, indent + 2);
                    }
                }
                out.push_str(&format!("{pad}    }}\n"));
            }
        }
    }

    fn emit_js_expr(&self, expr: &Expr, locals: &BTreeMap<u32, String>) -> String {
        match expr {
            Expr::Number(n) => format!("{}", n),
            Expr::Integer(i) => format!("{}", *i as f64),
            Expr::Bool(true) => "u64ToF64(TAG_TRUE)".to_string(),
            Expr::Bool(false) => "u64ToF64(TAG_FALSE)".to_string(),
            Expr::Undefined => "u64ToF64(TAG_UNDEFINED)".to_string(),
            Expr::Null => "u64ToF64(TAG_NULL)".to_string(),
            Expr::String(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r");
                format!("fromJsValue(\"{}\")", escaped)
            }
            Expr::LocalGet(id) => {
                locals.get(id).cloned().unwrap_or_else(|| format!("__l{}", id))
            }
            Expr::LocalSet(id, val) => {
                let name = locals.get(id).cloned().unwrap_or_else(|| format!("__l{}", id));
                let v = self.emit_js_expr(val, locals);
                format!("({} = {})", name, v)
            }
            Expr::GlobalGet(id) => format!("__g{}", id),
            Expr::GlobalSet(id, val) => {
                let v = self.emit_js_expr(val, locals);
                format!("(__g{} = {})", id, v)
            }
            Expr::Binary { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                match op {
                    BinaryOp::Add => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                    BinaryOp::Sub => format!("({} - {})", l, r),
                    BinaryOp::Mul => format!("({} * {})", l, r),
                    BinaryOp::Div => format!("({} / {})", l, r),
                    BinaryOp::Mod => format!("({} % {})", l, r),
                    _ => format!("fromJsValue(toJsValue({}) + toJsValue({}))", l, r),
                }
            }
            Expr::Compare { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                let js_op = match op {
                    CompareOp::Eq => "===",
                    CompareOp::Ne => "!==",
                    CompareOp::LooseEq => "==",
                    CompareOp::LooseNe => "!=",
                    CompareOp::Lt => "<",
                    CompareOp::Le => "<=",
                    CompareOp::Gt => ">",
                    CompareOp::Ge => ">=",
                };
                format!("(toJsValue({}) {} toJsValue({}) ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))", l, js_op, r)
            }
            Expr::Logical { op, left, right } => {
                let l = self.emit_js_expr(left, locals);
                let r = self.emit_js_expr(right, locals);
                match op {
                    LogicalOp::And => format!("(toJsValue({l}) ? {r} : {l})"),
                    LogicalOp::Or => format!("(toJsValue({l}) ? {l} : {r})"),
                    LogicalOp::Coalesce => format!("(isNull({l}) || isUndefined({l}) ? {r} : {l})"),
                }
            }
            Expr::Unary { op, operand } => {
                let o = self.emit_js_expr(operand, locals);
                match op {
                    UnaryOp::Neg => format!("(-{})", o),
                    UnaryOp::Not => format!("(toJsValue({}) ? u64ToF64(TAG_FALSE) : u64ToF64(TAG_TRUE))", o),
                    _ => o,
                }
            }
            Expr::Await(inner) => {
                let v = self.emit_js_expr(inner, locals);
                // In JS async context, we can truly await
                format!("fromJsValue(await toJsValue({}))", v)
            }
            Expr::Call { callee, args, .. } => {
                let args_js: Vec<String> = args.iter().map(|a| self.emit_js_expr(a, locals)).collect();
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&func_idx) = self.func_map.get(id) {
                            // Call exported WASM function. WASM funcs use i64 params/result.
                            let args_i64: Vec<String> = args_js.iter()
                                .map(|a| format!("f64ToU64({})", a))
                                .collect();
                            format!("u64ToF64(wasmInstance.exports.__wasm_func_{}({}))", func_idx, args_i64.join(", "))
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::ExternFuncRef { name, .. } => {
                        if let Some(&func_idx) = self.func_name_map.get(name) {
                            let args_i64: Vec<String> = args_js.iter()
                                .map(|a| format!("f64ToU64({})", a))
                                .collect();
                            format!("u64ToF64(wasmInstance.exports.__wasm_func_{}({}))", func_idx, args_i64.join(", "))
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                    Expr::PropertyGet { object, property } => {
                        let obj = self.emit_js_expr(object, locals);
                        let args_str = args_js.join(", ");
                        format!("fromJsValue(toJsValue({}).{}({}))", obj, property,
                            args.iter().map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals))).collect::<Vec<_>>().join(", "))
                    }
                    _ => {
                        let callee_js = self.emit_js_expr(callee, locals);
                        format!("{}({})", callee_js, args_js.join(", "))
                    }
                }
            }
            Expr::FetchWithOptions { url, method, body, headers } => {
                let url_js = self.emit_js_expr(url, locals);
                let method_js = self.emit_js_expr(method, locals);
                let body_js = self.emit_js_expr(body, locals);
                // In async JS context, we can do a real fetch
                let mut opts = format!("{{ method: getString({}) || 'GET'", method_js);
                if !matches!(body.as_ref(), Expr::Undefined) {
                    opts.push_str(&format!(", body: getString({})", body_js));
                }
                if !headers.is_empty() {
                    opts.push_str(", headers: {");
                    for (i, (key, val)) in headers.iter().enumerate() {
                        if i > 0 { opts.push_str(", "); }
                        let v = self.emit_js_expr(val, locals);
                        opts.push_str(&format!("'{}': getString({})", key, v));
                    }
                    opts.push('}');
                }
                opts.push('}');
                format!("fromJsValue(await fetch(getString({}), {}))", url_js, opts)
            }
            Expr::PropertyGet { object, property } => {
                let obj = self.emit_js_expr(object, locals);
                format!("fromJsValue(toJsValue({}).{})", obj, property)
            }
            Expr::PropertySet { object, property, value } => {
                let obj = self.emit_js_expr(object, locals);
                let val = self.emit_js_expr(value, locals);
                format!("(toJsValue({}).{} = toJsValue({}), {})", obj, property, val, val)
            }
            Expr::Object(fields) => {
                let mut parts = Vec::new();
                for (key, val) in fields {
                    let v = self.emit_js_expr(val, locals);
                    parts.push(format!("'{}': toJsValue({})", key, v));
                }
                format!("fromJsValue({{{}}})", parts.join(", "))
            }
            Expr::Array(elements) => {
                let elems: Vec<String> = elements.iter()
                    .map(|e| format!("toJsValue({})", self.emit_js_expr(e, locals)))
                    .collect();
                format!("fromJsValue([{}])", elems.join(", "))
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                let c = self.emit_js_expr(condition, locals);
                let t = self.emit_js_expr(then_expr, locals);
                let e = self.emit_js_expr(else_expr, locals);
                format!("(toJsValue({}) ? {} : {})", c, t, e)
            }
            Expr::NativeMethodCall { module, method, object, args, class_name } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "console" => {
                        let args_js: Vec<String> = args.iter()
                            .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                            .collect();
                        match method.as_str() {
                            "log" => format!("(console.log({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            "warn" => format!("(console.warn({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            "error" => format!("(console.error({}), u64ToF64(TAG_UNDEFINED))", args_js.join(", ")),
                            _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                        }
                    }
                    "JSON" => {
                        match method.as_str() {
                            "parse" if !args.is_empty() => {
                                let a = self.emit_js_expr(&args[0], locals);
                                format!("fromJsValue(JSON.parse(getString({})))", a)
                            }
                            "stringify" if !args.is_empty() => {
                                let a = self.emit_js_expr(&args[0], locals);
                                format!("fromJsValue(JSON.stringify(toJsValue({})))", a)
                            }
                            _ => "u64ToF64(TAG_UNDEFINED)".to_string(),
                        }
                    }
                    "perry/ui" | "perry/system" => {
                        let bridge_name = map_ui_method(method, class_name.as_deref());
                        let args_js: Vec<String> = args.iter()
                            .map(|a| self.emit_js_expr(a, locals))
                            .collect();
                        if let Some(obj) = object {
                            let obj_js = self.emit_js_expr(obj, locals);
                            let mut all_args = vec![obj_js];
                            all_args.extend(args_js);
                            format!("__perryUi.{}({})", bridge_name, all_args.join(", "))
                        } else {
                            format!("__perryUi.{}({})", bridge_name, args_js.join(", "))
                        }
                    }
                    _ => {
                        if let Some(obj) = object {
                            let obj_js = self.emit_js_expr(obj, locals);
                            let args_js: Vec<String> = args.iter()
                                .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                                .collect();
                            format!("fromJsValue(toJsValue({}).{}({}))", obj_js, method, args_js.join(", "))
                        } else {
                            "u64ToF64(TAG_UNDEFINED)".to_string()
                        }
                    }
                }
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    let m_js = self.emit_js_expr(m, locals);
                    format!("fromJsValue(new Error(getString({})))", m_js)
                } else {
                    "fromJsValue(new Error())".to_string()
                }
            }
            Expr::ErrorMessage(err) => {
                let e = self.emit_js_expr(err, locals);
                format!("fromJsValue(toJsValue({}).message)", e)
            }
            Expr::ErrorNewWithCause { message, cause } => {
                let m_js = self.emit_js_expr(message, locals);
                let c_js = self.emit_js_expr(cause, locals);
                format!("fromJsValue(new Error(getString({}), {{ cause: toJsValue({}) }}))", m_js, c_js)
            }
            Expr::TypeErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new TypeError(getString({})))", m_js)
            }
            Expr::RangeErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new RangeError(getString({})))", m_js)
            }
            Expr::ReferenceErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new ReferenceError(getString({})))", m_js)
            }
            Expr::SyntaxErrorNew(m) => {
                let m_js = self.emit_js_expr(m, locals);
                format!("fromJsValue(new SyntaxError(getString({})))", m_js)
            }
            Expr::AggregateErrorNew { errors, message } => {
                let e_js = self.emit_js_expr(errors, locals);
                let m_js = self.emit_js_expr(message, locals);
                format!("fromJsValue(new AggregateError(toJsValue({}), getString({})))", e_js, m_js)
            }
            Expr::JsonParse(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.parse(getString({})))", v)
            }
            Expr::JsonStringify(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(JSON.stringify(toJsValue({})))", v)
            }
            Expr::This => "__this".to_string(),
            Expr::IndexGet { object, index } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                format!("fromJsValue(toJsValue({})[toJsValue({})])", obj, idx)
            }
            Expr::IndexSet { object, index, value } => {
                let obj = self.emit_js_expr(object, locals);
                let idx = self.emit_js_expr(index, locals);
                let val = self.emit_js_expr(value, locals);
                format!("(toJsValue({})[toJsValue({})] = toJsValue({}), {})", obj, idx, val, val)
            }
            Expr::ArrayPush { array_id, value } => {
                let arr = locals.get(array_id).cloned().unwrap_or_else(|| format!("__l{}", array_id));
                let val = self.emit_js_expr(value, locals);
                format!("fromJsValue(toJsValue({}).push(toJsValue({})))", arr, val)
            }
            Expr::StringCoerce(val) => {
                let v = self.emit_js_expr(val, locals);
                format!("fromJsValue(String(toJsValue({})))", v)
            }
            Expr::MathFloor(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.floor({})", v)
            }
            Expr::MathCeil(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.ceil({})", v)
            }
            Expr::MathRound(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.round({})", v)
            }
            Expr::MathAbs(x) => {
                let v = self.emit_js_expr(x, locals);
                format!("Math.abs({})", v)
            }
            Expr::MathRandom => "Math.random()".to_string(),
            Expr::DateNow => "Date.now()".to_string(),
            Expr::Sequence(exprs) => {
                if exprs.is_empty() {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                } else {
                    let parts: Vec<String> = exprs.iter().map(|e| self.emit_js_expr(e, locals)).collect();
                    format!("({})", parts.join(", "))
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // In JS context, look up exported WASM functions
                if let Some(&func_idx) = self.func_name_map.get(name) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.func_map.get(id) {
                    format!("fromJsValue(wasmInstance.exports.__wasm_func_{})", func_idx)
                } else {
                    "u64ToF64(TAG_UNDEFINED)".to_string()
                }
            }
            Expr::New { class_name, args, .. } => {
                let args_js: Vec<String> = args.iter()
                    .map(|a| format!("toJsValue({})", self.emit_js_expr(a, locals)))
                    .collect();
                format!("fromJsValue(new (toJsValue(fromJsValue('{}')))({}))", class_name, args_js.join(", "))
            }
            Expr::InstanceOf { expr, ty } => {
                let e = self.emit_js_expr(expr, locals);
                // Use the bridge instanceof check
                format!("(toJsValue({}) instanceof {} ? u64ToF64(TAG_TRUE) : u64ToF64(TAG_FALSE))", e, ty)
            }
            Expr::TypeOf(operand) => {
                let o = self.emit_js_expr(operand, locals);
                format!("fromJsValue(typeof toJsValue({}))", o)
            }
            Expr::Void(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("({}, u64ToF64(TAG_UNDEFINED))", v)
            }
            Expr::Delete(e) => {
                let v = self.emit_js_expr(e, locals);
                format!("(delete toJsValue({}), u64ToF64(TAG_TRUE))", v)
            }
            _ => {
                // Fallback: return undefined for unhandled expressions
                "u64ToF64(TAG_UNDEFINED)".to_string()
            }
        }
    }

    fn collect_strings(&mut self, module: &perry_hir::ir::Module) {
        // Pre-intern common strings used by bridge calls
        self.intern_string("Authorization");
        self.intern_string("POST");
        self.intern_string("GET");
        self.intern_string("");

        // Pre-intern ALL bridge function names for mem_call dispatch
        let bridge_names = [
            "console_log", "console_warn", "console_error",
            "string_concat", "js_add", "string_eq", "string_len",
            "jsvalue_to_string", "is_truthy", "js_strict_eq",
            "math_floor", "math_ceil", "math_round", "math_abs", "math_sqrt",
            "math_pow", "math_random", "math_log", "date_now",
            "js_typeof", "math_min", "math_max", "parse_int", "parse_float",
            "js_mod", "is_null_or_undefined",
            "object_new", "object_set", "object_get", "object_get_dynamic",
            "object_set_dynamic", "object_delete", "object_delete_dynamic",
            "object_keys", "object_values", "object_entries",
            "object_has_property", "object_assign",
            "array_new", "array_push", "array_pop", "array_get", "array_set",
            "array_length", "array_slice", "array_splice", "array_shift",
            "array_unshift", "array_join", "array_index_of", "array_includes",
            "array_concat", "array_reverse", "array_flat", "array_is_array",
            "array_from", "array_push_spread",
            "string_charAt", "string_substring", "string_indexOf", "string_slice",
            "string_toLowerCase", "string_toUpperCase", "string_trim",
            "string_includes", "string_startsWith", "string_endsWith",
            "string_replace", "string_split", "string_fromCharCode",
            "string_padStart", "string_padEnd", "string_repeat", "string_match",
            "math_log2", "math_log10",
            // Issue #133 item 4: trig / exp / sign / trunc / cbrt / hypot etc.
            "math_sin", "math_cos", "math_tan", "math_asin", "math_acos", "math_atan",
            "math_atan2", "math_sinh", "math_cosh", "math_tanh",
            "math_asinh", "math_acosh", "math_atanh",
            "math_exp", "math_expm1", "math_log1p", "math_sign", "math_trunc",
            "math_cbrt", "math_hypot", "math_fround", "math_clz32",
            "closure_new", "closure_set_capture",
            "closure_call_0", "closure_call_1", "closure_call_2", "closure_call_3",
            "closure_call_spread",
            "array_map", "array_filter", "array_forEach", "array_reduce",
            "array_find", "array_find_index", "array_sort", "array_some", "array_every",
            "class_new", "class_set_method", "class_call_method",
            "class_get_field", "class_set_field",
            "class_set_static", "class_get_static", "class_instanceof",
            "json_parse", "json_stringify",
            "map_new", "map_set", "map_get", "map_has", "map_delete",
            "map_size", "map_clear", "map_entries", "map_keys", "map_values",
            "set_new", "set_new_from_array", "set_add", "set_has", "set_delete",
            "set_size", "set_clear", "set_values",
            "date_new_val", "date_get_time", "date_to_iso_string",
            "date_get_full_year", "date_get_month", "date_get_date",
            "date_get_hours", "date_get_minutes", "date_get_seconds", "date_get_milliseconds",
            "error_new", "error_message",
            "regexp_new", "regexp_test",
            "number_coerce", "is_nan", "is_finite",
            "console_log_multi",
            "class_set_parent",
            "try_start", "try_end", "throw_value", "has_exception", "get_exception",
            "url_parse", "url_get_href", "url_get_pathname", "url_get_hostname",
            "url_get_port", "url_get_search", "url_get_hash", "url_get_origin",
            "url_get_protocol", "url_get_search_params",
            "searchparams_get", "searchparams_has", "searchparams_set",
            "searchparams_append", "searchparams_delete", "searchparams_to_string",
            "crypto_random_uuid", "crypto_random_bytes",
            "path_join", "path_dirname", "path_basename", "path_extname",
            "path_resolve", "path_is_absolute",
            "os_platform", "process_argv", "process_cwd",
            "buffer_alloc", "buffer_from_string", "buffer_to_string",
            "buffer_get", "buffer_set", "buffer_length", "buffer_slice", "buffer_concat",
            "uint8array_new", "uint8array_from", "uint8array_length",
            "uint8array_get", "uint8array_set",
            "set_timeout", "set_interval", "clear_timeout", "clear_interval",
            "response_status", "response_ok", "response_headers_get", "response_url",
            "buffer_copy", "buffer_write", "buffer_equals", "buffer_is_buffer",
            "buffer_byte_length",
            "crypto_sha256", "crypto_md5",
            "fetch_url", "fetch_with_options", "response_json", "response_text",
            "promise_new", "promise_resolve", "promise_then", "await_promise",
        ];
        for name in &bridge_names {
            self.intern_string(name);
        }

        for func in &module.functions {
            self.collect_strings_in_stmts(&func.body);
        }
        for stmt in &module.init {
            self.collect_strings_in_stmt(stmt);
        }
        for global in &module.globals {
            if let Some(init) = &global.init {
                self.collect_strings_in_expr(init);
            }
        }
        // Collect enum names and member names/values
        for enum_def in &module.enums {
            self.intern_string(&enum_def.name);
            for member in &enum_def.members {
                self.intern_string(&member.name);
                match &member.value {
                    EnumValue::String(s) => {
                        self.intern_string(s);
                        self.enum_values.insert(
                            (enum_def.name.clone(), member.name.clone()),
                            EnumResolvedValue::String(s.clone()),
                        );
                    }
                    EnumValue::Number(n) => {
                        self.enum_values.insert(
                            (enum_def.name.clone(), member.name.clone()),
                            EnumResolvedValue::Number(*n as f64),
                        );
                    }
                }
            }
        }
        // Collect class names and method/field names
        for class in &module.classes {
            self.intern_string(&class.name);
            if let Some(parent) = &class.extends_name {
                self.intern_string(parent);
            }
            if let Some(ctor) = &class.constructor {
                self.collect_strings_in_stmts(&ctor.body);
                for param in &ctor.params {
                    if let Some(default) = &param.default {
                        self.collect_strings_in_expr(default);
                    }
                }
            }
            for method in &class.methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for method in &class.static_methods {
                self.intern_string(&method.name);
                self.collect_strings_in_stmts(&method.body);
            }
            for (name, getter) in &class.getters {
                self.intern_string(name);
                self.intern_string(&format!("__get_{}", name));
                self.collect_strings_in_stmts(&getter.body);
            }
            for (name, setter) in &class.setters {
                self.intern_string(name);
                self.intern_string(&format!("__set_{}", name));
                self.collect_strings_in_stmts(&setter.body);
            }
            for field in &class.fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
            for field in &class.static_fields {
                self.intern_string(&field.name);
                if let Some(init) = &field.init {
                    self.collect_strings_in_expr(init);
                }
            }
        }
    }

    fn collect_strings_in_stmts(&mut self, stmts: &[Stmt]) {
        for stmt in stmts {
            self.collect_strings_in_stmt(stmt);
        }
    }

    fn collect_strings_in_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init { self.collect_strings_in_expr(e); }
            }
            Stmt::Expr(e) => self.collect_strings_in_expr(e),
            Stmt::Return(e) => {
                if let Some(e) = e { self.collect_strings_in_expr(e); }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(then_branch);
                if let Some(eb) = else_branch { self.collect_strings_in_stmts(eb); }
            }
            Stmt::While { condition, body } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_stmts(body);
            }
            Stmt::DoWhile { body, condition } => {
                self.collect_strings_in_stmts(body);
                self.collect_strings_in_expr(condition);
            }
            Stmt::Labeled { body, .. } => {
                self.collect_strings_in_stmt(body);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(i) = init { self.collect_strings_in_stmt(i); }
                if let Some(c) = condition { self.collect_strings_in_expr(c); }
                if let Some(u) = update { self.collect_strings_in_expr(u); }
                self.collect_strings_in_stmts(body);
            }
            Stmt::Throw(e) => self.collect_strings_in_expr(e),
            Stmt::Try { body, catch, finally } => {
                self.collect_strings_in_stmts(body);
                if let Some(c) = catch {
                    self.collect_strings_in_stmts(&c.body);
                }
                if let Some(f) = finally { self.collect_strings_in_stmts(f); }
            }
            Stmt::Switch { discriminant, cases } => {
                self.collect_strings_in_expr(discriminant);
                for case in cases {
                    if let Some(t) = &case.test { self.collect_strings_in_expr(t); }
                    self.collect_strings_in_stmts(&case.body);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::LabeledBreak(_) | Stmt::LabeledContinue(_) => {}
        }
    }

    fn collect_strings_in_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::String(s) => { self.intern_string(s); }
            Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. }
            | Expr::Logical { left, right, .. } => {
                self.collect_strings_in_expr(left);
                self.collect_strings_in_expr(right);
            }
            Expr::Unary { operand, .. } => self.collect_strings_in_expr(operand),
            Expr::Call { callee, args, .. } => {
                self.collect_strings_in_expr(callee);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
                self.collect_strings_in_expr(val);
            }
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.collect_strings_in_expr(condition);
                self.collect_strings_in_expr(then_expr);
                self.collect_strings_in_expr(else_expr);
            }
            Expr::Closure { body, .. } => {
                self.collect_strings_in_stmts(body);
            }
            Expr::NativeMethodCall { module, method, args, class_name, object } => {
                for a in args { self.collect_strings_in_expr(a); }
                if let Some(obj) = object { self.collect_strings_in_expr(obj); }
                // Pre-intern bridge name for UI calls
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                if normalized == "perry/ui" || normalized == "perry/system" {
                    let bridge_name = map_ui_method(method, class_name.as_deref());
                    self.intern_string(bridge_name);
                }
                if normalized == "perry/thread" {
                    match method.as_str() {
                        "parallelMap" => { self.intern_string("thread_parallel_map"); }
                        "parallelFilter" => { self.intern_string("thread_parallel_filter"); }
                        "spawn" => { self.intern_string("thread_spawn"); }
                        _ => {}
                    }
                }
            }
            Expr::Array(elems) => {
                for e in elems { self.collect_strings_in_expr(e); }
            }
            Expr::Object(fields) => {
                for (k, v) in fields {
                    self.intern_string(k);
                    self.collect_strings_in_expr(v);
                }
            }
            Expr::PropertyGet { object, property } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::PropertySet { object, value, property, .. } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(value);
                self.intern_string(property);
            }
            Expr::IndexGet { object, index } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::IndexSet { object, index, value } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
                self.collect_strings_in_expr(value);
            }
            Expr::Await(e) | Expr::TypeOf(e) | Expr::Void(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::New { args, .. } => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::Update { .. } => {}
            Expr::Sequence(exprs) => {
                for e in exprs { self.collect_strings_in_expr(e); }
            }
            Expr::EnumMember { enum_name, member_name } => {
                self.intern_string(enum_name);
                self.intern_string(member_name);
            }
            Expr::StaticFieldGet { class_name, field_name } |
            Expr::StaticFieldSet { class_name, field_name, .. } => {
                self.intern_string(class_name);
                self.intern_string(field_name);
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                self.intern_string(class_name);
                self.intern_string(method_name);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::InstanceOf { expr, ty } => {
                self.collect_strings_in_expr(expr);
                self.intern_string(ty);
            }
            Expr::In { property, object } => {
                self.collect_strings_in_expr(property);
                self.collect_strings_in_expr(object);
            }
            Expr::Delete(e) => self.collect_strings_in_expr(e),
            Expr::RegExp { pattern, flags } => {
                self.intern_string(pattern);
                self.intern_string(flags);
            }
            Expr::RegExpTest { regex, string } => {
                self.collect_strings_in_expr(regex);
                self.collect_strings_in_expr(string);
            }
            Expr::StringMatch { string, regex } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(regex);
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(pattern);
                self.collect_strings_in_expr(replacement);
            }
            Expr::StringSplit(a, b) => {
                self.collect_strings_in_expr(a);
                self.collect_strings_in_expr(b);
            }
            Expr::StringFromCharCode(e) | Expr::StringFromCodePoint(e) | Expr::StringCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::StringAt { string, index } | Expr::StringCodePointAt { string, index } => {
                self.collect_strings_in_expr(string);
                self.collect_strings_in_expr(index);
            }
            Expr::ObjectSpread { parts } => {
                for (key_opt, val) in parts {
                    if let Some(k) = key_opt { self.intern_string(k); }
                    self.collect_strings_in_expr(val);
                }
            }
            Expr::ArraySpread(elements) => {
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) | ArrayElement::Spread(e) => {
                            self.collect_strings_in_expr(e);
                        }
                    }
                }
            }
            Expr::ObjectKeys(e) | Expr::ObjectValues(e) | Expr::ObjectEntries(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ObjectRest { object, exclude_keys } => {
                self.collect_strings_in_expr(object);
                for k in exclude_keys { self.intern_string(k); }
            }
            Expr::ArrayPush { value, .. } | Expr::ArrayUnshift { value, .. } => {
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayPushSpread { source, .. } => {
                self.collect_strings_in_expr(source);
            }
            Expr::ArraySlice { array, start, end } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(start);
                if let Some(e) = end { self.collect_strings_in_expr(e); }
            }
            Expr::ArraySplice { start, delete_count, items, .. } => {
                self.collect_strings_in_expr(start);
                if let Some(dc) = delete_count { self.collect_strings_in_expr(dc); }
                for item in items { self.collect_strings_in_expr(item); }
            }
            Expr::ArrayJoin { array, separator } => {
                self.collect_strings_in_expr(array);
                if let Some(s) = separator { self.collect_strings_in_expr(s); }
                self.intern_string(","); // default separator
            }
            Expr::ArrayIndexOf { array, value } | Expr::ArrayIncludes { array, value } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } |
            Expr::ArrayForEach { array, callback } | Expr::ArrayFind { array, callback } |
            Expr::ArrayFindIndex { array, callback } | Expr::ArraySort { array, comparator: callback } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
            }
            Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(callback);
                if let Some(i) = initial { self.collect_strings_in_expr(i); }
            }
            Expr::ArrayToSorted { array, comparator } => {
                self.collect_strings_in_expr(array);
                if let Some(c) = comparator { self.collect_strings_in_expr(c); }
            }
            Expr::ArrayToSpliced { array, start, delete_count, items } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(start);
                self.collect_strings_in_expr(delete_count);
                for item in items { self.collect_strings_in_expr(item); }
            }
            Expr::ArrayWith { array, index, value } => {
                self.collect_strings_in_expr(array);
                self.collect_strings_in_expr(index);
                self.collect_strings_in_expr(value);
            }
            Expr::ArrayCopyWithin { target, start, end, .. } => {
                self.collect_strings_in_expr(target);
                self.collect_strings_in_expr(start);
                if let Some(e) = end { self.collect_strings_in_expr(e); }
            }
            Expr::ArrayFlat { array } | Expr::ArrayIsArray(array) | Expr::ArrayFrom(array) | Expr::ArrayToReversed { array } => {
                self.collect_strings_in_expr(array);
            }
            Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
                self.collect_strings_in_expr(array);
            }
            Expr::ArrayFromMapped { iterable, map_fn } => {
                self.collect_strings_in_expr(iterable);
                self.collect_strings_in_expr(map_fn);
            }
            Expr::MapSet { map, key, value } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
                self.collect_strings_in_expr(value);
            }
            Expr::MapGet { map, key } | Expr::MapHas { map, key } | Expr::MapDelete { map, key } => {
                self.collect_strings_in_expr(map);
                self.collect_strings_in_expr(key);
            }
            Expr::MapSize(e) | Expr::MapClear(e) | Expr::MapEntries(e) |
            Expr::MapKeys(e) | Expr::MapValues(e) | Expr::MapNewFromArray(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetNewFromArray(e) | Expr::SetSize(e) | Expr::SetClear(e) | Expr::SetValues(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::SetAdd { value, .. } => { self.collect_strings_in_expr(value); }
            Expr::SetHas { set, value } | Expr::SetDelete { set, value } => {
                self.collect_strings_in_expr(set);
                self.collect_strings_in_expr(value);
            }
            Expr::DateNew(arg) => {
                if let Some(a) = arg { self.collect_strings_in_expr(a); }
            }
            Expr::DateGetTime(e) | Expr::DateToISOString(e) | Expr::DateGetFullYear(e) |
            Expr::DateGetMonth(e) | Expr::DateGetDate(e) | Expr::DateGetHours(e) |
            Expr::DateGetMinutes(e) | Expr::DateGetSeconds(e) | Expr::DateGetMilliseconds(e) |
            Expr::DateGetUtcDay(e) | Expr::DateGetUtcFullYear(e) | Expr::DateGetUtcMonth(e) |
            Expr::DateGetUtcDate(e) | Expr::DateGetUtcHours(e) | Expr::DateGetUtcMinutes(e) |
            Expr::DateGetUtcSeconds(e) | Expr::DateGetUtcMilliseconds(e) |
            Expr::DateValueOf(e) | Expr::DateToDateString(e) | Expr::DateToTimeString(e) |
            Expr::DateToLocaleDateString(e) | Expr::DateToLocaleTimeString(e) |
            Expr::DateToLocaleString(e) | Expr::DateGetTimezoneOffset(e) | Expr::DateToJSON(e) |
            Expr::DateParse(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::DateUtc(args) => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::DateSetUtcFullYear { date, value } | Expr::DateSetUtcMonth { date, value } |
            Expr::DateSetUtcDate { date, value } | Expr::DateSetUtcHours { date, value } |
            Expr::DateSetUtcMinutes { date, value } | Expr::DateSetUtcSeconds { date, value } |
            Expr::DateSetUtcMilliseconds { date, value } => {
                self.collect_strings_in_expr(date);
                self.collect_strings_in_expr(value);
            }
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg { self.collect_strings_in_expr(m); }
            }
            Expr::ErrorMessage(e) => { self.collect_strings_in_expr(e); }
            Expr::ErrorNewWithCause { message, cause } => {
                self.collect_strings_in_expr(message);
                self.collect_strings_in_expr(cause);
            }
            Expr::TypeErrorNew(m) | Expr::RangeErrorNew(m) | Expr::ReferenceErrorNew(m) | Expr::SyntaxErrorNew(m) => {
                self.collect_strings_in_expr(m);
            }
            Expr::AggregateErrorNew { errors, message } => {
                self.collect_strings_in_expr(errors);
                self.collect_strings_in_expr(message);
            }
            Expr::JsonParse(e) | Expr::JsonStringify(e) => { self.collect_strings_in_expr(e); }
            Expr::NumberCoerce(e) | Expr::IsNaN(e) | Expr::IsUndefinedOrBareNan(e) | Expr::IsFinite(e) | Expr::BigIntCoerce(e) => {
                self.collect_strings_in_expr(e);
            }
            Expr::ParseInt { string, radix } => {
                self.collect_strings_in_expr(string);
                if let Some(r) = radix { self.collect_strings_in_expr(r); }
            }
            Expr::ParseFloat(e) => { self.collect_strings_in_expr(e); }
            Expr::PropertyUpdate { object, property, .. } => {
                self.collect_strings_in_expr(object);
                self.intern_string(property);
            }
            Expr::IndexUpdate { object, index, .. } => {
                self.collect_strings_in_expr(object);
                self.collect_strings_in_expr(index);
            }
            Expr::SuperCall(args) => {
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::SuperMethodCall { method, args } => {
                self.intern_string(method);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::NewDynamic { callee, args } => {
                self.collect_strings_in_expr(callee);
                for a in args { self.collect_strings_in_expr(a); }
            }
            Expr::FetchWithOptions { url, method, body, headers } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(method);
                self.collect_strings_in_expr(body);
                for (key, val) in headers {
                    self.intern_string(key);
                    self.collect_strings_in_expr(val);
                }
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(auth_header);
            }
            Expr::FetchPostWithAuth { url, auth_header, body } => {
                self.collect_strings_in_expr(url);
                self.collect_strings_in_expr(auth_header);
                self.collect_strings_in_expr(body);
            }
            _ => {}
        }
    }
}

/// Context for emitting a single function body
struct FuncEmitCtx<'a> {
    emitter: &'a WasmModuleEmitter,
    local_map: &'a BTreeMap<LocalId, u32>,
    /// Block nesting depth for break/continue
    break_depth: Vec<u32>,
    loop_depth: Vec<u32>,
    block_depth: u32,
    /// Stack of (label, break_depth, continue_depth) for labeled break/continue.
    /// When `Labeled { label, body }` is a loop, this ties the label to the loop's blocks.
    label_stack: Vec<(String, u32, u32)>,
    /// Pending label to attach to the next loop encountered.
    pending_label: Option<String>,
    /// Current class name (set when compiling class methods/constructors)
    current_class: Option<String>,
    /// Index of a temp i64 local
    temp_local: u32,
    /// Index of a temp i32 local (for mem_call base address)
    temp_local_i32: u32,
    /// Index of a second temp i64 local for emit_store_arg
    temp_store_local: u32,
    /// Current frame size for emit_store_arg address computation
    current_frame_size: u32,
    /// Stack of saved frame sizes for nested frame support
    frame_stack: Vec<u32>,
}

impl<'a> FuncEmitCtx<'a> {
    fn new(emitter: &'a WasmModuleEmitter, local_map: &'a BTreeMap<LocalId, u32>, temp_local: u32, temp_local_i32: u32) -> Self {
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

    fn rt(&self) -> &RuntimeImports {
        self.emitter.rt.as_ref().unwrap()
    }

    // emit_nan_safe_const removed - all values are i64 now, NaN canonicalization is not an issue.

    /// Advance the sp and record the frame size for emit_store_arg.
    fn emit_frame_begin(&mut self, func: &mut Function, frame_size: u32) {
        let sp = self.emitter.nan_temp_global;
        self.frame_stack.push(self.current_frame_size);
        self.current_frame_size = frame_size;
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const((frame_size * 8) as i32));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::GlobalSet(sp));
    }

    /// Compute memory address for a slot in the current frame.
    /// Address = sp - (current_frame_size - slot) * 8
    /// This works correctly across nested calls because sp was advanced by emit_frame_begin.
    fn emit_slot_addr(&self, func: &mut Function, slot: u32) {
        let sp = self.emitter.nan_temp_global;
        let offset_from_sp = (self.current_frame_size - slot) * 8;
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(offset_from_sp as i32));
        func.instruction(&Instruction::I32Sub);
    }

    /// Store an expression's result to memory at the current frame's slot.
    fn emit_store_arg(&mut self, func: &mut Function, slot: u32, expr: &Expr) {
        match expr {
            Expr::String(s) => {
                let string_id = self.emitter.string_map.get(s.as_str()).copied().unwrap_or(0);
                let bits = (STRING_TAG << 48) | (string_id as u64);
                self.emit_slot_addr(func, slot);
                func.instruction(&Instruction::I64Const(bits as i64));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
            _ => {
                // Evaluate expression first, save to dedicated temp_store_local.
                // Prevents slot address (i32) from sitting on stack during nested memcalls.
                self.emit_expr(func, expr);
                func.instruction(&Instruction::LocalSet(self.temp_store_local));
                self.emit_slot_addr(func, slot);
                func.instruction(&Instruction::LocalGet(self.temp_store_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
            }
        }
    }

    fn emit_store_const(&self, func: &mut Function, slot: u32, val: f64) {
        let bits = val.to_bits();
        self.emit_slot_addr(func, slot);
        func.instruction(&Instruction::I64Const(bits as i64));
        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
    }

    /// Emit a load of a HIR local by id. Top-level `let`s are stored in WASM globals
    /// (not locals), so we must check `module_let_globals` before `local_map`. Falls
    /// back to `TAG_UNDEFINED`. Without this, Array* HIR nodes that reference a
    /// top-level `const xs = []` were pushing `I64Const(0)` into the temp — see
    /// Issue #133 item 3.
    fn emit_local_or_global_get(&self, func: &mut Function, id: &LocalId) {
        if let Some(&gidx) = self.emitter.module_let_globals.get(&(self.emitter.current_mod_idx, *id)) {
            func.instruction(&Instruction::GlobalGet(gidx));
        } else if let Some(&idx) = self.local_map.get(id) {
            func.instruction(&Instruction::LocalGet(idx));
        } else {
            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
        }
    }

    /// Emit a bridge function call via WASM memory (Firefox NaN-safe, reentrant-safe).
    /// Call pattern: emit_store_arg(0, ..), emit_store_arg(1, ..), ..., emit_memcall(name, N).
    /// Handles stack pointer save/advance/restore automatically.
    /// Call a bridge function. Frame must already be set up via emit_frame_begin + emit_store_arg.
    /// Returns f64 result, then restores sp.
    fn emit_memcall(&mut self, func: &mut Function, bridge_fn_name: &str, arg_count: u32) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self.emitter.string_map.get(bridge_fn_name).copied().unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        // base_addr = sp - frame_size * 8
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call));
        func.instruction(&Instruction::Drop);
        // Read result from base_addr via i64, then convert to f64.
        // NOTE: F64ReinterpretI64 canonicalizes NaN in Firefox, so NaN-boxed
        // bridge results lose their payload here. This is acceptable for values
        // that go to locals/arithmetic. For values that go to emit_store_arg,
        // the store will re-read from memory via the slot address.
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::I64Load(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
        // Result is already i64, no conversion needed
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
    }

    fn emit_memcall_void(&mut self, func: &mut Function, bridge_fn_name: &str, arg_count: u32) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self.emitter.string_map.get(bridge_fn_name).copied().unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call));
        func.instruction(&Instruction::Drop);
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
    }

    fn emit_memcall_i32(&mut self, func: &mut Function, bridge_fn_name: &str, arg_count: u32) {
        let sp = self.emitter.nan_temp_global;
        let func_name_id = self.emitter.string_map.get(bridge_fn_name).copied().unwrap_or(0);
        let frame_bytes = (self.current_frame_size * 8) as i32;
        func.instruction(&f64_const(func_name_id as f64));
        func.instruction(&f64_const(arg_count as f64));
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::Call(self.rt().mem_call_i32));
        // Restore sp and frame size
        func.instruction(&Instruction::GlobalGet(sp));
        func.instruction(&Instruction::I32Const(frame_bytes));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::GlobalSet(sp));
        self.current_frame_size = self.frame_stack.pop().unwrap_or(0);
    }





    /// Try to emit a method call on an object expression.
    /// Returns true if handled, false if not recognized.
    /// All bridge calls go through WASM memory to avoid Firefox NaN canonicalization.
    fn emit_method_call(&mut self, func: &mut Function, object: &Expr, method: &str, args: &[Expr]) -> bool {
        match method {
            // String methods
            "charAt" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_charAt", 2);
                true
            }
            "substring" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_substring", 3);
                true
            }
            "indexOf" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_indexOf", 2);
                true
            }
            "slice" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_slice", 3);
                true
            }
            "toLowerCase" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_toLowerCase", 1);
                true
            }
            "toUpperCase" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_toUpperCase", 1);
                true
            }
            "trim" if args.is_empty() => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "string_trim", 1);
                true
            }
            "includes" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_includes", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "startsWith" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_startsWith", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "endsWith" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "string_endsWith", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "replace" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_replace", 3);
                true
            }
            "split" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_split", 2);
                true
            }
            "repeat" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "string_repeat", 2);
                true
            }
            "padStart" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_padStart", 3);
                true
            }
            "padEnd" if args.len() >= 2 => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_store_arg(func, 2, &args[1]);
                self.emit_memcall(func, "string_padEnd", 3);
                true
            }
            // Array methods
            "push" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_push", 2);
                // result is the handle; now get length
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_length", 1);
                true
            }
            "pop" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_pop", 1);
                true
            }
            "shift" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_shift", 1);
                true
            }
            "join" => {
                self.emit_store_arg(func, 0, object);
                if !args.is_empty() {
                    self.emit_store_arg(func, 1, &args[0]);
                } else {
                    let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                    let bits = (STRING_TAG << 48) | (comma_id as u64);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 1, f64::from_bits(bits));
                }
                self.emit_memcall(func, "array_join", 2);
                true
            }
            "map" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_map", 2);
                true
            }
            "filter" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_filter", 2);
                true
            }
            "forEach" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_void(func, "array_forEach", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                true
            }
            "find" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_find", 2);
                true
            }
            "findIndex" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_find_index", 2);
                true
            }
            "reduce" if !args.is_empty() => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                if args.len() >= 2 {
                    self.emit_store_arg(func, 2, &args[1]);
                } else {
                    self.emit_slot_addr(func, 2);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                }
                self.emit_memcall(func, "array_reduce", 3);
                true
            }
            "sort" => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                if !args.is_empty() {
                    self.emit_store_arg(func, 1, &args[0]);
                } else {
                    self.emit_slot_addr(func, 1);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                }
                self.emit_memcall(func, "array_sort", 2);
                true
            }
            "reverse" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_reverse", 1);
                true
            }
            "concat" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall(func, "array_concat", 2);
                true
            }
            "flat" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "array_flat", 1);
                true
            }
            "toString" => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, object);
                self.emit_memcall(func, "jsvalue_to_string", 1);
                true
            }
            // Array some/every (return i32 → convert to boolean)
            "some" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "array_some", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            "every" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "array_every", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            // RegExp test
            "test" if !args.is_empty() => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, &args[0]);
                self.emit_memcall_i32(func, "regexp_test", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
                true
            }
            _ => false,
        }
    }

    /// Emit a binary bitwise operation with proper i32 truncation
    fn emit_bitwise_binary(&mut self, func: &mut Function, left: &Expr, right: &Expr, op: Instruction<'static>) {
        self.emit_expr(func, left);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I32TruncF64S);
        self.emit_expr(func, right);
        func.instruction(&Instruction::F64ReinterpretI64);
        func.instruction(&Instruction::I32TruncF64S);
        func.instruction(&op);
        func.instruction(&Instruction::F64ConvertI32S);
        func.instruction(&Instruction::I64ReinterpretF64);
    }

    fn emit_stmt(&mut self, func: &mut Function, stmt: &Stmt, in_returning_func: bool) {
        match stmt {
            Stmt::Let { id, init, .. } => {
                if let Some(init_expr) = init {
                    self.emit_expr(func, init_expr);
                } else {
                    // Default: undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                // Check module_let_globals FIRST (top-level Let → WASM global), then local_map
                if let Some(&gidx) = self.emitter.module_let_globals.get(&(self.emitter.current_mod_idx, *id)) {
                    func.instruction(&Instruction::GlobalSet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalSet(idx));
                } else {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Expr(expr) => {
                self.emit_expr(func, expr);
                // Drop the result (expression statement)
                // Check if expr produces a value
                if self.expr_has_value(expr) {
                    func.instruction(&Instruction::Drop);
                }
            }
            Stmt::Return(expr) => {
                if let Some(e) = expr {
                    self.emit_expr(func, e);
                } else if in_returning_func {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                func.instruction(&Instruction::Return);
            }
            Stmt::If { condition, then_branch, else_branch } => {
                // Convert to i32 boolean via is_truthy
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                for s in then_branch {
                    self.emit_stmt(func, s, in_returning_func);
                }
                if let Some(else_stmts) = else_branch {
                    func.instruction(&Instruction::Else);
                    for s in else_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::While { condition, body } => {
                // block $break
                //   loop $continue
                //     <condition>
                //     is_truthy
                //     i32.eqz
                //     br_if $break (1)
                //     <body>
                //     br $continue (0)
                //   end
                // end
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_depth = self.block_depth;
                self.break_depth.push(break_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_depth = self.block_depth;
                self.loop_depth.push(continue_depth);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_depth, continue_depth));
                    true
                } else { false };

                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::I32Eqz);
                func.instruction(&Instruction::BrIf(1)); // break to outer block

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                func.instruction(&Instruction::Br(0)); // continue (loop back)
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end loop

                if label_pushed { self.label_stack.pop(); }
                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end block
            }
            Stmt::DoWhile { body, condition } => {
                // block $break
                //   loop $continue
                //     <body>
                //     <condition>
                //     is_truthy
                //     br_if $continue (0) — loop back if truthy
                //   end
                // end
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_depth = self.block_depth;
                self.break_depth.push(break_depth);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_depth = self.block_depth;
                self.loop_depth.push(continue_depth);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_depth, continue_depth));
                    true
                } else { false };

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::BrIf(0)); // loop back if truthy

                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end loop

                if label_pushed { self.label_stack.pop(); }
                self.loop_depth.pop();
                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End); // end block
            }
            Stmt::Labeled { label, body } => {
                // Set the pending label and compile the body statement.
                // The inner loop will consume it and register in label_stack.
                self.pending_label = Some(label.clone());
                self.emit_stmt(func, body, in_returning_func);
                // If the body wasn't a loop, the pending label is stale; drop it.
                self.pending_label = None;
            }
            Stmt::For { init, condition, update, body } => {
                // <init>
                // block $break
                //   loop $loop_top
                //     <condition>
                //     is_truthy ; i32.eqz ; br_if $break   (rel=1, loop is directly inside block)
                //     block $body_end                       ← continue targets this block's exit
                //       <body>                             ← continue: br(rel) exits $body_end
                //     end                                  ← fall through to update
                //     <update> ; drop
                //     br 0                                 ← restart $loop_top (rel=0)
                //   end
                // end
                //
                // Wrapping the body in $body_end ensures `continue` falls through to
                // the update expression before restarting, fixing the iterator-stuck
                // bug when `continue` fires inside an if/else chain (issue #137).
                if let Some(init_stmt) = init {
                    self.emit_stmt(func, init_stmt, in_returning_func);
                }

                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let break_d = self.block_depth;
                self.break_depth.push(break_d);

                func.instruction(&Instruction::Loop(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                // block_depth is now the loop's depth; Br(0) here restarts the loop.

                if let Some(cond) = condition {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, cond);
                    self.emit_memcall_i32(func, "is_truthy", 1);
                    func.instruction(&Instruction::I32Eqz);
                    // loop is directly inside block, so break is always 1 level up
                    func.instruction(&Instruction::BrIf(1));
                }

                // Inner block: continue targets this block's exit, then update runs.
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                let continue_d = self.block_depth;
                self.loop_depth.push(continue_d);

                let label_pushed = if let Some(lbl) = self.pending_label.take() {
                    self.label_stack.push((lbl, break_d, continue_d));
                    true
                } else { false };

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                // Close inner body block; continue lands here, then falls to update.
                if label_pushed { self.label_stack.pop(); }
                self.loop_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);

                if let Some(upd) = update {
                    self.emit_expr(func, upd);
                    if self.expr_has_value(upd) {
                        func.instruction(&Instruction::Drop);
                    }
                }

                // Restart loop (block_depth == loop's depth, so rel=0).
                func.instruction(&Instruction::Br(0));
                self.block_depth -= 1;
                func.instruction(&Instruction::End);

                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
            Stmt::Break => {
                // Branch out to the enclosing loop's break block. Must compute
                // relative depth from break_depth/block_depth — a hardcoded Br(1)
                // miscounts when the break is nested inside an if/switch/try
                // (each pushes block_depth without a matching break_depth), which
                // in the worst case turns `break` into `continue` and hangs.
                if let Some(&target) = self.break_depth.last() {
                    let rel = self.block_depth.saturating_sub(target);
                    func.instruction(&Instruction::Br(rel));
                } else {
                    func.instruction(&Instruction::Br(1));
                }
            }
            Stmt::Continue => {
                if let Some(&target) = self.loop_depth.last() {
                    let rel = self.block_depth.saturating_sub(target);
                    func.instruction(&Instruction::Br(rel));
                } else {
                    func.instruction(&Instruction::Br(0));
                }
            }
            Stmt::LabeledBreak(label) => {
                // Find the label in the stack and compute the relative depth.
                if let Some(&(_, break_d, _)) = self.label_stack.iter().rev().find(|(l, _, _)| l == label) {
                    let rel = self.block_depth - break_d;
                    func.instruction(&Instruction::Br(rel));
                }
            }
            Stmt::LabeledContinue(label) => {
                if let Some(&(_, _, continue_d)) = self.label_stack.iter().rev().find(|(l, _, _)| l == label) {
                    let rel = self.block_depth - continue_d;
                    func.instruction(&Instruction::Br(rel));
                }
            }
            Stmt::Throw(expr) => {
                // Set exception in bridge and return
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, expr);
                self.emit_memcall_void(func, "throw_value", 1);
                if in_returning_func {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::Return);
                }
            }
            Stmt::Try { body, catch, finally } => {
                // Bridge-based exception handling:
                // try_start(); <try body>; try_end();
                // if has_exception(): <bind catch param>; <catch body>
                // <finally body>
                self.emit_frame_begin(func, 0);
                self.emit_memcall_void(func, "try_start", 0);

                for s in body {
                    self.emit_stmt(func, s, in_returning_func);
                }

                self.emit_frame_begin(func, 0);
                self.emit_memcall_void(func, "try_end", 0);

                // Check for exception and execute catch block
                if let Some(catch_clause) = catch {
                    self.emit_frame_begin(func, 0);
                    self.emit_memcall_i32(func, "has_exception", 0);
                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                    self.block_depth += 1;

                    // Bind catch parameter
                    if let Some((param_id, _)) = &catch_clause.param {
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "get_exception", 0);
                        if let Some(&local_idx) = self.local_map.get(param_id) {
                            func.instruction(&Instruction::LocalSet(local_idx));
                        } else {
                            func.instruction(&Instruction::Drop);
                        }
                    } else {
                        // No param, just clear the exception
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "get_exception", 0);
                        func.instruction(&Instruction::Drop);
                    }

                    for s in &catch_clause.body {
                        self.emit_stmt(func, s, in_returning_func);
                    }

                    self.block_depth -= 1;
                    func.instruction(&Instruction::End);
                }

                // Finally block (unconditional)
                if let Some(finally_stmts) = finally {
                    for s in finally_stmts {
                        self.emit_stmt(func, s, in_returning_func);
                    }
                }
            }
            Stmt::Switch { discriminant, cases } => {
                // Compile switch as cascading if/else blocks
                // Strategy: store discriminant in a local-like pattern, compare each case
                // Since we can't easily allocate a local here, we use nested blocks + br_table approach
                // Simpler approach: nested if/else with js_strict_eq

                // Outer block for break
                func.instruction(&Instruction::Block(wasm_encoder::BlockType::Empty));
                self.block_depth += 1;
                self.break_depth.push(self.block_depth);

                // We need to evaluate discriminant once. Without scratch locals,
                // we'll re-evaluate it for each case (works if it's a simple expression).
                // For complex discriminants, this could cause issues but handles most cases.

                let mut has_matched = false;
                for case in cases {
                    if let Some(test) = &case.test {
                        // case <test>:
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, discriminant);
                        self.emit_store_arg(func, 1, test);
                        self.emit_memcall_i32(func, "js_strict_eq", 2);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Empty));
                        self.block_depth += 1;
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        self.block_depth -= 1;
                        func.instruction(&Instruction::End);
                    } else {
                        // default:
                        for s in &case.body {
                            self.emit_stmt(func, s, in_returning_func);
                        }
                        has_matched = true;
                    }
                }
                let _ = has_matched;

                self.break_depth.pop();
                self.block_depth -= 1;
                func.instruction(&Instruction::End);
            }
        }
    }

    fn emit_expr(&mut self, func: &mut Function, expr: &Expr) {
        match expr {
            // --- Literals ---
            Expr::Number(n) => {
                func.instruction(&f64_const(*n));
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::Integer(i) => {
                func.instruction(&f64_const(*i as f64));
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::Bool(true) => {
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::Bool(false) => {
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
            }
            Expr::Undefined => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::Null => {
                func.instruction(&Instruction::I64Const(TAG_NULL as i64));
            }
            Expr::String(s) => {
                let string_id = self.emitter.string_map.get(s.as_str())
                    .copied().unwrap_or(0);
                // All values are i64 now. i64.const preserves all bits.
                let bits = (STRING_TAG << 48) | (string_id as u64);
                func.instruction(&Instruction::I64Const(bits as i64));
            }

            // --- Variables ---
            Expr::LocalGet(id) => {
                // Check module_let_globals FIRST (handles top-level Lets in current module)
                if let Some(&gidx) = self.emitter.module_let_globals.get(&(self.emitter.current_mod_idx, *id)) {
                    func.instruction(&Instruction::GlobalGet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    // Unknown local — push undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::LocalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&gidx) = self.emitter.module_let_globals.get(&(self.emitter.current_mod_idx, *id)) {
                    // Module-level let — write to WASM global, then read back to leave on stack
                    func.instruction(&Instruction::GlobalSet(gidx));
                    func.instruction(&Instruction::GlobalGet(gidx));
                } else if let Some(&idx) = self.local_map.get(id) {
                    // Tee: set and leave on stack
                    func.instruction(&Instruction::LocalTee(idx));
                }
            }
            Expr::GlobalGet(id) => {
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    func.instruction(&Instruction::GlobalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::GlobalSet(id, val) => {
                self.emit_expr(func, val);
                if let Some(&idx) = self.emitter.global_map.get(id) {
                    // Duplicate value on stack (set + leave result)
                    // WASM doesn't have GlobalTee, so we need a local
                    func.instruction(&Instruction::GlobalSet(idx));
                    func.instruction(&Instruction::GlobalGet(idx));
                }
            }

            // --- Update ---
            Expr::Update { id, op, prefix } => {
                if let Some(&idx) = self.local_map.get(id) {
                    if *prefix {
                        // ++x: increment then return new value
                        // local is i64, convert to f64, add 1, convert back
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => { func.instruction(&Instruction::F64Add); }
                            UpdateOp::Decrement => { func.instruction(&Instruction::F64Sub); }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::LocalTee(idx));
                    } else {
                        // x++: return old value, then increment
                        func.instruction(&Instruction::LocalGet(idx));
                        // Compute new value
                        func.instruction(&Instruction::LocalGet(idx));
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&f64_const(1.0));
                        match op {
                            UpdateOp::Increment => { func.instruction(&Instruction::F64Add); }
                            UpdateOp::Decrement => { func.instruction(&Instruction::F64Sub); }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                        func.instruction(&Instruction::LocalSet(idx));
                        // Old value (i64) is still on stack
                    }
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }

            // --- Binary operations ---
            Expr::Binary { op, left, right } => {
                match op {
                    BinaryOp::Add => {
                        // Use js_add for dynamic dispatch (handles string+number etc.)
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "js_add", 2);
                    }
                    // Bitwise ops need i32 truncation before the operation
                    BinaryOp::BitAnd => { self.emit_bitwise_binary(func, left, right, Instruction::I32And); }
                    BinaryOp::BitOr => { self.emit_bitwise_binary(func, left, right, Instruction::I32Or); }
                    BinaryOp::BitXor => { self.emit_bitwise_binary(func, left, right, Instruction::I32Xor); }
                    BinaryOp::Shl => { self.emit_bitwise_binary(func, left, right, Instruction::I32Shl); }
                    BinaryOp::Shr => { self.emit_bitwise_binary(func, left, right, Instruction::I32ShrS); }
                    BinaryOp::UShr => { self.emit_bitwise_binary(func, left, right, Instruction::I32ShrU); }
                    // Mod and Pow go through JS bridge (no native WASM instruction)
                    // — use emit_store_arg to keep values as i64, like Add
                    BinaryOp::Mod => {
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "js_mod", 2);
                    }
                    BinaryOp::Pow => {
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, left);
                        self.emit_store_arg(func, 1, right);
                        self.emit_memcall(func, "math_pow", 2);
                    }
                    _ => {
                        // Pure numeric operations - convert i64 to f64, operate, convert back
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::F64ReinterpretI64);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::F64ReinterpretI64);
                        match op {
                            BinaryOp::Sub => { func.instruction(&Instruction::F64Sub); }
                            BinaryOp::Mul => { func.instruction(&Instruction::F64Mul); }
                            BinaryOp::Div => { func.instruction(&Instruction::F64Div); }
                            _ => { func.instruction(&Instruction::F64Add); }
                        };
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                }
            }

            // --- Comparison ---
            Expr::Compare { op, left, right } => {
                self.emit_expr(func, left);
                self.emit_expr(func, right);
                // For strict equality on mixed types, use JS bridge
                match op {
                    CompareOp::Eq | CompareOp::Ne | CompareOp::LooseEq | CompareOp::LooseNe => {
                        // Values are i64 on stack, store them to memory via emit_store_arg pattern
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 1);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        let eq_fn = if matches!(op, CompareOp::LooseEq | CompareOp::LooseNe) { "js_loose_eq" } else { "js_strict_eq" };
                        self.emit_memcall_i32(func, eq_fn, 2);
                        if matches!(op, CompareOp::Ne | CompareOp::LooseNe) {
                            func.instruction(&Instruction::I32Eqz);
                        }
                        // Convert i32 result to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                    _ => {
                        // Numeric comparisons - convert i64 to f64 first
                        // Stack: [left_i64, right_i64]
                        func.instruction(&Instruction::LocalSet(self.temp_local)); // save right_i64
                        func.instruction(&Instruction::F64ReinterpretI64); // left -> f64
                        func.instruction(&Instruction::LocalGet(self.temp_local)); // push right_i64
                        func.instruction(&Instruction::F64ReinterpretI64); // right -> f64
                        match op {
                            CompareOp::Lt => func.instruction(&Instruction::F64Lt),
                            CompareOp::Le => func.instruction(&Instruction::F64Le),
                            CompareOp::Gt => func.instruction(&Instruction::F64Gt),
                            CompareOp::Ge => func.instruction(&Instruction::F64Ge),
                            _ => unreachable!(),
                        };
                        // Convert i32 to NaN-boxed boolean
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                }
            }

            // --- Logical ---
            Expr::Logical { op, left, right } => {
                match op {
                    LogicalOp::And => {
                        // Short-circuit: if left is falsy, return left; else return right
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Or => {
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::End);
                    }
                    LogicalOp::Coalesce => {
                        // a ?? b: if a is null/undefined, return b; otherwise return a
                        self.emit_frame_begin(func, 1);
                        self.emit_store_arg(func, 0, left);
                        self.emit_memcall_i32(func, "is_null_or_undefined", 1);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        self.emit_expr(func, right);
                        func.instruction(&Instruction::Else);
                        self.emit_expr(func, left);
                        func.instruction(&Instruction::End);
                    }
                }
            }

            // --- Unary ---
            Expr::Unary { op, operand } => {
                self.emit_expr(func, operand);
                match op {
                    UnaryOp::Neg => {
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&Instruction::F64Neg);
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                    UnaryOp::Pos => {} // no-op for numbers
                    UnaryOp::Not => {
                        self.emit_frame_begin(func, 1);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall_i32(func, "is_truthy", 1);
                        func.instruction(&Instruction::I32Eqz);
                        func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                        func.instruction(&Instruction::Else);
                        func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                        func.instruction(&Instruction::End);
                    }
                    UnaryOp::BitNot => {
                        // ~x: convert i64 to f64, truncate to i32, bitwise not, convert back to i64
                        func.instruction(&Instruction::F64ReinterpretI64);
                        func.instruction(&Instruction::I32TruncF64S);
                        func.instruction(&Instruction::I32Const(-1));
                        func.instruction(&Instruction::I32Xor);
                        func.instruction(&Instruction::F64ConvertI32S);
                        func.instruction(&Instruction::I64ReinterpretF64);
                    }
                };
            }

            // --- Function calls ---
            Expr::Call { callee, args, .. } => {
                // Check for method call patterns: obj.method(args)
                if let Expr::PropertyGet { object, property } = callee.as_ref() {
                    // console.log/warn/error
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        match property.as_str() {
                            "log" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_log", 1);
                                }
                                return;
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_warn", 1);
                                }
                                return;
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_error", 1);
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                    // String/Array method calls: expr.method(args)
                    if self.emit_method_call(func, object, property, args) {
                        return;
                    }

                    // Fallback: class/UI method dispatch via mem_call with stack-based buffer.
                    {
                        let method_name = property.as_str();
                        // Slot 0 = object, slots 1..N = args
                        self.emit_frame_begin(func, (args.len() + 1) as u32);
                        self.emit_store_arg(func, 0, object);
                        for (i, arg) in args.iter().enumerate() {
                            self.emit_store_arg(func, (i + 1) as u32, arg);
                        }
                        self.emit_memcall(func, method_name, (args.len() + 1) as u32);
                        return;
                    }
                }

                // Evaluate arguments first
                for arg in args {
                    self.emit_expr(func, arg);
                }
                // Call the function — resolve target and pad missing optional args with undefined
                match callee.as_ref() {
                    Expr::FuncRef(id) => {
                        if let Some(&idx) = self.emitter.func_map.get(id) {
                            // Pad missing arguments with TAG_UNDEFINED (for optional params)
                            if let Some(&expected) = self.emitter.func_param_counts.get(&idx) {
                                for _ in args.len()..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                            }
                            func.instruction(&Instruction::Call(idx));
                            // Void functions don't push a return value; push undefined
                            // so the caller always has a value on the stack.
                            if self.emitter.void_funcs.contains(&idx) {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        } else {
                            // Unknown function — push undefined
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                    Expr::ExternFuncRef { name, return_type, .. } => {
                        // Cross-module or FFI function call — look up by name
                        if let Some(&idx) = self.emitter.func_name_map.get(name) {
                            // Pad missing arguments with TAG_UNDEFINED (for optional params)
                            if let Some(&expected) = self.emitter.func_param_counts.get(&idx) {
                                for _ in args.len()..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                            }
                            func.instruction(&Instruction::Call(idx));
                            // Void functions don't push a return value, but call
                            // expressions always need a value on the stack. Push undefined.
                            if matches!(return_type, perry_types::Type::Void) || self.emitter.void_funcs.contains(&idx) {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        } else {
                            for _ in args {
                                func.instruction(&Instruction::Drop);
                            }
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                    _ => {
                        // Dynamic call via closure bridge
                        // Stack has: [arg0, arg1, ..., argN] but callee not yet pushed
                        // We need callee first for closure_call. Restructure:
                        // Drop the args we already pushed, re-emit callee first, then args
                        for _ in args {
                            func.instruction(&Instruction::Drop);
                        }
                        // Now emit: callee, args... via mem_call for Firefox NaN safety
                        self.emit_frame_begin(func, (args.len() + 1) as u32);
                        self.emit_store_arg(func, 0, callee);
                        for (i, arg) in args.iter().enumerate() {
                            self.emit_store_arg(func, (i + 1) as u32, arg);
                        }
                        match args.len() {
                            0 => { self.emit_memcall(func, "closure_call_0", 1); }
                            1 => { self.emit_memcall(func, "closure_call_1", 2); }
                            2 => { self.emit_memcall(func, "closure_call_2", 3); }
                            3 => { self.emit_memcall(func, "closure_call_3", 4); }
                            _ => {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                    }
                }
            }

            // --- Native method calls (console.log, etc.) ---
            Expr::NativeMethodCall { module, method, object, args, class_name } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                match normalized {
                    "console" => {
                        match method.as_str() {
                            "log" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_log", 1);
                                }
                            }
                            "warn" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_warn", 1);
                                }
                            }
                            "error" => {
                                for arg in args {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, arg);
                                    self.emit_memcall_void(func, "console_error", 1);
                                }
                            }
                            _ => {}
                        }
                    }
                    "JSON" => {
                        match method.as_str() {
                            "parse" => {
                                if let Some(a) = args.first() {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, a);
                                    self.emit_memcall(func, "json_parse", 1);
                                } else {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                            }
                            "stringify" => {
                                if let Some(a) = args.first() {
                                    self.emit_frame_begin(func, 1);
                                    self.emit_store_arg(func, 0, a);
                                    self.emit_memcall(func, "json_stringify", 1);
                                } else {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                            }
                            _ => {}
                        }
                    }
                    "Math" => {
                        match method.as_str() {
                            "floor" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64ReinterpretI64); func.instruction(&Instruction::F64Floor); func.instruction(&Instruction::I64ReinterpretF64); }
                            "ceil" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64ReinterpretI64); func.instruction(&Instruction::F64Ceil); func.instruction(&Instruction::I64ReinterpretF64); }
                            "round" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64ReinterpretI64); func.instruction(&Instruction::F64Nearest); func.instruction(&Instruction::I64ReinterpretF64); }
                            "abs" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64ReinterpretI64); func.instruction(&Instruction::F64Abs); func.instruction(&Instruction::I64ReinterpretF64); }
                            "sqrt" => { self.emit_expr(func, &args[0]); func.instruction(&Instruction::F64ReinterpretI64); func.instruction(&Instruction::F64Sqrt); func.instruction(&Instruction::I64ReinterpretF64); }
                            "pow" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_pow", 2);
                            }
                            "min" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Min);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "max" if args.len() >= 2 => {
                                self.emit_expr(func, &args[0]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                self.emit_expr(func, &args[1]);
                                func.instruction(&Instruction::F64ReinterpretI64);
                                func.instruction(&Instruction::F64Max);
                                func.instruction(&Instruction::I64ReinterpretF64);
                            }
                            "random" => {
                                self.emit_frame_begin(func, 0);
                                self.emit_memcall(func, "math_random", 0);
                            }
                            "log" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log", 1);
                            }
                            "log2" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log2", 1);
                            }
                            "log10" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_log10", 1);
                            }
                            // Trig / exp / sign / trunc / cbrt / hypot (Issue #133 item 4)
                            "sin" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sin", 1);
                            }
                            "cos" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cos", 1);
                            }
                            "tan" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_tan", 1);
                            }
                            "asin" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_asin", 1);
                            }
                            "acos" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_acos", 1);
                            }
                            "atan" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_atan", 1);
                            }
                            "atan2" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_atan2", 2);
                            }
                            "sinh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sinh", 1);
                            }
                            "cosh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cosh", 1);
                            }
                            "tanh" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_tanh", 1);
                            }
                            "exp" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_exp", 1);
                            }
                            "sign" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_sign", 1);
                            }
                            "trunc" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_trunc", 1);
                            }
                            "cbrt" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "math_cbrt", 1);
                            }
                            "hypot" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "math_hypot", 2);
                            }
                            _ => { func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); }
                        }
                    }
                    "perry/ui" | "perry/system" => {
                        // Memory-based dispatch: write args to WASM memory via i64.store.
                        let bridge_name = map_ui_method(method, class_name.as_deref());
                        let name_id = self.emitter.string_map.get(bridge_name).copied().unwrap_or(0);
                        let mut slot = 0u32;
                        let total_slots = (if object.is_some() { 1 } else { 0 }) + args.len() as u32;
                        self.emit_frame_begin(func, total_slots);

                        if let Some(obj) = object {
                            self.emit_store_arg(func, slot, obj);
                            slot += 1;
                        }
                        for arg in args {
                            self.emit_store_arg(func, slot, arg);
                            slot += 1;
                        }
                        self.emit_memcall(func, bridge_name, slot);
                    }
                    "perry/thread" => {
                        match method.as_str() {
                            "parallelMap" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "thread_parallel_map", 2);
                            }
                            "parallelFilter" if args.len() >= 2 => {
                                self.emit_frame_begin(func, 2);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_store_arg(func, 1, &args[1]);
                                self.emit_memcall(func, "thread_parallel_filter", 2);
                            }
                            "spawn" if !args.is_empty() => {
                                self.emit_frame_begin(func, 1);
                                self.emit_store_arg(func, 0, &args[0]);
                                self.emit_memcall(func, "thread_spawn", 1);
                            }
                            _ => {
                                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                            }
                        }
                    }
                    _ => {
                        // Handle instance method calls on objects
                        if let Some(obj) = object {
                            self.emit_expr(func, obj);
                            match method.as_str() {
                                // String instance methods
                                "charAt" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_char_at", 2);
                                }
                                "substring" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_substring", 3);
                                }
                                "indexOf" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_index_of", 2);
                                }
                                "slice" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_slice", 3);
                                }
                                "toLowerCase" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "string_to_lower_case", 1);
                                }
                                "toUpperCase" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "string_to_upper_case", 1);
                                }
                                "trim" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "string_trim", 1);
                                }
                                "includes" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_includes", 2);
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "startsWith" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_starts_with", 2);
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "endsWith" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_i32(func, "string_ends_with", 2);
                                    func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                                    func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                                    func.instruction(&Instruction::Else);
                                    func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                                    func.instruction(&Instruction::End);
                                }
                                "replace" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_replace", 3);
                                }
                                "split" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_split", 2);
                                }
                                "repeat" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "string_repeat", 2);
                                }
                                "padStart" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_pad_start", 3);
                                }
                                "padEnd" if args.len() >= 2 => {
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_store_arg(func, 2, &args[1]);
                                    self.emit_memcall(func, "string_pad_end", 3);
                                }
                                // Array instance methods called via NativeMethodCall
                                "push" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_push", 2);
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_length", 1);
                                }
                                "pop" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_pop", 1);
                                }
                                "shift" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_shift", 1);
                                }
                                "unshift" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_void(func, "array_unshift", 2);
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                "join" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                                        let bits = (STRING_TAG << 48) | (comma_id as u64);
                                        func.instruction(&Instruction::I64Const(bits as i64));
                                    }
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_join", 2);
                                }
                                "map" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_map", 2);
                                }
                                "filter" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_filter", 2);
                                }
                                "forEach" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall_void(func, "array_for_each", 2);
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                                "find" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_find", 2);
                                }
                                "findIndex" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_find_index", 2);
                                }
                                "reduce" if !args.is_empty() => {
                                    self.emit_expr(func, &args[0]);
                                    if args.len() >= 2 {
                                        self.emit_expr(func, &args[1]);
                                    } else {
                                        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                    }
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 2);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_reduce", 3);
                                }
                                "sort" => {
                                    if !args.is_empty() {
                                        self.emit_expr(func, &args[0]);
                                    } else {
                                        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                    }
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 1);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_sort", 2);
                                }
                                "reverse" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_reverse", 1);
                                }
                                "concat" if !args.is_empty() => {
                                    self.emit_frame_begin(func, 2);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_store_arg(func, 1, &args[0]);
                                    self.emit_memcall(func, "array_concat", 2);
                                }
                                "flat" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_flat", 1);
                                }
                                "length" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "array_length", 1);
                                }
                                // Response methods
                                "json" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "response_json", 1);
                                }
                                "text" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "response_text", 1);
                                }
                                "status" => {
                                    self.emit_frame_begin(func, 1);
                                    func.instruction(&Instruction::LocalSet(self.temp_local));
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "response_status", 1);
                                }
                                _ => {
                                    // Fall back to class_call_method via mem_call
                                    let method_id = self.emitter.string_map.get(method.as_str()).copied().unwrap_or(0);
                                    // obj is already on the stack from emit_expr(obj) above
                                    // Save obj to temp, build args array, then store all to memory
                                    func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 0 = obj handle
                                    self.emit_slot_addr(func, 0);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    let method_bits = (STRING_TAG << 48) | (method_id as u64);
                                    self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                                    // Build args array
                                    self.emit_frame_begin(func, 0);
                                    self.emit_memcall(func, "array_new", 0); // get new array handle
                                    for arg in args {
                                        self.emit_frame_begin(func, 2);
                                        func.instruction(&Instruction::LocalSet(self.temp_local));
                                        self.emit_slot_addr(func, 0);
                                        func.instruction(&Instruction::LocalGet(self.temp_local));
                                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                        self.emit_store_arg(func, 1, arg);
                                        self.emit_memcall(func, "array_push", 2); // push into array, returns handle
                                    }
                                    self.emit_frame_begin(func, 3);
                                    func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array handle
                                    self.emit_slot_addr(func, 2);
                                    func.instruction(&Instruction::LocalGet(self.temp_local));
                                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                                    self.emit_memcall(func, "class_call_method", 3);
                                }
                            }
                        } else {
                            // No object — module-level function
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                }
            }

            // --- Conditional (ternary) ---
            Expr::Conditional { condition, then_expr, else_expr } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, condition);
                self.emit_memcall_i32(func, "is_truthy", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                self.emit_expr(func, then_expr);
                func.instruction(&Instruction::Else);
                self.emit_expr(func, else_expr);
                func.instruction(&Instruction::End);
            }

            // --- Math ---
            Expr::MathFloor(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Floor);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathCeil(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Ceil);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathAbs(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Abs);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathSqrt(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Sqrt);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathRound(x) => {
                self.emit_expr(func, x);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Nearest);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathPow(base, exp) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, base);
                self.emit_store_arg(func, 1, exp);
                self.emit_memcall(func, "math_pow", 2);
            }
            Expr::MathMin(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::F64ReinterpretI64);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Min);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathMax(args) if args.len() == 2 => {
                self.emit_expr(func, &args[0]);
                func.instruction(&Instruction::F64ReinterpretI64);
                self.emit_expr(func, &args[1]);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::F64Max);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathRandom => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "math_random", 0);
            }

            // --- Typeof ---
            Expr::TypeOf(operand) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, operand);
                self.emit_memcall(func, "js_typeof", 1);
            }

            // --- Async ---
            Expr::Await(e) => {
                // Evaluate inner expression, then call await_promise bridge
                // If the value is a promise handle, tries to get resolved value
                // If not a promise, returns the value as-is
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, e);
                self.emit_memcall(func, "await_promise", 1);
            }

            // --- Object literal ---
            Expr::Object(fields) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                // Stack: [handle as i64]
                for (key, val) in fields {
                    // object_set(handle, key, value) returns handle (chaining)
                    let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                    let key_bits = (STRING_TAG << 48) | (key_id as u64);
                    // Save handle from stack to temp_local, then store via emit_slot_addr
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_frame_begin(func, 3);
                    // Store handle to slot 0
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    // Store key string to slot 1
                    self.emit_slot_addr(func, 1);
                    func.instruction(&Instruction::I64Const(key_bits as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    // Store value to slot 2
                    self.emit_store_arg(func, 2, val);
                    self.emit_memcall(func, "object_set", 3);
                }
                // Handle is on stack from last object_set (or object_new if no fields)
            }

            // --- Object spread ---
            Expr::ObjectSpread { parts } => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                for (key_opt, val) in parts {
                    if let Some(key) = key_opt {
                        let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_store_arg(func, 2, val);
                        self.emit_memcall(func, "object_set", 3);
                    } else {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_arg(func, 1, val);
                        self.emit_memcall(func, "object_assign", 2);
                    }
                }
            }

            // --- Array literal ---
            Expr::Array(elements) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for elem in elements {
                    self.emit_frame_begin(func, 2);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    self.emit_store_arg(func, 1, elem);
                    self.emit_memcall(func, "array_push", 2);
                }
            }

            // --- Array spread ---
            Expr::ArraySpread(elements) => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for elem in elements {
                    match elem {
                        ArrayElement::Expr(e) => {
                            self.emit_frame_begin(func, 2);
                            func.instruction(&Instruction::LocalSet(self.temp_local));
                            self.emit_slot_addr(func, 0);
                            func.instruction(&Instruction::LocalGet(self.temp_local));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            self.emit_store_arg(func, 1, e);
                            self.emit_memcall(func, "array_push", 2);
                        }
                        ArrayElement::Spread(e) => {
                            self.emit_frame_begin(func, 2);
                            func.instruction(&Instruction::LocalSet(self.temp_local));
                            self.emit_slot_addr(func, 0);
                            func.instruction(&Instruction::LocalGet(self.temp_local));
                            func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                            self.emit_store_arg(func, 1, e);
                            self.emit_memcall(func, "array_push_spread", 2);
                        }
                    }
                }
            }

            // --- Property access ---
            Expr::PropertyGet { object, property } => {
                // Special case: .length uses string_len which handles both strings and arrays
                if property == "length" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "string_len", 1);
                    return;
                }
                // Special case: .message on error objects
                if property == "message" {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, object);
                    self.emit_memcall(func, "error_message", 1);
                    return;
                }
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Use class_get_field (works for both plain objects and class instances)
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_memcall(func, "class_get_field", 2);
            }
            Expr::PropertySet { object, property, value } => {
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Use class_set_field (works for both plain objects and class instances)
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "class_set_field", 3);
                // class_set_field is void; push the object back for chaining
                self.emit_expr(func, object);
            }
            Expr::PropertyUpdate { object, property, op, prefix } => {
                // obj.prop++ or ++obj.prop
                self.emit_expr(func, object);
                let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                let key_bits = (STRING_TAG << 48) | (key_id as u64);
                // Get current value
                // We need the object handle twice. Can't dup in WASM without locals.
                // For simplicity: re-emit object (works if object is a simple expression)
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(key_bits));
                self.emit_memcall(func, "object_get", 2);
                // Stack: [old_value_i64]
                if *prefix {
                    func.instruction(&Instruction::F64ReinterpretI64);
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                    func.instruction(&Instruction::I64ReinterpretF64);
                    // Set new value
                    self.emit_expr(func, object);
                    func.instruction(&Instruction::I64Const(key_bits as i64));
                    // Stack: [new_val, handle, key] — wrong order for object_set(handle, key, val)
                    // We need to restructure. For now, just emit the value (prefix returns new)
                    // This is imprecise but works for basic cases
                } else {
                    // postfix: return old, then update
                    // For now, just do the increment and return new value (approximate)
                    func.instruction(&Instruction::F64ReinterpretI64);
                    func.instruction(&f64_const(1.0));
                    match op {
                        BinaryOp::Add => func.instruction(&Instruction::F64Add),
                        BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                        _ => func.instruction(&Instruction::F64Add),
                    };
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }

            // --- Index access ---
            Expr::IndexGet { object, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "object_get_dynamic", 2);
            }
            Expr::IndexSet { object, index, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "object_set_dynamic", 3);
                // set_dynamic is void; push undefined as expression result
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::IndexUpdate { object, index, op, prefix: _ } => {
                // Approximate: get, increment, set
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "object_get_dynamic", 2);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&f64_const(1.0));
                match op {
                    BinaryOp::Add => func.instruction(&Instruction::F64Add),
                    BinaryOp::Sub => func.instruction(&Instruction::F64Sub),
                    _ => func.instruction(&Instruction::F64Add),
                };
                func.instruction(&Instruction::I64ReinterpretF64);
            }

            // --- Object/Array methods ---
            Expr::ObjectKeys(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_keys", 1);
            }
            Expr::ObjectValues(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_values", 1);
            }
            Expr::ObjectEntries(obj) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, obj);
                self.emit_memcall(func, "object_entries", 1);
            }
            Expr::ObjectRest { object, .. } => {
                // For now, just return a copy of the object (approximate)
                self.emit_expr(func, object);
            }
            Expr::Delete(expr) => {
                match expr.as_ref() {
                    Expr::PropertyGet { object, property } => {
                        self.emit_expr(func, object);
                        let key_id = self.emitter.string_map.get(property.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_memcall_void(func, "object_delete", 2);
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                    }
                    Expr::IndexGet { object, index } => {
                        self.emit_frame_begin(func, 2);
                        self.emit_store_arg(func, 0, object);
                        self.emit_store_arg(func, 1, index);
                        self.emit_memcall_void(func, "object_delete_dynamic", 2);
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                    }
                    _ => {
                        func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                    }
                }
            }
            Expr::In { property, object } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, object);
                self.emit_store_arg(func, 1, property);
                self.emit_memcall_i32(func, "object_has_property", 2);
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Array methods (HIR-level) ---
            Expr::ArrayPush { array_id, value } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_arg(func, 1, value);
                // array_push returns handle, but ArrayPush typically returns new length
                // The bridge returns the array handle. We need to store back and return length.
                // For now, return the result of array_push (the handle).
                // Actually, drop result and push the new length
                self.emit_memcall(func, "array_push", 2);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_length", 1);
            }
            Expr::ArrayPushSpread { array_id, source } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_arg(func, 1, source);
                self.emit_memcall(func, "array_push_spread", 2);
                // Returns handle
            }
            Expr::ArrayPop(array_id) => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_pop", 1);
            }
            Expr::ArrayShift(array_id) => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_shift", 1);
            }
            Expr::ArrayUnshift { array_id, value } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "array_unshift", 2);
                // void return, push length
                self.emit_local_or_global_get(func, array_id);
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_length", 1);
            }
            Expr::ArraySlice { array, start, end } => {
                self.emit_expr(func, array);
                self.emit_expr(func, start);
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_slice", 3);
            }
            Expr::ArraySplice { array_id, start, delete_count, items } => {
                self.emit_local_or_global_get(func, array_id);
                self.emit_expr(func, start);
                if let Some(dc) = delete_count {
                    self.emit_expr(func, dc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_splice", 3);
                // Returns removed elements array handle
                // TODO: insert items if present
                let _ = items;
            }
            Expr::ArrayJoin { array, separator } => {
                self.emit_expr(func, array);
                if let Some(sep) = separator {
                    self.emit_expr(func, sep);
                } else {
                    // Default separator: ","
                    let comma_id = self.emitter.string_map.get(",").copied().unwrap_or(0);
                    let comma_bits = (STRING_TAG << 48) | (comma_id as u64);
                    func.instruction(&Instruction::I64Const(comma_bits as i64));
                }
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "array_join", 2);
            }
            Expr::ArrayIndexOf { array, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "array_index_of", 2);
            }
            Expr::ArrayIncludes { array, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_i32(func, "array_includes", 2);
                // Convert i32 to NaN-boxed boolean
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFlat { array } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_flat", 1);
            }
            Expr::ArrayIsArray(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "array_is_array", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::ArrayFrom(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "array_from", 1);
            }
            Expr::ArrayFromMapped { iterable, map_fn } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, iterable);
                self.emit_store_arg(func, 1, map_fn);
                self.emit_memcall(func, "array_from_mapped", 2);
            }

            // --- Array higher-order methods ---
            Expr::ArrayMap { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_map", 2);
            }
            Expr::ArrayFilter { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_filter", 2);
            }
            Expr::ArrayForEach { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall_void(func, "array_for_each", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::ArrayFind { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_find", 2);
            }
            Expr::ArrayFindIndex { array, callback } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, callback);
                self.emit_memcall(func, "array_find_index", 2);
            }
            Expr::ArraySort { array, comparator } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, comparator);
                self.emit_memcall(func, "array_sort", 2);
            }
            Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
                let is_right = matches!(expr, Expr::ArrayReduceRight { .. });
                self.emit_expr(func, array);
                self.emit_expr(func, callback);
                if let Some(init) = initial {
                    self.emit_expr(func, init);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                let name = if is_right { "array_reduce_right" } else { "array_reduce" };
                self.emit_memcall(func, name, 3);
            }
            Expr::ArrayToReversed { array } => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_to_reversed", 1);
            }
            Expr::ArrayToSorted { array, comparator } => {
                if let Some(cmp) = comparator {
                    self.emit_store_arg(func, 0, array);
                    self.emit_store_arg(func, 1, cmp);
                    self.emit_memcall(func, "array_to_sorted_cmp", 2);
                } else {
                    self.emit_store_arg(func, 0, array);
                    self.emit_memcall(func, "array_to_sorted", 1);
                }
            }
            Expr::ArrayToSpliced { array, start, delete_count, items } => {
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, start);
                self.emit_store_arg(func, 2, delete_count);
                // items passed as count only for now (WASM doesn't support varargs easily)
                self.emit_memcall(func, "array_to_spliced", 3);
            }
            Expr::ArrayWith { array, index, value } => {
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall(func, "array_with", 3);
            }
            Expr::ArrayCopyWithin { array_id, target, start, end } => {
                // emit local get for array_id
                func.instruction(&Instruction::LocalGet(*array_id));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_arg(func, 1, target);
                self.emit_store_arg(func, 2, start);
                if let Some(e) = end {
                    self.emit_store_arg(func, 3, e);
                    self.emit_memcall(func, "array_copy_within", 4);
                } else {
                    self.emit_memcall(func, "array_copy_within", 3);
                }
            }
            Expr::ArrayEntries(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_entries", 1);
            }
            Expr::ArrayKeys(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_keys", 1);
            }
            Expr::ArrayValues(array) => {
                self.emit_store_arg(func, 0, array);
                self.emit_memcall(func, "array_values", 1);
            }

            // --- Closure ---
            Expr::Closure { func_id, params, body, captures, mutable_captures, .. } => {
                // Compile closure body as a function (it was already registered if it's in module.functions)
                // If not registered, we need to handle it inline
                if let Some(&func_idx) = self.emitter.func_map.get(func_id) {
                    // Function is registered, create closure handle
                    // Use table index, not raw WASM function index
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, captures.len() as f64);
                    self.emit_memcall(func, "closure_new", 2);
                    // Set captures
                    for (i, cap_id) in captures.iter().chain(mutable_captures.iter()).enumerate() {
                        // Duplicate closure handle (it's returned by closure_new)
                        // closure_set_capture(handle, idx, value) -> handle (chaining)
                        func.instruction(&f64_const(i as f64));
                        func.instruction(&Instruction::I64ReinterpretF64);
                        if let Some(&local_idx) = self.local_map.get(cap_id) {
                            func.instruction(&Instruction::LocalGet(local_idx));
                        } else {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 2);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 1);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall(func, "closure_set_capture", 3);
                    }
                } else {
                    // Inline closure — not in function table, push undefined
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                let _ = (params, body);
            }
            Expr::FuncRef(id) => {
                if let Some(&func_idx) = self.emitter.func_map.get(id) {
                    // Create a closure wrapper with 0 captures for function reference
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, 0.0);
                    self.emit_memcall(func, "closure_new", 2);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::ExternFuncRef { name, .. } => {
                // Look up by function name in the flat function index space
                if let Some(&func_idx) = self.emitter.func_name_map.get(name) {
                    // Create a closure wrapper with 0 captures (like FuncRef)
                    let table_idx = self.emitter.func_to_table_idx.get(&func_idx).copied().unwrap_or(func_idx);
                    self.emit_frame_begin(func, 2);
                    self.emit_store_const(func, 0, table_idx as f64);
                    self.emit_store_const(func, 1, 0.0);
                    self.emit_memcall(func, "closure_new", 2);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }

            // --- Class instantiation ---
            Expr::New { class_name, args, .. } => {
                // Handle built-in constructors that need native JS objects
                match class_name.as_str() {
                    "RegExp" if args.len() >= 1 => {
                        self.emit_expr(func, &args[0]);
                        if args.len() >= 2 {
                            self.emit_expr(func, &args[1]);
                        } else {
                            // Empty flags string
                            let empty_id = self.emitter.string_map.get("").copied().unwrap_or(0);
                            let empty_bits = (STRING_TAG << 48) | (empty_id as u64);
                            func.instruction(&Instruction::I64Const(empty_bits as i64));
                        }
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 1);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall(func, "regexp_new", 2);
                        return;
                    }
                    "Error" => {
                        if let Some(msg) = args.first() {
                            self.emit_expr(func, msg);
                        } else {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        self.emit_frame_begin(func, 1);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall(func, "error_new", 1);
                        return;
                    }
                    "Date" => {
                        if let Some(arg) = args.first() {
                            self.emit_expr(func, arg);
                        } else {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        self.emit_frame_begin(func, 1);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall(func, "date_new", 1);
                        return;
                    }
                    "Map" => {
                        self.emit_frame_begin(func, 0);
                        self.emit_memcall(func, "map_new", 0);
                        return;
                    }
                    "Set" => {
                        if let Some(arg) = args.first() {
                            self.emit_frame_begin(func, 1);
                            self.emit_store_arg(func, 0, arg);
                            self.emit_memcall(func, "set_new_from_array", 1);
                        } else {
                            self.emit_frame_begin(func, 0);
                            self.emit_memcall(func, "set_new", 0);
                        }
                        return;
                    }
                    "URL" => {
                        if let Some(arg) = args.first() {
                            self.emit_expr(func, arg);
                        } else {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                        self.emit_frame_begin(func, 1);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_memcall(func, "url_parse", 1);
                        return;
                    }
                    _ => {}
                }

                // User-defined class instantiation
                let class_name_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_name_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, args.len() as f64);
                self.emit_memcall(func, "class_new", 2);
                // Call the compiled constructor if it exists
                if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(class_name.as_str()) {
                    // Stack: [instance_handle]
                    for arg in args {
                        self.emit_expr(func, arg);
                    }
                    // Pad missing arguments with TAG_UNDEFINED
                    if let Some(&expected) = self.emitter.func_param_counts.get(&ctor_idx) {
                        let provided = args.len() + 1;
                        for _ in provided..expected {
                            func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                        }
                    }
                    func.instruction(&Instruction::Call(ctor_idx));
                }
                // If no compiled constructor, just leave the instance handle on stack
            }
            Expr::NewDynamic { callee, args } => {
                // Dynamic new — approximate with regular call via mem_call
                self.emit_frame_begin(func, (args.len() + 1) as u32);
                self.emit_store_arg(func, 0, callee);
                for (i, arg) in args.iter().enumerate() {
                    self.emit_store_arg(func, (i + 1) as u32, arg);
                }
                match args.len() {
                    0 => { self.emit_memcall(func, "closure_call_0", 1); }
                    1 => { self.emit_memcall(func, "closure_call_1", 2); }
                    2 => { self.emit_memcall(func, "closure_call_2", 3); }
                    3 => { self.emit_memcall(func, "closure_call_3", 4); }
                    _ => {
                        func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    }
                }
            }
            Expr::This => {
                // 'this' is passed as first parameter (local 0) in methods
                func.instruction(&Instruction::LocalGet(0));
            }
            Expr::SuperCall(args) => {
                // Call parent constructor: super(args)
                // this is local 0 in the current constructor
                let mut called = false;
                if let Some(ref current_class) = self.current_class {
                    // Look up parent class name
                    if let Some(parent_name) = self.emitter.class_parent_map.get(current_class) {
                        if let Some(&ctor_idx) = self.emitter.class_ctor_map.get(parent_name) {
                            // Call parent constructor with this + args
                            func.instruction(&Instruction::LocalGet(0)); // this
                            for arg in args {
                                self.emit_expr(func, arg);
                            }
                            if let Some(&expected) = self.emitter.func_param_counts.get(&ctor_idx) {
                                let provided = args.len() + 1;
                                for _ in provided..expected {
                                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                                }
                            }
                            func.instruction(&Instruction::Call(ctor_idx));
                            func.instruction(&Instruction::Drop); // parent ctor returns this, discard
                            called = true;
                        }
                    }
                }
                if !called {
                    // No parent constructor found, drop args
                    for arg in args {
                        self.emit_expr(func, arg);
                        func.instruction(&Instruction::Drop);
                    }
                }
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::SuperMethodCall { method, args } => {
                // Call parent method on this via class_call_method (walks parent chain)
                self.emit_slot_addr(func, 0); // this handle
                func.instruction(&Instruction::LocalGet(0));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 })); // slot 0 = this (already i64)
                let method_id = self.emitter.string_map.get(method.as_str()).copied().unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                // Build args array
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for arg in args {
                    self.emit_frame_begin(func, 2);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    self.emit_store_arg(func, 1, arg);
                    self.emit_memcall(func, "array_push", 2);
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "class_call_method", 3);
            }
            Expr::ClassRef(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::StaticFieldGet { class_name, field_name } => {
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self.emitter.string_map.get(field_name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, f64::from_bits(field_bits));
                self.emit_memcall(func, "class_get_static", 2);
            }
            Expr::StaticFieldSet { class_name, field_name, value } => {
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let field_id = self.emitter.string_map.get(field_name.as_str()).copied().unwrap_or(0);
                let field_bits = (STRING_TAG << 48) | (field_id as u64);
                self.emit_frame_begin(func, 3);
                self.emit_store_const(func, 0, f64::from_bits(class_bits));
                self.emit_store_const(func, 1, f64::from_bits(field_bits));
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "class_set_static", 3);
                // void return, push the value back
                self.emit_expr(func, value);
            }
            Expr::StaticMethodCall { class_name, method_name, args } => {
                // Try to call compiled static method directly
                if let Some(statics) = self.emitter.class_static_map.get(class_name.as_str()) {
                    if let Some(&static_idx) = statics.get(method_name.as_str()) {
                        // Direct call to compiled static method (no this param)
                        for arg in args {
                            self.emit_expr(func, arg);
                        }
                        func.instruction(&Instruction::Call(static_idx));
                        return;
                    }
                }
                // Fallback: bridge dispatch via mem_call
                let class_id = self.emitter.string_map.get(class_name.as_str()).copied().unwrap_or(0);
                let class_bits = (STRING_TAG << 48) | (class_id as u64);
                let method_id = self.emitter.string_map.get(method_name.as_str()).copied().unwrap_or(0);
                let method_bits = (STRING_TAG << 48) | (method_id as u64);
                self.emit_store_const(func, 0, f64::from_bits(class_bits)); // slot 0 = class handle
                self.emit_store_const(func, 1, f64::from_bits(method_bits)); // slot 1 = method name
                // Build args array
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "array_new", 0);
                for arg in args {
                    self.emit_frame_begin(func, 2);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    self.emit_store_arg(func, 1, arg);
                    self.emit_memcall(func, "array_push", 2);
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local)); // slot 2 = args array
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "class_call_method", 3);
            }

            // --- Enum members ---
            Expr::EnumMember { enum_name, member_name } => {
                // Look up resolved value from enum definitions
                let key = (enum_name.clone(), member_name.clone());
                if let Some(resolved) = self.emitter.enum_values.get(&key) {
                    match resolved.clone() {
                        EnumResolvedValue::Number(n) => {
                            func.instruction(&f64_const(n));
                            func.instruction(&Instruction::I64ReinterpretF64);
                        }
                        EnumResolvedValue::String(s) => {
                            let id = self.emitter.string_map.get(s.as_str()).copied().unwrap_or(0);
                            let bits = (STRING_TAG << 48) | (id as u64);
                            func.instruction(&Instruction::I64Const(bits as i64));
                        }
                    }
                } else if let Ok(n) = member_name.parse::<f64>() {
                    func.instruction(&f64_const(n));
                    func.instruction(&Instruction::I64ReinterpretF64);
                } else {
                    // Fallback: return the member name as a string
                    let id = self.emitter.string_map.get(member_name.as_str()).copied().unwrap_or(0);
                    let bits = (STRING_TAG << 48) | (id as u64);
                    func.instruction(&Instruction::I64Const(bits as i64));
                }
            }

            // --- InstanceOf ---
            Expr::InstanceOf { expr, ty } => {
                self.emit_expr(func, expr);
                let type_id = self.emitter.string_map.get(ty.as_str()).copied().unwrap_or(0);
                let type_bits = (STRING_TAG << 48) | (type_id as u64);
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(type_bits));
                self.emit_memcall_i32(func, "class_instanceof", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Void ---
            Expr::Void(e) => {
                self.emit_expr(func, e);
                func.instruction(&Instruction::Drop);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- String methods ---
            Expr::StringSplit(string, delim) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, delim);
                self.emit_memcall(func, "string_split", 2);
            }
            Expr::StringFromCharCode(code) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_from_char_code", 1);
            }
            Expr::StringFromCodePoint(code) => {
                // WASM stub: same as fromCharCode for now (BMP-only)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, code);
                self.emit_memcall(func, "string_from_char_code", 1);
            }
            Expr::StringAt { string, index } => {
                // WASM stub: alias to char_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_at", 2);
            }
            Expr::StringCodePointAt { string, index } => {
                // WASM stub: alias to char_code_at
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "string_char_code_at", 2);
            }
            Expr::StringMatch { string, regex } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, regex);
                self.emit_memcall(func, "string_match", 2);
            }
            Expr::StringReplace { string, pattern, replacement } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, string);
                self.emit_store_arg(func, 1, pattern);
                self.emit_store_arg(func, 2, replacement);
                self.emit_memcall(func, "string_replace", 3);
            }
            Expr::StringCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "jsvalue_to_string", 1);
            }

            // --- JSON ---
            Expr::JsonParse(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_parse", 1);
            }
            Expr::JsonStringify(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "json_stringify", 1);
            }

            // --- Map ---
            Expr::MapNew => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "map_new", 0);
            }
            Expr::MapNewFromArray(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "map_new_from_array", 1);
            }
            Expr::MapSet { map, key, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "map_set", 3);
                // void return, push the map back
                self.emit_expr(func, map);
            }
            Expr::MapGet { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall(func, "map_get", 2);
            }
            Expr::MapHas { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall_i32(func, "map_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::MapDelete { map, key } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, map);
                self.emit_store_arg(func, 1, key);
                self.emit_memcall_void(func, "map_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::MapSize(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_size", 1);
            }
            Expr::MapClear(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall_void(func, "map_clear", 1);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::MapEntries(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_entries", 1);
            }
            Expr::MapKeys(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_keys", 1);
            }
            Expr::MapValues(map) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, map);
                self.emit_memcall(func, "map_values", 1);
            }

            // --- Set ---
            Expr::SetNew => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "set_new", 0);
            }
            Expr::SetNewFromArray(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "set_new_from_array", 1);
            }
            Expr::SetAdd { set_id, value } => {
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "set_add", 2);
                // void, push set back
                if let Some(&idx) = self.local_map.get(set_id) {
                    func.instruction(&Instruction::LocalGet(idx));
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::SetHas { set, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, set);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_i32(func, "set_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::SetDelete { set, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, set);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall_void(func, "set_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
            }
            Expr::SetSize(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall(func, "set_size", 1);
            }
            Expr::SetClear(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall_void(func, "set_clear", 1);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::SetValues(set) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, set);
                self.emit_memcall(func, "set_values", 1);
            }

            // --- Date ---
            Expr::DateNew(arg) => {
                if let Some(a) = arg {
                    self.emit_expr(func, a);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "date_new", 1);
            }
            Expr::DateGetTime(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_time", 1);
            }
            Expr::DateToISOString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_iso_string", 1);
            }
            Expr::DateGetFullYear(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_full_year", 1);
            }
            Expr::DateGetMonth(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_month", 1);
            }
            Expr::DateGetDate(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_date", 1);
            }
            Expr::DateGetHours(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_hours", 1);
            }
            Expr::DateGetMinutes(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_minutes", 1);
            }
            Expr::DateGetSeconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_seconds", 1);
            }
            Expr::DateGetMilliseconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_milliseconds", 1);
            }
            Expr::DateParse(s) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, s);
                self.emit_memcall(func, "date_parse", 1);
            }
            Expr::DateUtc(args) => {
                let n = args.len().max(1) as u32;
                self.emit_frame_begin(func, n);
                for (i, a) in args.iter().enumerate() {
                    self.emit_store_arg(func, i as u32, a);
                }
                self.emit_memcall(func, "date_utc", n);
            }
            Expr::DateGetUtcDay(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_day", 1);
            }
            Expr::DateGetUtcFullYear(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_full_year", 1);
            }
            Expr::DateGetUtcMonth(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_month", 1);
            }
            Expr::DateGetUtcDate(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_date", 1);
            }
            Expr::DateGetUtcHours(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_hours", 1);
            }
            Expr::DateGetUtcMinutes(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_minutes", 1);
            }
            Expr::DateGetUtcSeconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_seconds", 1);
            }
            Expr::DateGetUtcMilliseconds(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_utc_milliseconds", 1);
            }
            Expr::DateValueOf(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_value_of", 1);
            }
            Expr::DateGetTimezoneOffset(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_get_timezone_offset", 1);
            }
            Expr::DateToDateString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_date_string", 1);
            }
            Expr::DateToTimeString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_time_string", 1);
            }
            Expr::DateToLocaleDateString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_date_string", 1);
            }
            Expr::DateToLocaleTimeString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_time_string", 1);
            }
            Expr::DateToLocaleString(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_locale_string", 1);
            }
            Expr::DateToJSON(d) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, d);
                self.emit_memcall(func, "date_to_json", 1);
            }
            Expr::DateSetUtcFullYear { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_full_year", 2);
            }
            Expr::DateSetUtcMonth { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_month", 2);
            }
            Expr::DateSetUtcDate { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_date", 2);
            }
            Expr::DateSetUtcHours { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_hours", 2);
            }
            Expr::DateSetUtcMinutes { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_minutes", 2);
            }
            Expr::DateSetUtcSeconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_seconds", 2);
            }
            Expr::DateSetUtcMilliseconds { date, value } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, date);
                self.emit_store_arg(func, 1, value);
                self.emit_memcall(func, "date_set_utc_milliseconds", 2);
            }

            // --- Error ---
            Expr::ErrorNew(msg) => {
                if let Some(m) = msg {
                    self.emit_expr(func, m);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::ErrorMessage(err) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, err);
                self.emit_memcall(func, "error_message", 1);
            }
            Expr::ErrorNewWithCause { message, cause: _ } => {
                // WASM stub: ignore cause for now, falls back to plain Error
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::TypeErrorNew(msg) | Expr::RangeErrorNew(msg) | Expr::ReferenceErrorNew(msg) | Expr::SyntaxErrorNew(msg) => {
                // WASM stub: alias to error_new
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, msg);
                self.emit_memcall(func, "error_new", 1);
            }
            Expr::AggregateErrorNew { errors: _, message } => {
                // WASM stub: alias to error_new (drops errors array)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, message);
                self.emit_memcall(func, "error_new", 1);
            }

            // --- RegExp ---
            Expr::RegExp { pattern, flags } => {
                let pat_id = self.emitter.string_map.get(pattern.as_str()).copied().unwrap_or(0);
                let pat_bits = (STRING_TAG << 48) | (pat_id as u64);
                let flags_id = self.emitter.string_map.get(flags.as_str()).copied().unwrap_or(0);
                let flags_bits = (STRING_TAG << 48) | (flags_id as u64);
                self.emit_frame_begin(func, 2);
                self.emit_store_const(func, 0, f64::from_bits(pat_bits));
                self.emit_store_const(func, 1, f64::from_bits(flags_bits));
                self.emit_memcall(func, "regexp_new", 2);
            }
            Expr::RegExpTest { regex, string } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, regex);
                self.emit_store_arg(func, 1, string);
                self.emit_memcall_i32(func, "regexp_test", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }

            // --- Global builtins ---
            Expr::ParseInt { string, radix } => {
                self.emit_expr(func, string);
                let _ = radix; // TODO: radix support
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "parse_int", 1);
            }
            Expr::ParseFloat(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "parse_float", 1);
            }
            Expr::NumberCoerce(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "number_coerce", 1);
            }
            Expr::IsNaN(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "is_nan", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::IsUndefinedOrBareNan(val) => {
                // WASM fallback: delegate to is_nan (close enough for most cases)
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "is_nan", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::IsFinite(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "is_finite", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BigIntCoerce(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- Math extra ---
            Expr::MathLog2(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_log2", 1);
            }
            Expr::MathLog10(x) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, x);
                self.emit_memcall(func, "math_log10", 1);
            }
            // Issue #133 item 4: trig / exp / etc. are lowered to Expr::Math* at the HIR level
            // (see perry-hir/src/lower.rs). Route them through the Firefox-safe mem_call bridge.
            Expr::MathSin(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_sin", 1); }
            Expr::MathCos(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_cos", 1); }
            Expr::MathTan(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_tan", 1); }
            Expr::MathAsin(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_asin", 1); }
            Expr::MathAcos(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_acos", 1); }
            Expr::MathAtan(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_atan", 1); }
            Expr::MathAtan2(y, x) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, y);
                self.emit_store_arg(func, 1, x);
                self.emit_memcall(func, "math_atan2", 2);
            }
            Expr::MathSinh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_sinh", 1); }
            Expr::MathCosh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_cosh", 1); }
            Expr::MathTanh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_tanh", 1); }
            Expr::MathAsinh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_asinh", 1); }
            Expr::MathAcosh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_acosh", 1); }
            Expr::MathAtanh(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_atanh", 1); }
            Expr::MathCbrt(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_cbrt", 1); }
            Expr::MathExp(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_exp", 1); }
            Expr::MathExpm1(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_expm1", 1); }
            Expr::MathLog1p(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_log1p", 1); }
            Expr::MathFround(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_fround", 1); }
            Expr::MathClz32(x) => { self.emit_frame_begin(func, 1); self.emit_store_arg(func, 0, x); self.emit_memcall(func, "math_clz32", 1); }
            Expr::MathHypot(args) => {
                // Variadic: iteratively fold via math_hypot(acc, x)
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_hypot", 2);
                    }
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }
            Expr::MathImul(a, b) => {
                self.emit_expr(func, a);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::I32TruncF64S);
                self.emit_expr(func, b);
                func.instruction(&Instruction::F64ReinterpretI64);
                func.instruction(&Instruction::I32TruncF64S);
                func.instruction(&Instruction::I32Mul);
                func.instruction(&Instruction::F64ConvertI32S);
                func.instruction(&Instruction::I64ReinterpretF64);
            }
            Expr::MathMin(args) if args.len() != 2 => {
                // Variadic min — use bridge
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_min", 2);
                    }
                } else {
                    func.instruction(&f64_const(f64::INFINITY));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }
            Expr::MathMax(args) if args.len() != 2 => {
                if let Some(first) = args.first() {
                    self.emit_expr(func, first);
                    for arg in &args[1..] {
                        self.emit_frame_begin(func, 2);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_arg(func, 1, arg);
                        self.emit_memcall(func, "math_max", 2);
                    }
                } else {
                    func.instruction(&f64_const(f64::NEG_INFINITY));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
            }

            // --- URL ---
            Expr::UrlNew { url, base } => {
                self.emit_expr(func, url);
                if let Some(b) = base {
                    // URL(url, base) — for now just use url
                    self.emit_expr(func, b);
                    func.instruction(&Instruction::Drop);
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "url_parse", 1);
            }
            Expr::UrlGetHref(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_href", 1);
            }
            Expr::UrlGetPathname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_pathname", 1);
            }
            Expr::UrlGetProtocol(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_protocol", 1);
            }
            Expr::UrlGetHost(u) | Expr::UrlGetHostname(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hostname", 1);
            }
            Expr::UrlGetPort(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_port", 1);
            }
            Expr::UrlGetSearch(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search", 1);
            }
            Expr::UrlGetHash(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_hash", 1);
            }
            Expr::UrlGetOrigin(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_origin", 1);
            }
            Expr::UrlGetSearchParams(u) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, u);
                self.emit_memcall(func, "url_get_search_params", 1);
            }

            // --- Process/OS ---
            Expr::ProcessArgv => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_argv", 0);
            }
            Expr::ProcessCwd => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "process_cwd", 0);
            }
            Expr::OsPlatform => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "os_platform", 0);
            }
            Expr::ProcessUptime | Expr::ProcessMemoryUsage |
            Expr::ProcessPid | Expr::ProcessPpid | Expr::ProcessVersion | Expr::ProcessVersions |
            Expr::ProcessHrtimeBigint | Expr::ProcessStdin | Expr::ProcessStdout | Expr::ProcessStderr |
            Expr::OsArch | Expr::OsHostname | Expr::OsHomedir | Expr::OsTmpdir |
            Expr::OsTotalmem | Expr::OsFreemem | Expr::OsUptime |
            Expr::OsType | Expr::OsRelease | Expr::OsCpus | Expr::OsNetworkInterfaces |
            Expr::OsUserInfo | Expr::OsEOL => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::ProcessNextTick(_) | Expr::ProcessChdir(_) |
            Expr::ProcessOn { .. } | Expr::ProcessKill { .. } |
            Expr::ProcessExit(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::EnvGet(_) | Expr::EnvGetDynamic(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- FS stubs ---
            Expr::FsReadFileSync(_) | Expr::FsWriteFileSync(_, _) |
            Expr::FsExistsSync(_) | Expr::FsMkdirSync(_) |
            Expr::FsUnlinkSync(_) | Expr::FsAppendFileSync(_, _) |
            Expr::FsReadFileBinary(_) | Expr::FsRmRecursive(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Path ---
            Expr::PathJoin(a, b) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, a);
                self.emit_store_arg(func, 1, b);
                self.emit_memcall(func, "path_join", 2);
            }
            Expr::PathDirname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_dirname", 1);
            }
            Expr::PathBasename(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_basename", 1);
            }
            Expr::PathExtname(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_extname", 1);
            }
            Expr::PathResolve(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_resolve", 1);
            }
            Expr::PathIsAbsolute(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall_i32(func, "path_is_absolute", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::FileURLToPath(p) => {
                self.emit_expr(func, p);
                // In WASM, just return the string as-is
            }
            Expr::PathRelative(from, to) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, from);
                self.emit_store_arg(func, 1, to);
                self.emit_memcall(func, "path_relative", 2);
            }
            Expr::PathNormalize(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_normalize", 1);
            }
            Expr::PathParse(p) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, p);
                self.emit_memcall(func, "path_parse", 1);
            }
            Expr::PathFormat(o) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, o);
                self.emit_memcall(func, "path_format", 1);
            }
            Expr::PathBasenameExt(p, ext) => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, p);
                self.emit_store_arg(func, 1, ext);
                self.emit_memcall(func, "path_basename", 2);
            }
            Expr::PathSep => {
                self.emit_memcall(func, "path_sep", 0);
            }
            Expr::PathDelimiter => {
                self.emit_memcall(func, "path_delimiter", 0);
            }
            // --- WeakRef and FinalizationRegistry (stub: routes to host runtime) ---
            Expr::WeakRefNew(target) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, target);
                self.emit_memcall(func, "weakref_new", 1);
            }
            Expr::WeakRefDeref(weakref_expr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, weakref_expr);
                self.emit_memcall(func, "weakref_deref", 1);
            }
            Expr::FinalizationRegistryNew(callback) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, callback);
                self.emit_memcall(func, "finreg_new", 1);
            }
            Expr::FinalizationRegistryRegister { registry, target, held, token } => {
                self.emit_frame_begin(func, 4);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, target);
                self.emit_store_arg(func, 2, held);
                if let Some(t) = token {
                    self.emit_store_arg(func, 3, t);
                } else {
                    self.emit_slot_addr(func, 3);
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                }
                self.emit_memcall(func, "finreg_register", 4);
            }
            Expr::FinalizationRegistryUnregister { registry, token } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, registry);
                self.emit_store_arg(func, 1, token);
                self.emit_memcall(func, "finreg_unregister", 2);
            }
            // --- Buffer/TypedArray ---
            Expr::BufferAlloc { ref size, .. } => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, size.as_ref());
                self.emit_memcall(func, "buffer_alloc", 1);
            }
            Expr::BufferAllocUnsafe(size) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, size);
                self.emit_memcall(func, "buffer_alloc", 1);
            }
            Expr::BufferFrom { data, encoding } => {
                self.emit_expr(func, data);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "buffer_from_string", 2);
            }
            Expr::BufferToString { buffer, encoding } => {
                self.emit_expr(func, buffer);
                if let Some(enc) = encoding {
                    self.emit_expr(func, enc);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 2);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "buffer_to_string", 2);
            }
            Expr::BufferLength(buf) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, buf);
                self.emit_memcall(func, "buffer_length", 1);
            }
            Expr::BufferSlice { buffer, start, end } => {
                self.emit_expr(func, buffer);
                if let Some(s) = start {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
                if let Some(e) = end {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "buffer_slice", 3);
            }
            Expr::BufferConcat(arr) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, arr);
                self.emit_memcall(func, "buffer_concat", 1);
            }
            Expr::BufferIndexGet { buffer, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "buffer_get", 2);
            }
            Expr::BufferIndexSet { buffer, index, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "buffer_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::BufferCopy { source, target, target_start, source_start, source_end } => {
                self.emit_expr(func, source);
                self.emit_expr(func, target);
                if let Some(ts) = target_start {
                    self.emit_expr(func, ts);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(ss) = source_start {
                    self.emit_expr(func, ss);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(se) = source_end {
                    self.emit_expr(func, se);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 5);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 4);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "buffer_copy", 5);
            }
            Expr::BufferWrite { buffer, string, offset, encoding } => {
                self.emit_expr(func, buffer);
                self.emit_expr(func, string);
                if let Some(o) = offset {
                    self.emit_expr(func, o);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                if let Some(e) = encoding {
                    self.emit_expr(func, e);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "buffer_write", 4);
            }
            Expr::BufferEquals { buffer, other } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, buffer);
                self.emit_store_arg(func, 1, other);
                self.emit_memcall_i32(func, "buffer_equals", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BufferIsBuffer(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall_i32(func, "buffer_is_buffer", 1);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::BufferByteLength(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "buffer_byte_length", 1);
            }
            Expr::Uint8ArrayNew(size) => {
                if let Some(s) = size {
                    self.emit_expr(func, s);
                } else {
                    func.instruction(&f64_const(0.0));
                    func.instruction(&Instruction::I64ReinterpretF64);
                }
                self.emit_frame_begin(func, 1);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "uint8array_new", 1);
            }
            Expr::Uint8ArrayFrom(val) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, val);
                self.emit_memcall(func, "uint8array_from", 1);
            }
            Expr::Uint8ArrayLength(buf) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, buf);
                self.emit_memcall(func, "uint8array_length", 1);
            }
            Expr::Uint8ArrayGet { array, index } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_memcall(func, "uint8array_get", 2);
            }
            Expr::Uint8ArraySet { array, index, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, array);
                self.emit_store_arg(func, 1, index);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "uint8array_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Child process stubs ---
            Expr::ChildProcessExecSync { .. } | Expr::ChildProcessSpawnSync { .. } |
            Expr::ChildProcessSpawn { .. } | Expr::ChildProcessExec { .. } |
            Expr::ChildProcessSpawnBackground { .. } |
            Expr::ChildProcessGetProcessStatus(_) | Expr::ChildProcessKillProcess(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Fetch ---
            Expr::FetchWithOptions { url, method, body, headers } => {
                self.emit_expr(func, url);
                self.emit_expr(func, method);
                self.emit_expr(func, body);
                // Build headers object
                if headers.is_empty() {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                } else {
                    self.emit_frame_begin(func, 0);
                    self.emit_memcall(func, "object_new", 0);
                    for (key, val) in headers {
                        let key_id = self.emitter.string_map.get(key.as_str()).copied().unwrap_or(0);
                        let key_bits = (STRING_TAG << 48) | (key_id as u64);
                        self.emit_frame_begin(func, 3);
                        func.instruction(&Instruction::LocalSet(self.temp_local));
                        self.emit_slot_addr(func, 0);
                        func.instruction(&Instruction::LocalGet(self.temp_local));
                        func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                        self.emit_store_const(func, 1, f64::from_bits(key_bits));
                        self.emit_store_arg(func, 2, val);
                        self.emit_memcall(func, "object_set", 3);
                    }
                }
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchGetWithAuth { url, auth_header } => {
                self.emit_expr(func, url);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // method (default GET)
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64)); // body
                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self.emitter.string_map.get("Authorization").copied().unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            Expr::FetchPostWithAuth { url, auth_header, body } => {
                self.emit_expr(func, url);
                // POST method string
                let post_id = self.emitter.string_map.get("POST").copied().unwrap_or(0);
                let post_bits = (STRING_TAG << 48) | (post_id as u64);
                func.instruction(&Instruction::I64Const(post_bits as i64));
                self.emit_expr(func, body);
                // Build headers object with Authorization
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "object_new", 0);
                let auth_key_id = self.emitter.string_map.get("Authorization").copied().unwrap_or(0);
                let auth_key_bits = (STRING_TAG << 48) | (auth_key_id as u64);
                self.emit_frame_begin(func, 3);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_store_const(func, 1, f64::from_bits(auth_key_bits));
                self.emit_store_arg(func, 2, auth_header);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 1);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 0);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_slot_addr(func, 2);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "object_set", 3);
                self.emit_frame_begin(func, 4);
                func.instruction(&Instruction::LocalSet(self.temp_local));
                self.emit_slot_addr(func, 3);
                func.instruction(&Instruction::LocalGet(self.temp_local));
                func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                self.emit_memcall(func, "fetch_with_options", 4);
            }
            // --- Net stubs ---
            Expr::NetCreateServer { .. } | Expr::NetCreateConnection { .. } |
            Expr::NetConnect { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Crypto ---
            Expr::CryptoRandomUUID => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "crypto_random_uuid", 0);
            }
            Expr::CryptoRandomBytes(n) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, n);
                self.emit_memcall(func, "crypto_random_bytes", 1);
            }
            Expr::CryptoSha256(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_sha256", 1);
            }
            Expr::CryptoMd5(data) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, data);
                self.emit_memcall(func, "crypto_md5", 1);
            }
            // --- URL SearchParams ---
            Expr::UrlSearchParamsNew(init) => {
                if let Some(init_expr) = init {
                    self.emit_frame_begin(func, 1);
                    self.emit_store_arg(func, 0, init_expr);
                    self.emit_memcall(func, "url_parse", 1);
                    self.emit_frame_begin(func, 1);
                    func.instruction(&Instruction::LocalSet(self.temp_local));
                    self.emit_slot_addr(func, 0);
                    func.instruction(&Instruction::LocalGet(self.temp_local));
                    func.instruction(&Instruction::I64Store(wasm_encoder::MemArg { offset: 0, align: 3, memory_index: 0 }));
                    self.emit_memcall(func, "url_get_search_params", 1);
                } else {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }
            Expr::UrlSearchParamsGet { params, name } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall(func, "searchparams_get", 2);
            }
            Expr::UrlSearchParamsHas { params, name } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_i32(func, "searchparams_has", 2);
                func.instruction(&Instruction::If(wasm_encoder::BlockType::Result(ValType::I64)));
                func.instruction(&Instruction::I64Const(TAG_TRUE as i64));
                func.instruction(&Instruction::Else);
                func.instruction(&Instruction::I64Const(TAG_FALSE as i64));
                func.instruction(&Instruction::End);
            }
            Expr::UrlSearchParamsSet { params, name, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_set", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsAppend { params, name, value } => {
                self.emit_frame_begin(func, 3);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_store_arg(func, 2, value);
                self.emit_memcall_void(func, "searchparams_append", 3);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsDelete { params, name } => {
                self.emit_frame_begin(func, 2);
                self.emit_store_arg(func, 0, params);
                self.emit_store_arg(func, 1, name);
                self.emit_memcall_void(func, "searchparams_delete", 2);
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::UrlSearchParamsToString(params) => {
                self.emit_frame_begin(func, 1);
                self.emit_store_arg(func, 0, params);
                self.emit_memcall(func, "searchparams_to_string", 1);
            }
            Expr::UrlSearchParamsGetAll { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- JS runtime interop stubs ---
            Expr::JsLoadModule { .. } | Expr::JsGetExport { .. } |
            Expr::JsCallFunction { .. } | Expr::JsCallMethod { .. } |
            Expr::JsGetProperty { .. } | Expr::JsSetProperty { .. } |
            Expr::JsNew { .. } | Expr::JsNewFromHandle { .. } |
            Expr::JsCreateCallback { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            // --- Misc ---
            Expr::ImportMetaUrl(_) | Expr::StaticPluginResolve(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::Yield { .. } => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
            Expr::BigInt(_) | Expr::NativeModuleRef(_) => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }

            // --- DateNow ---
            Expr::DateNow => {
                self.emit_frame_begin(func, 0);
                self.emit_memcall(func, "date_now", 0);
            }

            // --- Sequence ---
            Expr::Sequence(exprs) => {
                for (i, e) in exprs.iter().enumerate() {
                    self.emit_expr(func, e);
                    if i < exprs.len() - 1 {
                        func.instruction(&Instruction::Drop);
                    }
                }
                if exprs.is_empty() {
                    func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
                }
            }

            // --- Catch-all: emit undefined ---
            _ => {
                func.instruction(&Instruction::I64Const(TAG_UNDEFINED as i64));
            }
        }
    }

    /// Check if an expression produces a value on the stack
    fn expr_has_value(&self, expr: &Expr) -> bool {
        match expr {
            Expr::NativeMethodCall { module, method, .. } => {
                let normalized = module.strip_prefix("node:").unwrap_or(module);
                if normalized == "console" {
                    return false;
                }
                // void-returning array methods via NativeMethodCall
                if matches!(method.as_str(), "forEach") {
                    return false;
                }
                true
            }
            // console.log/warn/error via Call + PropertyGet pattern
            Expr::Call { callee, .. } => {
                if let Expr::PropertyGet { object, property } = callee.as_ref() {
                    if let Expr::GlobalGet(_) = object.as_ref() {
                        if matches!(property.as_str(), "log" | "warn" | "error") {
                            return false;
                        }
                    }
                }
                true
            }
            // ArrayForEach returns undefined but we emit it explicitly
            _ => true,
        }
    }
}

/// Recursively scan statements for local variable declarations
/// Walk a module's init statements and assign WASM global indices to top-level Lets.
/// Module-level Lets are then accessible from any function in the same module via
/// the (mod_idx, LocalId) key.
fn collect_module_let_ids(
    stmts: &[Stmt],
    mod_idx: usize,
    map: &mut BTreeMap<(usize, LocalId), u32>,
    next_global: &mut u32,
) {
    for stmt in stmts {
        if let Stmt::Let { id, .. } = stmt {
            map.insert((mod_idx, *id), *next_global);
            *next_global += 1;
        }
    }
}

fn collect_locals(stmts: &[Stmt], map: &mut BTreeMap<LocalId, u32>, count: &mut u32, offset: u32) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { id, .. } => {
                if !map.contains_key(id) {
                    map.insert(*id, offset + *count);
                    *count += 1;
                }
            }
            Stmt::If { then_branch, else_branch, .. } => {
                collect_locals(then_branch, map, count, offset);
                if let Some(eb) = else_branch {
                    collect_locals(eb, map, count, offset);
                }
            }
            Stmt::While { body, .. } => {
                collect_locals(body, map, count, offset);
            }
            Stmt::For { init, body, .. } => {
                if let Some(init_stmt) = init {
                    collect_locals(std::slice::from_ref(init_stmt.as_ref()), map, count, offset);
                }
                collect_locals(body, map, count, offset);
            }
            Stmt::Try { body, catch, finally } => {
                collect_locals(body, map, count, offset);
                if let Some(c) = catch {
                    if let Some((id, _)) = &c.param {
                        if !map.contains_key(id) {
                            map.insert(*id, offset + *count);
                            *count += 1;
                        }
                    }
                    collect_locals(&c.body, map, count, offset);
                }
                if let Some(f) = finally {
                    collect_locals(f, map, count, offset);
                }
            }
            Stmt::Switch { cases, .. } => {
                for case in cases {
                    collect_locals(&case.body, map, count, offset);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect all Expr::Closure nodes from statements
fn collect_closures_from_stmts(
    stmts: &[Stmt],
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    for stmt in stmts {
        match stmt {
            Stmt::Let { init, .. } => {
                if let Some(e) = init { collect_closures_from_expr(e, out); }
            }
            Stmt::Expr(e) | Stmt::Throw(e) => collect_closures_from_expr(e, out),
            Stmt::Return(e) => {
                if let Some(e) = e { collect_closures_from_expr(e, out); }
            }
            Stmt::If { condition, then_branch, else_branch } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(then_branch, out);
                if let Some(eb) = else_branch { collect_closures_from_stmts(eb, out); }
            }
            Stmt::While { condition, body } => {
                collect_closures_from_expr(condition, out);
                collect_closures_from_stmts(body, out);
            }
            Stmt::For { init, condition, update, body } => {
                if let Some(i) = init { collect_closures_from_stmts(std::slice::from_ref(i.as_ref()), out); }
                if let Some(c) = condition { collect_closures_from_expr(c, out); }
                if let Some(u) = update { collect_closures_from_expr(u, out); }
                collect_closures_from_stmts(body, out);
            }
            Stmt::Try { body, catch, finally } => {
                collect_closures_from_stmts(body, out);
                if let Some(c) = catch { collect_closures_from_stmts(&c.body, out); }
                if let Some(f) = finally { collect_closures_from_stmts(f, out); }
            }
            Stmt::Switch { discriminant, cases } => {
                collect_closures_from_expr(discriminant, out);
                for case in cases {
                    if let Some(t) = &case.test { collect_closures_from_expr(t, out); }
                    collect_closures_from_stmts(&case.body, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect Expr::Closure from an expression tree
fn collect_closures_from_expr(
    expr: &Expr,
    out: &mut Vec<(FuncId, Vec<Param>, Vec<Stmt>, Vec<LocalId>, Vec<LocalId>)>,
) {
    match expr {
        Expr::Closure { func_id, params, body, captures, mutable_captures, .. } => {
            out.push((*func_id, params.clone(), body.clone(), captures.clone(), mutable_captures.clone()));
            // Also collect nested closures
            collect_closures_from_stmts(body, out);
        }
        Expr::Call { callee, args, .. } => {
            collect_closures_from_expr(callee, out);
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::Binary { left, right, .. } | Expr::Compare { left, right, .. } |
        Expr::Logical { left, right, .. } => {
            collect_closures_from_expr(left, out);
            collect_closures_from_expr(right, out);
        }
        Expr::Unary { operand, .. } | Expr::TypeOf(operand) | Expr::Void(operand) |
        Expr::Await(operand) => {
            collect_closures_from_expr(operand, out);
        }
        Expr::LocalSet(_, val) | Expr::GlobalSet(_, val) => {
            collect_closures_from_expr(val, out);
        }
        Expr::Conditional { condition, then_expr, else_expr } => {
            collect_closures_from_expr(condition, out);
            collect_closures_from_expr(then_expr, out);
            collect_closures_from_expr(else_expr, out);
        }
        Expr::Object(fields) => {
            for (_, v) in fields { collect_closures_from_expr(v, out); }
        }
        Expr::Array(elems) => {
            for e in elems { collect_closures_from_expr(e, out); }
        }
        Expr::PropertyGet { object, .. } => { collect_closures_from_expr(object, out); }
        Expr::PropertySet { object, value, .. } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(value, out);
        }
        Expr::IndexGet { object, index } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
        }
        Expr::IndexSet { object, index, value } => {
            collect_closures_from_expr(object, out);
            collect_closures_from_expr(index, out);
            collect_closures_from_expr(value, out);
        }
        Expr::NativeMethodCall { args, object, .. } => {
            if let Some(o) = object { collect_closures_from_expr(o, out); }
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::New { args, .. } => {
            for a in args { collect_closures_from_expr(a, out); }
        }
        Expr::ArrayMap { array, callback } | Expr::ArrayFilter { array, callback } |
        Expr::ArrayForEach { array, callback } | Expr::ArrayFind { array, callback } |
        Expr::ArrayFindIndex { array, callback } | Expr::ArraySort { array, comparator: callback } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
        }
        Expr::ArrayReduce { array, callback, initial } | Expr::ArrayReduceRight { array, callback, initial } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(callback, out);
            if let Some(i) = initial { collect_closures_from_expr(i, out); }
        }
        Expr::ArrayToSorted { array, comparator } => {
            collect_closures_from_expr(array, out);
            if let Some(c) = comparator { collect_closures_from_expr(c, out); }
        }
        Expr::ArrayToSpliced { array, start, delete_count, items } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(start, out);
            collect_closures_from_expr(delete_count, out);
            for item in items { collect_closures_from_expr(item, out); }
        }
        Expr::ArrayWith { array, index, value } => {
            collect_closures_from_expr(array, out);
            collect_closures_from_expr(index, out);
            collect_closures_from_expr(value, out);
        }
        Expr::ArrayCopyWithin { target, start, end, .. } => {
            collect_closures_from_expr(target, out);
            collect_closures_from_expr(start, out);
            if let Some(e) = end { collect_closures_from_expr(e, out); }
        }
        Expr::ArrayToReversed { array } => {
            collect_closures_from_expr(array, out);
        }
        Expr::ArrayEntries(array) | Expr::ArrayKeys(array) | Expr::ArrayValues(array) => {
            collect_closures_from_expr(array, out);
        }
        Expr::Sequence(exprs) => {
            for e in exprs { collect_closures_from_expr(e, out); }
        }
        _ => {}
    }
}

/// Check if a statement or its children contain a return statement
fn has_return(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(_) => true,
        Stmt::If { then_branch, else_branch, .. } => {
            then_branch.iter().any(has_return) ||
            else_branch.as_ref().map_or(false, |eb| eb.iter().any(has_return))
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            body.iter().any(has_return)
        }
        Stmt::Try { body, catch, finally } => {
            body.iter().any(has_return) ||
            catch.as_ref().map_or(false, |c| c.body.iter().any(has_return)) ||
            finally.as_ref().map_or(false, |f| f.iter().any(has_return))
        }
        Stmt::Switch { cases, .. } => {
            cases.iter().any(|c| c.body.iter().any(has_return))
        }
        _ => false,
    }
}
