/// Cross-platform feature parity matrix for Perry UI.
///
/// This is the single source of truth for which `perry_ui_*` / `perry_system_*`
/// FFI functions each platform is expected to provide. Tests in `tests/ffi_parity.rs`
/// verify actual source code against this matrix.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Platform fully implements this function.
    Supported,
    /// Platform has a stub (compiles but does nothing useful).
    Stub,
    /// Platform does not implement this function.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    AppLifecycle,
    WidgetCreation,
    ChildManagement,
    StateSystem,
    StateBind,
    TextStyling,
    ButtonOps,
    TextFieldOps,
    ScrollView,
    Styling,
    Canvas,
    Menu,
    Clipboard,
    Dialog,
    KeyboardShortcut,
    Events,
    Animation,
    SystemApi,
    Advanced,
    Timer,
    Layout,
    ForEach,
    Navigation,
    Picker,
    Image,
    ProgressView,
}

pub struct Feature {
    /// Canonical function name (matches macOS/iOS FFI naming).
    pub name: &'static str,
    pub category: Category,
    pub macos: Support,
    pub ios: Support,
    pub android: Support,
    pub gtk4: Support,
    pub windows: Support,
    pub web: Support,
    /// If the web runtime uses a different function name, specify it here.
    /// When `None` and `web == Supported`, the web symbol matches `name`.
    pub web_name: Option<&'static str>,
}

use Category::*;
use Support::*;

// Shorthand aliases for the matrix below
const S: Support = Supported;
const U: Support = Unsupported;

/// Complete feature matrix. Every `perry_ui_*` / `perry_system_*` function across
/// all platforms is listed here. The macOS naming convention is canonical.
///
/// # Conventions
/// - `web_name: Some("...")` when the web runtime uses a different JS function name
/// - `web_name: None` when web uses the same name as native, or when web is Unsupported
pub const FEATURES: &[Feature] = &[
    // ── AppLifecycle ──────────────────────────────────────────────────────
    Feature { name: "perry_ui_app_create",       category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_run",          category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_set_body",     category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_set_min_size", category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_set_max_size", category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_on_activate",  category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_app_on_terminate", category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_window_create",    category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_window_set_body",  category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_window_show",      category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_window_close",     category: AppLifecycle, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Timer ─────────────────────────────────────────────────────────────
    Feature { name: "perry_ui_app_set_timer", category: Timer, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Widget Creation ──────────────────────────────────────────────────
    Feature { name: "perry_ui_text_create",          category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_button_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_vstack_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_hstack_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_spacer_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_divider_create",       category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_slider_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_toggle_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_textfield_create",     category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_securefield_create",   category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_scrollview_create",    category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_form_create",          category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_section_create",       category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_zstack_create",        category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_lazyvstack_create",    category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_lazyvstack_update",    category: WidgetCreation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_table_create",         category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_table_set_column_header", category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_table_set_column_width",  category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_table_update_row_count",  category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_table_set_on_row_select", category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_table_get_selected_row",  category: WidgetCreation, macos: S, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },

    // ── Child Management ─────────────────────────────────────────────────
    Feature { name: "perry_ui_widget_add_child",    category: ChildManagement, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_widget_add_child_at", category: ChildManagement, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_widget_clear_children", category: ChildManagement, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_widget_remove_all_children") },

    // ── State System ─────────────────────────────────────────────────────
    Feature { name: "perry_ui_state_create",    category: StateSystem, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_get",       category: StateSystem, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_set",       category: StateSystem, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_on_change", category: StateSystem, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── State Bindings ───────────────────────────────────────────────────
    Feature { name: "perry_ui_state_bind_slider",        category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_bind_text_numeric",  category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_bind_text_template", category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_state_bind_text") },
    Feature { name: "perry_ui_state_bind_textfield",     category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_bind_toggle",        category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_state_bind_visibility",    category: StateBind, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Text Styling ─────────────────────────────────────────────────────
    Feature { name: "perry_ui_text_set_string",      category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_text_set_color",       category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_foreground") },
    Feature { name: "perry_ui_text_set_font_size",   category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_font_size") },
    Feature { name: "perry_ui_text_set_font_weight", category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_font_weight") },
    Feature { name: "perry_ui_text_set_selectable",  category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_text_set_font_family", category: TextStyling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_font_family") },

    // ── Button Ops ───────────────────────────────────────────────────────
    Feature { name: "perry_ui_button_set_bordered",           category: ButtonOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_button_set_title",              category: ButtonOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_button_set_content_tint_color", category: ButtonOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_button_set_image_position",     category: ButtonOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── TextField Ops ────────────────────────────────────────────────────
    Feature { name: "perry_ui_textfield_focus",      category: TextFieldOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_textfield_set_string", category: TextFieldOps, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── ScrollView ───────────────────────────────────────────────────────
    Feature { name: "perry_ui_scrollview_set_child",  category: ScrollView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_scrollview_scroll_to",  category: ScrollView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_scrollview_get_offset", category: ScrollView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_scrollview_set_offset", category: ScrollView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Styling ──────────────────────────────────────────────────────────
    Feature { name: "perry_ui_widget_set_background_color",    category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_background") },
    Feature { name: "perry_ui_widget_set_background_gradient", category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_widget_set_corner_radius",       category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_corner_radius") },
    Feature { name: "perry_ui_widget_set_context_menu",        category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_widget_set_control_size",        category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_control_size") },
    Feature { name: "perry_ui_widget_set_enabled",             category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_enabled") },
    Feature { name: "perry_ui_widget_set_tooltip",             category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_tooltip") },
    Feature { name: "perry_ui_set_widget_hidden",              category: Styling, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Canvas ───────────────────────────────────────────────────────────
    Feature { name: "perry_ui_canvas_clear",         category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_canvas_clear_rect") },
    Feature { name: "perry_ui_canvas_begin_path",    category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_move_to",       category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_line_to",       category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_stroke",        category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_fill_gradient", category: Canvas, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Menu ─────────────────────────────────────────────────────────────
    Feature { name: "perry_ui_menu_create",                category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_add_item",              category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_add_item_with_shortcut", category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_add_separator",         category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_add_submenu",           category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menubar_create",             category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menubar_add_menu",           category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menubar_attach",             category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_clear",                 category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_menu_add_standard_action",   category: Menu, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Clipboard ────────────────────────────────────────────────────────
    Feature { name: "perry_ui_clipboard_read",  category: Clipboard, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_clipboard_write", category: Clipboard, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Dialog ───────────────────────────────────────────────────────────
    Feature { name: "perry_ui_open_file_dialog", category: Dialog, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_save_file_dialog", category: Dialog, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_alert",            category: Dialog, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Keyboard Shortcut ────────────────────────────────────────────────
    Feature { name: "perry_ui_add_keyboard_shortcut", category: KeyboardShortcut, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Events ───────────────────────────────────────────────────────────
    Feature { name: "perry_ui_widget_set_on_hover",        category: Events, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_on_hover") },
    Feature { name: "perry_ui_widget_set_on_double_click", category: Events, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_set_on_double_click") },
    Feature { name: "perry_ui_widget_set_on_click",        category: Events, macos: S, ios: S, android: S, gtk4: S, windows: S, web: U, web_name: None },

    // ── Animation ────────────────────────────────────────────────────────
    Feature { name: "perry_ui_widget_animate_opacity",  category: Animation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_animate_opacity") },
    Feature { name: "perry_ui_widget_animate_position", category: Animation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_animate_position") },

    // ── Layout ───────────────────────────────────────────────────────────
    Feature { name: "perry_ui_vstack_create_with_insets",   category: Layout, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_hstack_create_with_insets",   category: Layout, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_stack_set_detaches_hidden",   category: Layout, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── ForEach ──────────────────────────────────────────────────────────
    Feature { name: "perry_ui_for_each_init", category: ForEach, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_state_bind_foreach") },

    // ── Navigation ───────────────────────────────────────────────────────
    Feature { name: "perry_ui_navstack_create", category: Navigation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_navigationstack_create") },
    Feature { name: "perry_ui_navstack_push",   category: Navigation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_navstack_pop",    category: Navigation, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Picker ───────────────────────────────────────────────────────────
    Feature { name: "perry_ui_picker_create",       category: Picker, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_picker_add_item",     category: Picker, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_picker_set_selected", category: Picker, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_picker_get_selected", category: Picker, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Image ────────────────────────────────────────────────────────────
    Feature { name: "perry_ui_image_create_file",   category: Image, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: Some("perry_ui_image_create") },
    Feature { name: "perry_ui_image_create_symbol", category: Image, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_image_set_size",      category: Image, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_image_set_tint",      category: Image, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── ProgressView ─────────────────────────────────────────────────────
    Feature { name: "perry_ui_progressview_create",    category: ProgressView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_progressview_set_value", category: ProgressView, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── Advanced (Sheet, Toolbar) ────────────────────────────────────────
    Feature { name: "perry_ui_sheet_create",  category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_sheet_present", category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_sheet_dismiss", category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_toolbar_create",   category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_toolbar_add_item", category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_ui_toolbar_attach",   category: Advanced, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },

    // ── System API ───────────────────────────────────────────────────────
    Feature { name: "perry_system_open_url",           category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_is_dark_mode",       category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_preferences_set",    category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_preferences_get",    category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_keychain_save",      category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_keychain_get",       category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_keychain_delete",    category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_notification_send",  category: SystemApi, macos: S, ios: S, android: S, gtk4: S, windows: S, web: S, web_name: None },
    Feature { name: "perry_system_notification_register_remote", category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_on_receive",      category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_schedule_interval", category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_schedule_calendar", category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_schedule_location", category: SystemApi, macos: U, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_cancel",            category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },
    Feature { name: "perry_system_notification_on_tap",            category: SystemApi, macos: S, ios: S, android: U, gtk4: U, windows: U, web: U, web_name: None },

    // ── Web-Only Functions ───────────────────────────────────────────────
    // These exist only in the web runtime and have no native equivalent.
    Feature { name: "perry_ui_set_border",             category: Styling,  macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_set_frame",              category: Styling,  macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_set_on_click",           category: Events,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_set_opacity",            category: Styling,  macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_set_padding",            category: Styling,  macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_arc",             category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_close_path",      category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_fill",            category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_fill_rect",       category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_fill_text",       category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_set_fill_color",  category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_set_font",        category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_set_line_width",  category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_set_stroke_color", category: Canvas,  macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
    Feature { name: "perry_ui_canvas_stroke_rect",     category: Canvas,   macos: U, ios: U, android: U, gtk4: U, windows: U, web: S, web_name: None },
];

/// Returns features filtered by category, sorted by name.
pub fn features_by_category(cat: Category) -> Vec<&'static Feature> {
    let mut v: Vec<_> = FEATURES.iter().filter(|f| f.category == cat).collect();
    v.sort_by_key(|f| f.name);
    v
}

/// All categories in display order.
pub const CATEGORY_ORDER: &[Category] = &[
    AppLifecycle, Timer, WidgetCreation, ChildManagement,
    StateSystem, StateBind, TextStyling, ButtonOps, TextFieldOps,
    ScrollView, Styling, Canvas, Menu, Clipboard, Dialog,
    KeyboardShortcut, Events, Animation, Layout, ForEach,
    Navigation, Picker, Image, ProgressView, Advanced, SystemApi,
];

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AppLifecycle => "App Lifecycle",
            Timer => "Timer",
            WidgetCreation => "Widget Creation",
            ChildManagement => "Child Management",
            StateSystem => "State System",
            StateBind => "State Bindings",
            TextStyling => "Text Styling",
            ButtonOps => "Button Ops",
            TextFieldOps => "TextField Ops",
            ScrollView => "ScrollView",
            Styling => "Styling",
            Canvas => "Canvas",
            Menu => "Menu",
            Clipboard => "Clipboard",
            Dialog => "Dialog",
            KeyboardShortcut => "Keyboard Shortcut",
            Events => "Events",
            Animation => "Animation",
            Layout => "Layout",
            ForEach => "ForEach",
            Navigation => "Navigation",
            Picker => "Picker",
            Image => "Image",
            ProgressView => "ProgressView",
            Advanced => "Advanced",
            SystemApi => "System API",
        })
    }
}
