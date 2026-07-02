//! `map_ui_method`: maps perry/ui and perry/system method names to bridge
//! function names. Pure code-movement from `mod.rs`.

#[allow(unused_imports)]
use super::*;

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
        // #1546: showToast(msg) renders a bottom-of-page toast on web —
        // previously was a no-op (only HarmonyOS routed
        // `promptAction.showToast` through the runtime drain queue).
        // Now wires to a JS-side fade-in/out div in wasm_runtime.js.
        "showToast" => "perry_ui_show_toast",
        // Issue #1865: display-link frame callbacks. On web these map to
        // requestAnimationFrame / cancelAnimationFrame via wasm_runtime.js.
        "onFrame" => "perry_ui_on_frame",
        "cancelFrame" => "perry_ui_cancel_frame",
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
        // Issue #1392 — `state<T>` desugar synthetic API (keyed registry).
        // state_desugar (crates/perry-transform/src/state_desugar.rs) emits
        // these on non-arkts targets. The native LLVM backend special-cases
        // them in lower_call/native/mod.rs; on web/wasm they must reach the
        // keyed-state bridge functions in wasm_runtime.js or reactive `state`
        // / `setText` silently no-op (the displayed text never updates).
        "__state_init" => "perry_ui_state_init",
        "__state_get" => "perry_ui_state_get_keyed",
        "__state_set" => "perry_ui_state_set_keyed",
        "__foreach_register" => "perry_ui_foreach_register",
        "__navstack_register_route" => "perry_ui_navstack_register_route",
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
        "drawImage" | "canvas_draw_image" => "perry_ui_canvas_draw_image",
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
        "registerGlobalHotkey" => "perry_ui_register_global_hotkey",
        // Continuous keyboard events (issue #1864)
        "onKeyDown" => "perry_ui_widget_set_on_key_down",
        "onKeyUp" => "perry_ui_widget_set_on_key_up",
        "onAppKeyDown" => "perry_ui_app_set_on_key_down",
        "onAppKeyUp" => "perry_ui_app_set_on_key_up",
        "focus" => "perry_ui_focus_widget",
        "blur" => "perry_ui_blur_widget",
        "isKeyDown" => "perry_ui_is_key_down",
        "currentModifiers" => "perry_ui_current_modifiers",
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
        "appSetActivationPolicy" => "perry_ui_app_set_activation_policy",
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
