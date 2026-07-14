use super::*;
use std::fmt::Write as FmtWrite;

impl JsEmitter {
    pub(super) fn emit_console_call(&mut self, method: &str, args: &[Expr]) {
        let _ = write!(self.output, "console.{}(", method);
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
        }
        self.output.push(')');
    }

    pub(super) fn emit_system_method_call(&mut self, method: &str, args: &[Expr]) {
        match method {
            "openURL" | "open_url" => {
                self.output.push_str("window.open(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(", '_blank')");
            }
            "isDarkMode" | "is_dark_mode" => {
                self.output.push_str("(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 1.0 : 0.0)");
            }
            "preferencesGet" | "preferences_get" => {
                self.output.push_str("(localStorage.getItem(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(") || '')");
            }
            "preferencesSet" | "preferences_set" => {
                self.output.push_str("localStorage.setItem(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push_str(", ");
                if let Some(a) = args.get(1) {
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            "hapticPlay" | "haptic_play" => {
                self.output.push_str("perry_system_haptic_play(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            "audioStart" | "audio_start" => {
                self.output.push_str("perry_system_audio_start()");
            }
            "audioStop" | "audio_stop" => {
                self.output.push_str("perry_system_audio_stop()");
            }
            "audioGetLevel" | "audio_get_level" => {
                self.output.push_str("perry_system_audio_get_level()");
            }
            "audioGetPeak" | "audio_get_peak" => {
                self.output.push_str("perry_system_audio_get_peak()");
            }
            "audioGetWaveformSamples" | "audio_get_waveform" => {
                self.output.push_str("perry_system_audio_get_waveform(");
                if let Some(a) = args.first() {
                    self.emit_expr(a);
                }
                self.output.push(')');
            }
            "getDeviceModel" | "get_device_model" => {
                self.output.push_str("perry_system_get_device_model()");
            }
            "getAppVersion" | "get_app_version" => {
                let s = self.quote_string(&self.app_metadata.version.clone());
                self.output.push_str(&s);
            }
            "getAppBuildNumber" | "get_app_build_number" => {
                let _ = write!(self.output, "{}", self.app_metadata.build_number);
            }
            "getBundleId" | "get_bundle_id" => {
                let s = self.quote_string(&self.app_metadata.bundle_id.clone());
                self.output.push_str(&s);
            }
            _ => {
                let _ = write!(
                    self.output,
                    "console.warn('perry/system.{} not available in browser')",
                    method
                );
            }
        }
    }

    /// Emit a `perry/audio` (issue #1867) method call: look up the
    /// method in `PERRY_AUDIO_TABLE` and emit `__perry.<runtime>(args...)`.
    /// Argument coercion is purely positional — the dispatch table
    /// guarantees uniform `f64` / `string` / `closure` shapes.
    pub(super) fn emit_audio_method_call(&mut self, method: &str, args: &[Expr]) {
        let rt = match perry_dispatch::perry_audio_lookup(method) {
            Some(row) => row.runtime,
            None => {
                let _ = write!(
                    self.output,
                    "console.warn('perry/audio.{} not available')",
                    method
                );
                return;
            }
        };
        let _ = write!(self.output, "__perry.{}(", rt);
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
        }
        self.output.push(')');
    }

    pub(super) fn emit_ui_method_call(
        &mut self,
        class_name: Option<&str>,
        object: Option<&Expr>,
        method: &str,
        args: &[Expr],
    ) {
        // Map perry/ui methods to __perry.perry_ui_* calls
        let ui_fn = match method {
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
            "scrollViewSetChild" => "perry_ui_scrollview_set_child",
            "scrollViewScrollTo" => "perry_ui_scrollview_scroll_to",
            "scrollViewGetOffset" => "perry_ui_scrollview_get_offset",
            "scrollViewSetOffset" => "perry_ui_scrollview_set_offset",
            "Spacer" | "spacer_create" => "perry_ui_spacer_create",
            "Divider" | "divider_create" => "perry_ui_divider_create",
            "ProgressView" | "progressview_create" => "perry_ui_progressview_create",
            "Image" | "image_create" => "perry_ui_image_create",
            "Picker" | "picker_create" => "perry_ui_picker_create",
            // Table (issue #192)
            "Table" | "table_create" => "perry_ui_table_create",
            "tableSetColumnHeader" => "perry_ui_table_set_column_header",
            "tableSetColumnWidth" => "perry_ui_table_set_column_width",
            "tableUpdateRowCount" => "perry_ui_table_update_row_count",
            "tableSetOnRowSelect" => "perry_ui_table_set_on_row_select",
            "tableGetSelectedRow" => "perry_ui_table_get_selected_row",
            // Camera (issue #191) — browser stubs in wasm_runtime.js return
            // 0 / -1 since there's no DOM equivalent for live capture; the
            // dispatch entries here exist so the JS-target compile resolves
            // the names rather than emitting `perry_ui_unknown`.
            "CameraView" | "camera_create" => "perry_ui_camera_create",
            "cameraStart" => "perry_ui_camera_start",
            "cameraStop" => "perry_ui_camera_stop",
            "cameraFreeze" => "perry_ui_camera_freeze",
            "cameraUnfreeze" => "perry_ui_camera_unfreeze",
            "cameraSampleColor" => "perry_ui_camera_sample_color",
            "cameraSetOnTap" => "perry_ui_camera_set_on_tap",
            "Form" | "form_create" => "perry_ui_form_create",
            "Section" | "section_create" => "perry_ui_section_create",
            "NavigationStack" | "navigationstack_create" => "perry_ui_navigationstack_create",
            "Canvas" | "canvas_create" => "perry_ui_canvas_create",
            // Child management
            "addChild" | "widget_add_child" => "perry_ui_widget_add_child",
            "removeAllChildren" | "widget_remove_all_children" => {
                "perry_ui_widget_remove_all_children"
            }
            // Styling
            "setBackground" | "set_background" => "perry_ui_set_background",
            "setForeground" | "set_foreground" => "perry_ui_set_foreground",
            "setFontSize" | "set_font_size" => "perry_ui_set_font_size",
            "setFontWeight" | "set_font_weight" => "perry_ui_set_font_weight",
            "setFontFamily" | "set_font_family" => "perry_ui_set_font_family",
            "setPadding" | "set_padding" => "perry_ui_set_padding",
            "setFrame" | "set_frame" => "perry_ui_set_frame",
            "setCornerRadius" | "set_corner_radius" => "perry_ui_set_corner_radius",
            "setBorder" | "set_border" => "perry_ui_set_border",
            "setOpacity" | "set_opacity" => "perry_ui_set_opacity",
            "setEnabled" | "set_enabled" => "perry_ui_set_enabled",
            "setTooltip" | "set_tooltip" => "perry_ui_set_tooltip",
            "setControlSize" | "set_control_size" => "perry_ui_set_control_size",
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
            "createState" | "state_create" => {
                self.output.push_str("__perry.stateCreate(");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.output.push_str(", ");
                    }
                    self.emit_expr(arg);
                }
                self.output.push(')');
                return;
            }
            "get" if class_name == Some("State") => {
                self.output.push_str("__perry.stateGet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                self.output.push(')');
                return;
            }
            "set" if class_name == Some("State") => {
                self.output.push_str("__perry.stateSet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                for arg in args {
                    self.output.push_str(", ");
                    self.emit_expr(arg);
                }
                self.output.push(')');
                return;
            }
            "value" if class_name == Some("State") => {
                self.output.push_str("__perry.stateGet(");
                if let Some(obj) = object {
                    self.emit_expr(obj);
                }
                self.output.push(')');
                return;
            }
            // Issue #1392 — `state<T>` desugar synthetic API (keyed registry).
            // state_desugar (crates/perry-transform/src/state_desugar.rs) emits
            // these on non-arkts targets. The native LLVM backend special-cases
            // them in lower_call/native/mod.rs; on `--target web` they must reach
            // the keyed-state bridge functions in web_runtime.js or reactive
            // `state` / `setText` silently no-op (displayed text never updates).
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
            "onChange" | "state_on_change" => "perry_ui_state_on_change",
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
            // Hone IDE camelCase free-function imports.
            // #853: `"textSetColor"` previously had two arms — the first
            // mapped to `perry_ui_set_foreground` (which has never existed
            // as an FFI symbol), shadowing the correct mapping below. The
            // dead first arm was deleted; the canonical mapping lives later
            // in this block alongside the other `text*` entries.
            "textSetFontSize" => "perry_ui_set_font_size",
            "textSetFontWeight" => "perry_ui_set_font_weight",
            "textSetFontFamily" => "perry_ui_set_font_family",
            "textSetString" => "perry_ui_text_set_string",
            "buttonSetBordered" => "perry_ui_button_set_bordered",
            "buttonSetTextColor" => "perry_ui_button_set_text_color",
            "buttonSetTitle" => "perry_ui_button_set_title",
            "buttonSetImage" => "perry_ui_button_set_image",
            "buttonSetContentTintColor" => "perry_ui_button_set_content_tint_color",
            "widgetSetBackgroundColor" => "perry_ui_set_background",
            "widgetAddChild" => "perry_ui_widget_add_child",
            "widgetRemoveChild" => "perry_ui_widget_remove_child",
            "widgetReorderChild" => "perry_ui_widget_reorder_child",
            "widgetClearChildren" => "perry_ui_widget_remove_all_children",
            "widgetSetWidth" => "perry_ui_widget_set_width",
            "widgetSetHeight" => "perry_ui_widget_set_height",
            "widgetSetHugging" => "perry_ui_widget_set_hugging",
            "widgetSetHidden" => "perry_ui_set_widget_hidden",
            "widgetMatchParentHeight" => "perry_ui_widget_match_parent_height",
            "widgetMatchParentWidth" => "perry_ui_widget_match_parent_width",
            "widgetAddOverlay" => "perry_ui_widget_add_overlay",
            "widgetSetOverlayFrame" => "perry_ui_widget_set_overlay_frame",
            "widgetSetEdgeInsets" => "perry_ui_widget_set_edge_insets",
            "widgetSetContextMenu" => "perry_ui_widget_set_context_menu",
            "stackSetDetachesHidden" => "perry_ui_stack_set_detaches_hidden",
            "stackSetDistribution" => "perry_ui_stack_set_distribution",
            "buttonSetImagePosition" => "perry_ui_button_set_image_position",
            "textSetColor" => "perry_ui_text_set_color",
            "textSetTextAlignment" => "perry_ui_text_set_text_alignment",
            "textSetWraps" => "perry_ui_text_set_wraps",
            "textfieldSetString" => "perry_ui_textfield_set_string",
            "textfieldGetString" => "perry_ui_textfield_get_string",
            "textfieldFocus" => "perry_ui_textfield_focus",
            "textfieldBlurAll" => "perry_ui_textfield_blur_all",
            "textfieldSetOnSubmit" => "perry_ui_textfield_set_on_submit",
            "textfieldSetOnFocus" => "perry_ui_textfield_set_on_focus",
            "pollOpenFile" => "perry_ui_poll_open_file",
            "frameSplitCreate" => "perry_ui_frame_split_create",
            "frameSplitAddChild" => "perry_ui_frame_split_add_child",
            "saveFileDialog" => "perry_ui_save_file_dialog",
            "VStackWithInsets" => "perry_ui_vstack_create_with_insets",
            "HStackWithInsets" => "perry_ui_hstack_create_with_insets",
            "embedNSView" => "perry_ui_embed_ns_view",
            "openFolderDialog" => "perry_ui_open_folder_dialog",
            "openFileDialog" => "perry_ui_open_file_dialog",
            // App lifecycle
            "run" | "app_run" => "perry_ui_app_run",
            // Menu
            "menuCreate" | "menu_create" => "perry_ui_menu_create",
            "menuAddItem" | "menu_add_item" => "perry_ui_menu_add_item",
            "menuAddStandardAction" | "menu_add_standard_action" => {
                "perry_ui_menu_add_standard_action"
            }
            "menuClear" | "menu_clear" => "perry_ui_menu_clear",
            "menuAddSeparator" | "menu_add_separator" => "perry_ui_menu_add_separator",
            "menuAddSubmenu" | "menu_add_submenu" => "perry_ui_menu_add_submenu",
            "menuBarCreate" | "menubar_create" => "perry_ui_menubar_create",
            "menuBarAddMenu" | "menubar_add_menu" => "perry_ui_menubar_add_menu",
            "menuBarAttach" | "menubar_attach" => "perry_ui_menubar_attach",
            // Default — try the centralised perry-dispatch tables first
            // (Tier 1.3, v0.5.332). PERRY_UI_TABLE / PERRY_UI_INSTANCE_TABLE
            // / PERRY_SYSTEM_TABLE are the canonical source of truth, so
            // any new perry/ui or perry/system method added there resolves
            // on `--target web` without a parallel edit here. The static
            // arms above are kept for legacy snake_case aliases (`app_create`,
            // `vstack_create`, …) that aren't in PERRY_UI_TABLE because
            // the LLVM backend only expects the canonical camelCase names.
            _ => {
                if let Some(rt) = perry_dispatch::ui_method_to_runtime(method) {
                    rt
                } else {
                    // Last-resort fallback: emit as __perry.perry_ui_<method>(...).
                    // The browser-side runtime will throw "function not found"
                    // at call time if the symbol doesn't exist — this preserves
                    // the pre-1.3 best-effort behavior for unknown methods.
                    let _ = write!(self.output, "__perry.perry_ui_{}(", method);
                    if let Some(obj) = object {
                        self.emit_expr(obj);
                        if !args.is_empty() {
                            self.output.push_str(", ");
                        }
                    }
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            self.output.push_str(", ");
                        }
                        self.emit_expr(arg);
                    }
                    self.output.push(')');
                    return;
                }
            }
        };

        // Emit the __perry.fn_name(object?, args...) call
        let _ = write!(self.output, "__perry.{}(", ui_fn);
        let mut first = true;
        if let Some(obj) = object {
            self.emit_expr(obj);
            first = false;
        }
        for arg in args {
            if !first {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
            first = false;
        }
        self.output.push(')');
    }

    // --- Helpers ---

    pub(super) fn emit_math_unary(&mut self, func: &str, arg: &Expr) {
        self.output.push_str(func);
        self.output.push('(');
        self.emit_expr(arg);
        self.output.push(')');
    }

    pub(super) fn emit_math_variadic(&mut self, func: &str, args: &[Expr]) {
        self.output.push_str(func);
        self.output.push('(');
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.output.push_str(", ");
            }
            self.emit_expr(arg);
        }
        self.output.push(')');
    }

    pub(super) fn quote_string(&self, s: &str) -> String {
        let mut result = String::with_capacity(s.len() + 2);
        result.push('"');
        for ch in s.chars() {
            match ch {
                '"' => result.push_str("\\\""),
                '\\' => result.push_str("\\\\"),
                '\n' => result.push_str("\\n"),
                '\r' => result.push_str("\\r"),
                '\t' => result.push_str("\\t"),
                '\0' => result.push_str("\\0"),
                c if c < ' ' => {
                    let _ = write!(result, "\\x{:02x}", c as u32);
                }
                c => result.push(c),
            }
        }
        result.push('"');
        result
    }
}
