//! `PERRY_UI_TABLE` rows, part B. Split out of ui_table.rs to satisfy the
//! 2000-line file-size gate; concatenated at compile time in the parent.

use crate::{ArgKind, MethodRow, ReturnKind};

pub(crate) const PERRY_UI_TABLE_PART_B: &[MethodRow] = &[
    // ---- Chart (issue #474) ----
    MethodRow {
        method: "Chart",
        runtime: "perry_ui_chart_create",
        args: &[ArgKind::I64Raw, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "chartAddDataPoint",
        runtime: "perry_ui_chart_add_data_point",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "chartClearData",
        runtime: "perry_ui_chart_clear_data",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "chartSetTitle",
        runtime: "perry_ui_chart_set_title",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "chartReload",
        runtime: "perry_ui_chart_reload",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- Command palette (issue #477) ----
    MethodRow {
        method: "commandPaletteRegister",
        runtime: "perry_ui_command_palette_register",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "commandPaletteUnregister",
        runtime: "perry_ui_command_palette_unregister",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "commandPaletteClear",
        runtime: "perry_ui_command_palette_clear",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "commandPaletteShow",
        runtime: "perry_ui_command_palette_show",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "commandPaletteHide",
        runtime: "perry_ui_command_palette_hide",
        args: &[],
        ret: ReturnKind::Void,
    },
    // ---- MapView (issue #517) ----
    MethodRow {
        method: "MapView",
        runtime: "perry_ui_map_view_create",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "mapViewSetRegion",
        runtime: "perry_ui_map_view_set_region",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "mapViewAddPin",
        runtime: "perry_ui_map_view_add_pin",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "mapViewClearPins",
        runtime: "perry_ui_map_view_clear_pins",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "mapViewSetMapType",
        runtime: "perry_ui_map_view_set_map_type",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- PdfView (issue #516) ----
    MethodRow {
        method: "PdfView",
        runtime: "perry_ui_pdf_view_create",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "pdfViewLoadFile",
        runtime: "perry_ui_pdf_view_load_file",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "pdfViewGetPageCount",
        runtime: "perry_ui_pdf_view_get_page_count",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "pdfViewGoToPage",
        runtime: "perry_ui_pdf_view_go_to_page",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "pdfViewGetCurrentPage",
        runtime: "perry_ui_pdf_view_get_current_page",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "pdfViewSetScale",
        runtime: "perry_ui_pdf_view_set_scale",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Rich text editor (issue #478) ----
    MethodRow {
        method: "RichTextEditor",
        runtime: "perry_ui_rich_text_create",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "richTextSetString",
        runtime: "perry_ui_rich_text_set_string",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "richTextGetString",
        runtime: "perry_ui_rich_text_get_string",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "richTextSetHtml",
        runtime: "perry_ui_rich_text_set_html",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "richTextGetHtml",
        runtime: "perry_ui_rich_text_get_html",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "richTextToggleBold",
        runtime: "perry_ui_rich_text_toggle_bold",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "richTextToggleItalic",
        runtime: "perry_ui_rich_text_toggle_italic",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "richTextToggleUnderline",
        runtime: "perry_ui_rich_text_toggle_underline",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetControlSize",
        runtime: "perry_ui_widget_set_control_size",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOnClick",
        runtime: "perry_ui_widget_set_on_click",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOnHover",
        runtime: "perry_ui_widget_set_on_hover",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOnDoubleClick",
        runtime: "perry_ui_widget_set_on_double_click",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // Continuous pointer events (issue #1868). Callbacks receive a
    // PointerEvent { x, y, button, pointerType } object — allocated
    // in perry-runtime/src/pointer_event.rs and passed via
    // js_closure_call1. Coordinates are widget-local points (top-left
    // origin). onMouseMove is coalesced to one call per frame per
    // widget at the platform-backend layer.
    MethodRow {
        method: "widgetSetOnMouseDown",
        runtime: "perry_ui_widget_set_on_mouse_down",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOnMouseUp",
        runtime: "perry_ui_widget_set_on_mouse_up",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOnMouseMove",
        runtime: "perry_ui_widget_set_on_mouse_move",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetAnimateOpacity",
        runtime: "perry_ui_widget_animate_opacity",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetAnimatePosition",
        runtime: "perry_ui_widget_animate_position",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetAddOverlay",
        runtime: "perry_ui_widget_add_overlay",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetBorderColor",
        runtime: "perry_ui_widget_set_border_color",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetBorderWidth",
        runtime: "perry_ui_widget_set_border_width",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // Drop shadow setter (issue #185 Phase B). Args: handle, r,g,b,a (color
    // 0-1; alpha lands in shadowOpacity), blur, offset_x, offset_y. Wired
    // on every Apple platform; Phase B closures will add Android (elevation),
    // GTK4 (CSS box-shadow), Web (CSS), Windows (DirectComposition).
    MethodRow {
        method: "widgetSetShadow",
        runtime: "perry_ui_widget_set_shadow",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetContextMenu",
        runtime: "perry_ui_widget_set_context_menu",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stackSetDetachesHidden",
        runtime: "perry_ui_stack_set_detaches_hidden",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Additional constructors ----
    MethodRow {
        method: "Toggle",
        runtime: "perry_ui_toggle_create",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    // Programmatically set a Toggle's on/off state (issue #5076). `on`
    // is a raw i64 (0 = off, non-zero = on); `Toggle(label, onChange)`
    // has no initial-state param, so this is the documented way to show
    // a non-default ON state in a rebuild/re-create render model.
    MethodRow {
        method: "toggleSetState",
        runtime: "perry_ui_toggle_set_state",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "Slider",
        runtime: "perry_ui_slider_create",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "SecureField",
        runtime: "perry_ui_securefield_create",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "ProgressView",
        runtime: "perry_ui_progressview_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "ZStack",
        runtime: "perry_ui_zstack_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "Section",
        runtime: "perry_ui_section_create",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    // ---- ProgressView ----
    MethodRow {
        method: "progressviewSetValue",
        runtime: "perry_ui_progressview_set_value",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Picker ----
    MethodRow {
        method: "Picker",
        runtime: "perry_ui_picker_create",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "pickerAddItem",
        runtime: "perry_ui_picker_add_item",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "pickerGetSelected",
        runtime: "perry_ui_picker_get_selected",
        args: &[ArgKind::Widget],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "pickerSetSelected",
        runtime: "perry_ui_picker_set_selected",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- NavigationStack ----
    MethodRow {
        method: "NavStack",
        runtime: "perry_ui_navstack_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "navstackPush",
        runtime: "perry_ui_navstack_push",
        args: &[ArgKind::Widget, ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "navstackPop",
        runtime: "perry_ui_navstack_pop",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- TabBar ----
    MethodRow {
        method: "TabBar",
        runtime: "perry_ui_tabbar_create",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "tabbarAddTab",
        runtime: "perry_ui_tabbar_add_tab",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tabbarSetSelected",
        runtime: "perry_ui_tabbar_set_selected",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Menu extras ----
    MethodRow {
        method: "menuAddSubmenu",
        runtime: "perry_ui_menu_add_submenu",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuClear",
        runtime: "perry_ui_menu_clear",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuAddItemWithShortcut",
        runtime: "perry_ui_menu_add_item_with_shortcut",
        args: &[
            ArgKind::Widget,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Closure,
        ],
        ret: ReturnKind::Void,
    },
    // ---- ScrollView extras (scrollViewSetOffset / scrollViewScrollTo
    //                        moved up next to scrollViewGetOffset to
    //                        eliminate a pre-Tier-1.3 duplicate row pair
    //                        that the drift test now catches) ----

    // ---- Button extras ----
    MethodRow {
        method: "buttonSetContentTintColor",
        runtime: "perry_ui_button_set_content_tint_color",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "buttonSetImage",
        runtime: "perry_ui_button_set_image",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "buttonSetImagePosition",
        runtime: "perry_ui_button_set_image_position",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Clipboard ----
    MethodRow {
        method: "clipboardRead",
        runtime: "perry_ui_clipboard_read",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "clipboardWrite",
        runtime: "perry_ui_clipboard_write",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- Alert ----
    // `alert(title, message)` dispatches to a dedicated 2-arg FFI; the prior
    // entry pointed at the 4-arg `perry_ui_alert` symbol, which was ABI-broken
    // (buttons/callback read from uninitialized registers, usually segfaulting
    // inside js_array_get_length).
    MethodRow {
        method: "alert",
        runtime: "perry_ui_alert_simple",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // `alertWithButtons(title, message, buttons, cb)` — buttons is a JS array
    // of labels, callback receives the 0-based button index. Passed as F64
    // because the runtime extracts the array pointer via
    // `js_nanbox_get_pointer` just like closures.
    MethodRow {
        method: "alertWithButtons",
        runtime: "perry_ui_alert",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- Window (constructor — receiver-less) ----
    MethodRow {
        method: "Window",
        runtime: "perry_ui_window_create",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    // ---- VStack/HStack with built-in insets (no children array — children added via widgetAddChild) ----
    MethodRow {
        method: "VStackWithInsets",
        runtime: "perry_ui_vstack_create_with_insets",
        args: &[
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "HStackWithInsets",
        runtime: "perry_ui_hstack_create_with_insets",
        args: &[
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Widget,
    },
    // ---- Embed external NSView ----
    MethodRow {
        method: "embedNSView",
        runtime: "perry_ui_embed_nsview",
        args: &[ArgKind::I64Raw],
        ret: ReturnKind::Widget,
    },
    // ---- File dialogs ----
    MethodRow {
        method: "openFileDialog",
        runtime: "perry_ui_open_file_dialog",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "openFolderDialog",
        runtime: "perry_ui_open_folder_dialog",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "saveFileDialog",
        runtime: "perry_ui_save_file_dialog",
        args: &[ArgKind::Closure, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- Widget overlay frame ----
    MethodRow {
        method: "widgetSetOverlayFrame",
        runtime: "perry_ui_widget_set_overlay_frame",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    // ---- Toolbar ----
    MethodRow {
        method: "toolbarCreate",
        runtime: "perry_ui_toolbar_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "toolbarAddItem",
        runtime: "perry_ui_toolbar_add_item",
        args: &[
            ArgKind::Widget,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Closure,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "toolbarAttach",
        runtime: "perry_ui_toolbar_attach",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- SplitView ----
    MethodRow {
        method: "SplitView",
        runtime: "perry_ui_splitview_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "splitViewAddChild",
        runtime: "perry_ui_splitview_add_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- Sheet ----
    MethodRow {
        method: "sheetCreate",
        runtime: "perry_ui_sheet_create",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "sheetPresent",
        runtime: "perry_ui_sheet_present",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "sheetDismiss",
        runtime: "perry_ui_sheet_dismiss",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- FrameSplit (NSSplitView wrapper) ----
    MethodRow {
        method: "frameSplitCreate",
        runtime: "perry_ui_frame_split_create",
        args: &[ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "frameSplitAddChild",
        runtime: "perry_ui_frame_split_add_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- File dialog polling ----
    MethodRow {
        method: "pollOpenFile",
        runtime: "perry_ui_poll_open_file",
        args: &[],
        ret: ReturnKind::F64,
    },
    // ---- Keyboard shortcuts ----
    // `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
    MethodRow {
        method: "addKeyboardShortcut",
        runtime: "perry_ui_add_keyboard_shortcut",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // System-wide hotkey — fires even when the app is backgrounded.
    // Real Carbon `RegisterEventHotKey` impl on macOS; no-op stub on all other platforms.
    MethodRow {
        method: "registerGlobalHotkey",
        runtime: "perry_ui_register_global_hotkey",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- Continuous keyboard events (issue #1864) ----
    // Widget-scoped: fires only while `widget` owns logical focus.
    MethodRow {
        method: "onKeyDown",
        runtime: "perry_ui_widget_set_on_key_down",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onKeyUp",
        runtime: "perry_ui_widget_set_on_key_up",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // App-level fallback: fires when no widget currently owns focus.
    MethodRow {
        method: "onAppKeyDown",
        runtime: "perry_ui_app_set_on_key_down",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onAppKeyUp",
        runtime: "perry_ui_app_set_on_key_up",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // Programmatic focus management (paired with `style: { focusable: true }`
    // on widgets that are not naturally focusable, e.g. Canvas / VStack).
    MethodRow {
        method: "focus",
        runtime: "perry_ui_focus_widget",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "blur",
        runtime: "perry_ui_blur_widget",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // Branchless poll for `isKeyDown(Key.ArrowLeft)`. Returns 0/1 as a JS number.
    // Argument is the numeric `Key` enum value — no string round-trip.
    MethodRow {
        method: "isKeyDown",
        runtime: "perry_ui_is_key_down",
        args: &[ArgKind::F64],
        ret: ReturnKind::I64AsF64,
    },
    // Snapshot of the current modifier bitfield. Accurate outside of any
    // key event — answers "is Shift held *right now*" while drawing, etc.
    MethodRow {
        method: "currentModifiers",
        runtime: "perry_ui_current_modifiers",
        args: &[],
        ret: ReturnKind::I64AsF64,
    },
    // ---- App lifecycle hooks ----
    MethodRow {
        method: "onTerminate",
        runtime: "perry_ui_app_on_terminate",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onActivate",
        runtime: "perry_ui_app_on_activate",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- App extras ----
    // Issue #389: signature is `(Widget, intervalMs, callback)`. The
    // codegen accepts both the 2-arg user form
    // `appSetTimer(intervalMs, callback)` and the historical 3-arg
    // `appSetTimer(app, intervalMs, callback)` — see
    // `lower_perry_ui_table_call`'s `appSetTimer` arity adapter. The
    // platform runtime helpers ignore `_app_handle` already, so the
    // codegen synthesises a 0 widget handle for the 2-arg form.
    MethodRow {
        method: "appSetTimer",
        runtime: "perry_ui_app_set_timer",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "appSetMinSize",
        runtime: "perry_ui_app_set_min_size",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "appSetMaxSize",
        runtime: "perry_ui_app_set_max_size",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // Menu-bar / background apps: set the macOS activation policy
    // ("regular" | "accessory" | "background"). Like `appSetTimer` the
    // native ABI is `(app_handle, value)`, but the user-facing form is the
    // 1-arg `appSetActivationPolicy(policy)`; `lower_perry_ui_table_call`
    // prepends a synthetic 0 app-handle. On macOS "accessory" also
    // suppresses the auto-presented launch window (menu-bar-only apps open
    // windows on demand). FFI helpers ignore `_app_handle` on every backend.
    MethodRow {
        method: "appSetActivationPolicy",
        runtime: "perry_ui_app_set_activation_policy",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- (#391: removed the 1-arg `scrollviewSetOffset(scrollView, y)`
    // legacy alias here — the 2-arg `(x, y)` form is now declared
    // alongside `scrollviewGetOffset` / `scrollviewScrollTo` above and
    // matches the type stub. Old code calling
    // `scrollviewSetOffset(sv, y)` will need to migrate to
    // `scrollviewSetOffset(sv, 0, y)` or
    // `scrollviewScrollTo(sv, 0, y)`.) ----
    // ---- Table (issue #192) ----
    // NSTableView-backed scrollable table. Real implementation lives in
    // `perry-ui-macos`; iOS / Android / GTK4 / Windows / tvOS / visionOS /
    // watchOS export no-op stubs (returns handle 0, all setters no-op).
    // The render closure is `(row: number, col: number) => Widget` —
    // returns a Text/HStack/etc. that becomes the cell view. Free-function
    // call shape mirrors `pickerAddItem` / `pickerSetSelected` rather
    // than the `picker.addItem(...)` method form, matching the existing
    // wasm/js dispatch tables that already route `tableSetColumnHeader`
    // and friends.
    MethodRow {
        method: "Table",
        runtime: "perry_ui_table_create",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "tableSetColumnHeader",
        runtime: "perry_ui_table_set_column_header",
        args: &[ArgKind::Widget, ArgKind::I64Raw, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableSetColumnWidth",
        runtime: "perry_ui_table_set_column_width",
        args: &[ArgKind::Widget, ArgKind::I64Raw, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableUpdateRowCount",
        runtime: "perry_ui_table_update_row_count",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableSetOnRowSelect",
        runtime: "perry_ui_table_set_on_row_select",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableGetSelectedRow",
        runtime: "perry_ui_table_get_selected_row",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    // Issue #473 — sort + filter + multi-select extensions
    MethodRow {
        method: "tableSetOnSortChange",
        runtime: "perry_ui_table_set_on_sort_change",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableSetAllowsMultipleSelection",
        runtime: "perry_ui_table_set_allows_multiple_selection",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableGetSelectedRowsCount",
        runtime: "perry_ui_table_get_selected_rows_count",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "tableGetSelectedRowAt",
        runtime: "perry_ui_table_get_selected_row_at",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "tableSetFilterText",
        runtime: "perry_ui_table_set_filter_text",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "tableGetFilterText",
        runtime: "perry_ui_table_get_filter_text",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    // ---- Camera (issue #191) ----
    // Live camera preview widget. Real implementations live in
    // `perry-ui-ios` (AVCaptureSession) and `perry-ui-android` (Camera2).
    // tvOS / visionOS / watchOS / macOS / GTK4 / Windows export no-op
    // stubs so cross-platform user code links cleanly. `cameraSampleColor`
    // returns packed RGB (`r*65536 + g*256 + b`) or `-1` if no frame is
    // available — F64 return is preserved as a plain JS number.
    MethodRow {
        method: "CameraView",
        runtime: "perry_ui_camera_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "cameraStart",
        runtime: "perry_ui_camera_start",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraStop",
        runtime: "perry_ui_camera_stop",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraFreeze",
        runtime: "perry_ui_camera_freeze",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraUnfreeze",
        runtime: "perry_ui_camera_unfreeze",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraSampleColor",
        runtime: "perry_ui_camera_sample_color",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "cameraSetOnTap",
        runtime: "perry_ui_camera_set_on_tap",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraRegisterFrameCallback",
        runtime: "perry_ui_camera_register_frame_callback",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cameraUnregisterFrameCallback",
        runtime: "perry_ui_camera_unregister_frame_callback",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- Canvas ----
    MethodRow {
        method: "Canvas",
        runtime: "perry_ui_canvas_create",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    // ---- BloomView (issue #2395 / #5519) ----
    // A render-surface host: `BloomView(width, height)` reserves a native view
    // the Bloom engine draws into. `bloomViewGetNativeHandle(view)` returns the
    // platform handle (HWND / NSView* / UIView* / GtkWidget* / ANativeWindow*)
    // as a JS number so user TS can call the engine's attach (`attachToNSView`
    // / `attachToSurface` / …, all forwarding to `bloom_attach_native`).
    MethodRow {
        method: "BloomView",
        runtime: "perry_ui_bloomview_create",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    // Canonical name since #5519 — platform-neutral now that the handle is an
    // NSView*/UIView*/GtkWidget*/ANativeWindow*, not only an HWND.
    MethodRow {
        method: "bloomViewGetNativeHandle",
        runtime: "perry_ui_bloomview_get_hwnd",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    // Deprecated alias — kept so existing code keeps working. Same runtime
    // symbol as `bloomViewGetNativeHandle`.
    MethodRow {
        method: "bloomViewGetHwnd",
        runtime: "perry_ui_bloomview_get_hwnd",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    // ---- Drag & drop (issue #4773) ----
    // Widget-level setters that attach drag/drop behavior to an existing
    // widget handle. `widgetOnDrop` registers a drop destination; the
    // callback receives a `{ text?, files?, urls? }` object built natively.
    // The three `widgetSetDrag*` setters register a drag source; each
    // provider closure returns the string payload for its pasteboard type
    // (text / file-path / url). Real behavior is implemented per platform;
    // every backend exports these symbols (no-op where the OS has no DnD).
    MethodRow {
        method: "widgetOnDrop",
        runtime: "perry_ui_widget_on_drop",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetDragText",
        runtime: "perry_ui_widget_set_drag_text",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetDragFile",
        runtime: "perry_ui_widget_set_drag_file",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetDragUrl",
        runtime: "perry_ui_widget_set_drag_url",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
];
