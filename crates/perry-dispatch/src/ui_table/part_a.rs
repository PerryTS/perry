//! `PERRY_UI_TABLE` rows, part A. Split out of ui_table.rs to satisfy the
//! 2000-line file-size gate; concatenated at compile time in the parent.

use crate::{ArgKind, MethodRow, ReturnKind};

pub(crate) const PERRY_UI_TABLE_PART_A: &[MethodRow] = &[
    // ---- Constructors (return widget handle) ----
    // AdBanner(unitId, size) — #867. Both args required (the generic
    // dispatch no-ops on arity mismatch); use the `AdSize` string
    // constants from the d.ts for `size`.
    MethodRow {
        method: "AdBanner",
        runtime: "perry_ui_adbanner_create",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "Divider",
        runtime: "perry_ui_divider_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "ScrollView",
        runtime: "perry_ui_scrollview_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "Spacer",
        runtime: "perry_ui_spacer_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "Text",
        runtime: "perry_ui_text_create",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    // ---- Cross-platform reactive text + toast (Phase 2 v3.3) ----
    // `Text(content, id)` 2-arg form is special-cased in lower_call/native.rs
    // (like VStack / Button) so the id string reaches perry_ui_text_create_with_id.
    // Only the 1-arg form routes through this table entry; the 2-arg form is
    // intercepted before the table lookup and is not represented here.
    MethodRow {
        method: "showToast",
        runtime: "perry_ui_show_toast",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setText",
        runtime: "perry_ui_set_text",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "TextArea",
        runtime: "perry_ui_textarea_create",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    // ---- Issue #710: AttributedText (per-range styling) ----
    MethodRow {
        method: "AttributedText",
        runtime: "perry_ui_attributed_text_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "attributedTextAppend",
        runtime: "perry_ui_attributed_text_append",
        args: &[
            ArgKind::Widget,
            ArgKind::Str,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "attributedTextClear",
        runtime: "perry_ui_attributed_text_clear",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "TextField",
        runtime: "perry_ui_textfield_create",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    // ---- Menu / menu bar ----
    MethodRow {
        method: "menuAddItem",
        runtime: "perry_ui_menu_add_item",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuAddSeparator",
        runtime: "perry_ui_menu_add_separator",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuAddStandardAction",
        runtime: "perry_ui_menu_add_standard_action",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuBarAddMenu",
        runtime: "perry_ui_menubar_add_menu",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuBarAttach",
        runtime: "perry_ui_menubar_attach",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "menuBarCreate",
        runtime: "perry_ui_menubar_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "menuCreate",
        runtime: "perry_ui_menu_create",
        args: &[],
        ret: ReturnKind::Widget,
    },
    // ---- Tray icon (issue #490) ----
    MethodRow {
        method: "trayCreate",
        runtime: "perry_ui_tray_create",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "traySetIcon",
        runtime: "perry_ui_tray_set_icon",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "traySetTooltip",
        runtime: "perry_ui_tray_set_tooltip",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "trayAttachMenu",
        runtime: "perry_ui_tray_attach_menu",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "trayOnClick",
        runtime: "perry_ui_tray_on_click",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "trayDestroy",
        runtime: "perry_ui_tray_destroy",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- ScrollView ----
    MethodRow {
        method: "scrollviewSetChild",
        runtime: "perry_ui_scrollview_set_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewSetChild",
        runtime: "perry_ui_scrollview_set_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewGetOffset",
        runtime: "perry_ui_scrollview_get_offset",
        args: &[ArgKind::Widget],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "scrollViewSetOffset",
        runtime: "perry_ui_scrollview_set_offset",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewScrollTo",
        runtime: "perry_ui_scrollview_scroll_to",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // Issue #391: lowercase-v aliases for symmetry with
    // `scrollviewSetChild`. Each routes to the same runtime FFI as
    // its `scrollView…` peer above; both spellings coexist so old
    // code (targeting an earlier Perry that used the lowercase form)
    // keeps working and new code can match the camelCase convention.
    MethodRow {
        method: "scrollviewGetOffset",
        runtime: "perry_ui_scrollview_get_offset",
        args: &[ArgKind::Widget],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "scrollviewSetOffset",
        runtime: "perry_ui_scrollview_set_offset",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollviewScrollTo",
        runtime: "perry_ui_scrollview_scroll_to",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // Issue #390: native pull-to-refresh — restore the dispatch
    // entries that connect the user-facing API to the existing
    // platform runtime helpers (`perry_ui_scrollview_set_refresh_control`
    // and `_end_refreshing` are already implemented on every platform
    // crate; the dispatch table just lost the connection at some
    // earlier rename pass). Both lowercase-v and camelCase spellings
    // are dispatched for consistency with the other ScrollView aliases.
    MethodRow {
        method: "scrollviewSetRefreshControl",
        runtime: "perry_ui_scrollview_set_refresh_control",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewSetRefreshControl",
        runtime: "perry_ui_scrollview_set_refresh_control",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollviewEndRefreshing",
        runtime: "perry_ui_scrollview_end_refreshing",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewEndRefreshing",
        runtime: "perry_ui_scrollview_end_refreshing",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- Issue #553: infinite-scroll callback + LazyVStack pull-to-refresh ----
    // Mirrors the #390 ScrollView pattern; same backpressure contract on
    // both platforms (the callback fires once per threshold-cross and
    // re-arms only when the user scrolls back up past the threshold).
    MethodRow {
        method: "scrollviewSetScrollEndCallback",
        runtime: "perry_ui_scrollview_set_scroll_end_callback",
        args: &[ArgKind::Widget, ArgKind::Closure, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "scrollViewSetScrollEndCallback",
        runtime: "perry_ui_scrollview_set_scroll_end_callback",
        args: &[ArgKind::Widget, ArgKind::Closure, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lazyvstackSetRefreshControl",
        runtime: "perry_ui_lazyvstack_set_refresh_control",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lazyvstackEndRefreshing",
        runtime: "perry_ui_lazyvstack_end_refreshing",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lazyvstackSetScrollEndCallback",
        runtime: "perry_ui_lazyvstack_set_scroll_end_callback",
        args: &[ArgKind::Widget, ArgKind::Closure, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Issue #553: BottomNavigation (5-tab bottom bar) ----
    MethodRow {
        method: "BottomNavigation",
        runtime: "perry_ui_bottom_nav_create",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "bottomNavAddItem",
        runtime: "perry_ui_bottom_nav_add_item",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "bottomNavSetBadge",
        runtime: "perry_ui_bottom_nav_set_badge",
        args: &[ArgKind::Widget, ArgKind::I64Raw, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "bottomNavSetSelected",
        runtime: "perry_ui_bottom_nav_set_selected",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Issue #706: BottomNavigation tint customization ----
    MethodRow {
        method: "bottomNavSetTintColor",
        runtime: "perry_ui_bottom_nav_set_tint_color",
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
        method: "bottomNavSetUnselectedTintColor",
        runtime: "perry_ui_bottom_nav_set_unselected_tint_color",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    // ---- Issue #553: ImageGallery (swipeable carousel) ----
    MethodRow {
        method: "ImageGallery",
        runtime: "perry_ui_image_gallery_create",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "imageGalleryAddImage",
        runtime: "perry_ui_image_gallery_add_image",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "imageGallerySetIndex",
        runtime: "perry_ui_image_gallery_set_index",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Stack layout ----
    MethodRow {
        method: "stackSetAlignment",
        runtime: "perry_ui_stack_set_alignment",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stackSetDistribution",
        runtime: "perry_ui_stack_set_distribution",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Text setters ----
    MethodRow {
        method: "textSetColor",
        runtime: "perry_ui_text_set_color",
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
        method: "textSetFontFamily",
        runtime: "perry_ui_text_set_font_family",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textSetFontSize",
        runtime: "perry_ui_text_set_font_size",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textSetFontWeight",
        runtime: "perry_ui_text_set_font_weight",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textSetString",
        runtime: "perry_ui_text_set_string",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- Issue #707: Text line cap + truncation mode ----
    MethodRow {
        method: "textSetNumberOfLines",
        runtime: "perry_ui_text_set_number_of_lines",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textSetTruncationMode",
        runtime: "perry_ui_text_set_truncation_mode",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Issue #3621: Text horizontal alignment ----
    MethodRow {
        method: "textSetTextAlignment",
        runtime: "perry_ui_text_set_text_alignment",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textSetWraps",
        runtime: "perry_ui_text_set_wraps",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Button setters ----
    MethodRow {
        method: "buttonSetBordered",
        runtime: "perry_ui_button_set_bordered",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "buttonSetTextColor",
        runtime: "perry_ui_button_set_text_color",
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
        method: "buttonSetTitle",
        runtime: "perry_ui_button_set_title",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- TextField / TextArea ----
    MethodRow {
        method: "textfieldSetString",
        runtime: "perry_ui_textfield_set_string",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textareaSetString",
        runtime: "perry_ui_textarea_set_string",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    // ---- Generic widget ops ----
    MethodRow {
        method: "setCornerRadius",
        runtime: "perry_ui_widget_set_corner_radius",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetAddChild",
        runtime: "perry_ui_widget_add_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetClearChildren",
        runtime: "perry_ui_widget_clear_children",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetMatchParentHeight",
        runtime: "perry_ui_widget_match_parent_height",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetMatchParentWidth",
        runtime: "perry_ui_widget_match_parent_width",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetBackgroundColor",
        runtime: "perry_ui_widget_set_background_color",
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
        method: "widgetSetBackgroundGradient",
        runtime: "perry_ui_widget_set_background_gradient",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
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
        method: "widgetSetHeight",
        runtime: "perry_ui_widget_set_height",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetHidden",
        runtime: "perry_ui_set_widget_hidden",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetHugging",
        runtime: "perry_ui_widget_set_hugging",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetWidth",
        runtime: "perry_ui_widget_set_width",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Image ----
    MethodRow {
        method: "ImageFile",
        runtime: "perry_ui_image_create_file",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "ImageSymbol",
        runtime: "perry_ui_image_create_symbol",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    // ---- Canvas image assets (issue #2022) ----
    MethodRow {
        method: "loadImage",
        runtime: "perry_ui_load_image",
        args: &[ArgKind::Str],
        ret: ReturnKind::Promise,
    },
    // ---- Issue #635: single-Image-by-URL ----
    // The TS surface accepts both `Image(url, alt?)` (positional, picked
    // up by this row) and `Image({ url, alt })` (object-literal, handled
    // by a special case in `lower_call/native.rs` that destructures the
    // options object before falling through here).
    MethodRow {
        method: "Image",
        runtime: "perry_ui_image_create_url",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "imageSetSize",
        runtime: "perry_ui_image_set_size",
        args: &[ArgKind::Widget, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "imageSetTint",
        runtime: "perry_ui_image_set_tint",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    // ---- WebView (issue #658) ----
    // The TS surface accepts `WebView({ url, allowedDomains?, userAgent?,
    // ephemeral?, onShouldNavigate?, onLoaded?, onError?, width?, height? })`.
    // The object-literal form is destructured by `lower_call/native.rs` into
    // a `webviewCreate(url, w, h)` call followed by per-prop set_* calls.
    // This row backs the lowered create call.
    MethodRow {
        method: "webviewCreate",
        runtime: "perry_ui_webview_create",
        // v2-B: accepts a 4th `ephemeral_hint` arg (1.0 = ephemeral cookies,
        // default; 0.0 = persistent). Setting it via this param instead of
        // a follow-up `set_ephemeral` lets backends with construction-time
        // data-store choices (WebView2 userDataFolder, WebKitGTK
        // NetworkSession) honor it before any navigation kicks off.
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "webviewSetUserAgent",
        runtime: "perry_ui_webview_set_user_agent",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewSetAllowedDomains",
        runtime: "perry_ui_webview_set_allowed_domains",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewSetEphemeral",
        runtime: "perry_ui_webview_set_ephemeral",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewSetOnShouldNavigate",
        runtime: "perry_ui_webview_set_on_should_navigate",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewSetOnLoaded",
        runtime: "perry_ui_webview_set_on_loaded",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewSetOnError",
        runtime: "perry_ui_webview_set_on_error",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewLoadUrl",
        runtime: "perry_ui_webview_load_url",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewReload",
        runtime: "perry_ui_webview_reload",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewGoBack",
        runtime: "perry_ui_webview_go_back",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewGoForward",
        runtime: "perry_ui_webview_go_forward",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewCanGoBack",
        runtime: "perry_ui_webview_can_go_back",
        args: &[ArgKind::Widget],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "webviewEvaluateJs",
        runtime: "perry_ui_webview_evaluate_js",
        args: &[ArgKind::Widget, ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "webviewClearCookies",
        runtime: "perry_ui_webview_clear_cookies",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- Padding / Edge Insets ----
    MethodRow {
        method: "setPadding",
        runtime: "perry_ui_widget_set_edge_insets",
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
        method: "widgetSetEdgeInsets",
        runtime: "perry_ui_widget_set_edge_insets",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    // ---- LazyVStack (virtualized list) ----
    // `LazyVStack(count, (i) => Widget)` — on macOS backed by NSTableView
    // with lazy row rendering. The render closure is invoked only for rows
    // currently in the visible rect.
    MethodRow {
        method: "LazyVStack",
        runtime: "perry_ui_lazyvstack_create",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "lazyvstackUpdate",
        runtime: "perry_ui_lazyvstack_update",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lazyvstackSetRowHeight",
        runtime: "perry_ui_lazyvstack_set_row_height",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- State ----
    MethodRow {
        method: "State",
        runtime: "perry_ui_state_create",
        args: &[ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "stateCreate",
        runtime: "perry_ui_state_create",
        args: &[ArgKind::F64],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "stateGet",
        runtime: "perry_ui_state_get",
        args: &[ArgKind::Widget],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "stateSet",
        runtime: "perry_ui_state_set",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateOnChange",
        runtime: "perry_ui_state_on_change",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateBindTextNumeric",
        runtime: "perry_ui_state_bind_text_numeric",
        args: &[ArgKind::Widget, ArgKind::Widget, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateBindSlider",
        runtime: "perry_ui_state_bind_slider",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateBindToggle",
        runtime: "perry_ui_state_bind_toggle",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateBindVisibility",
        runtime: "perry_ui_state_bind_visibility",
        args: &[ArgKind::Widget, ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stateBindTextfield",
        runtime: "perry_ui_state_bind_textfield",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    // ---- TextField extras ----
    // perry_ui_textfield_get_string returns *mut StringHeader cast to i64;
    // the GC alloc is GC_FLAG_PINNED before return so it survives until
    // we NaN-box it. ReturnKind::F64 here treated the pointer bits as a
    // raw double — every read produced gibberish (e.g. "27017",
    // "65933097631650390000000000000000") that string ops then operated on.
    MethodRow {
        method: "textfieldGetString",
        runtime: "perry_ui_textfield_get_string",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "textfieldFocus",
        runtime: "perry_ui_textfield_focus",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldBlurAll",
        runtime: "perry_ui_textfield_blur_all",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetNextKeyView",
        runtime: "perry_ui_textfield_set_next_key_view",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetOnSubmit",
        runtime: "perry_ui_textfield_set_on_submit",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetOnFocus",
        runtime: "perry_ui_textfield_set_on_focus",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetBackgroundColor",
        runtime: "perry_ui_textfield_set_background_color",
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
        method: "textfieldSetBorderless",
        runtime: "perry_ui_textfield_set_borderless",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetFontSize",
        runtime: "perry_ui_textfield_set_font_size",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "textfieldSetTextColor",
        runtime: "perry_ui_textfield_set_text_color",
        args: &[
            ArgKind::Widget,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    // Same fix as textfieldGetString — runtime returns a string pointer.
    MethodRow {
        method: "textareaGetString",
        runtime: "perry_ui_textarea_get_string",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    // ---- Text extras ----
    MethodRow {
        method: "textSetSelectable",
        runtime: "perry_ui_text_set_selectable",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // Text decoration (issue #185 Phase B): 0=none, 1=underline,
    // 2=strikethrough. Wired on every backend (Apple via
    // NSAttributedString, Android via Paint flags, GTK4 via Pango
    // attributes, Web via CSS `text-decoration`, watchOS via tree
    // metadata + SwiftUI host modifier). Windows is stub-with-state.
    MethodRow {
        method: "textSetDecoration",
        runtime: "perry_ui_text_set_decoration",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    // ---- Widget extras ----
    MethodRow {
        method: "widgetAddChildAt",
        runtime: "perry_ui_widget_add_child_at",
        args: &[ArgKind::Widget, ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetRemoveChild",
        runtime: "perry_ui_widget_remove_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetReorderChild",
        runtime: "perry_ui_widget_reorder_child",
        args: &[ArgKind::Widget, ArgKind::I64Raw, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetOpacity",
        runtime: "perry_ui_widget_set_opacity",
        args: &[ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetEnabled",
        runtime: "perry_ui_widget_set_enabled",
        args: &[ArgKind::Widget, ArgKind::I64Raw],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetTooltip",
        runtime: "perry_ui_widget_set_tooltip",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "widgetSetRichTooltip",
        runtime: "perry_ui_widget_set_rich_tooltip",
        args: &[ArgKind::Widget, ArgKind::Widget, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Combobox (issue #475) ----
    MethodRow {
        method: "Combobox",
        runtime: "perry_ui_combobox_create",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "comboboxAddItem",
        runtime: "perry_ui_combobox_add_item",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "comboboxSetValue",
        runtime: "perry_ui_combobox_set_value",
        args: &[ArgKind::Widget, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "comboboxGetValue",
        runtime: "perry_ui_combobox_get_value",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    // ---- TreeView (issue #480) ----
    MethodRow {
        method: "TreeNode",
        runtime: "perry_ui_tree_node_create",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "treeNodeAddChild",
        runtime: "perry_ui_tree_node_add_child",
        args: &[ArgKind::Widget, ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "TreeView",
        runtime: "perry_ui_tree_view_create",
        args: &[ArgKind::Widget, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "treeViewExpandAll",
        runtime: "perry_ui_tree_view_expand_all",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "treeViewCollapseAll",
        runtime: "perry_ui_tree_view_collapse_all",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "treeViewGetSelectedId",
        runtime: "perry_ui_tree_view_get_selected_id",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    // ---- Calendar (issue #481) ----
    MethodRow {
        method: "Calendar",
        runtime: "perry_ui_calendar_create",
        args: &[ArgKind::I64Raw, ArgKind::I64Raw, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "calendarSetDate",
        runtime: "perry_ui_calendar_set_date",
        args: &[
            ArgKind::Widget,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "calendarGetSelectedDate",
        runtime: "perry_ui_calendar_get_selected_date",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
    // ---- DatePicker (issue #4772) ----
    MethodRow {
        method: "DatePicker",
        runtime: "perry_ui_date_picker_create",
        args: &[ArgKind::I64Raw, ArgKind::I64Raw, ArgKind::Closure],
        ret: ReturnKind::Widget,
    },
    MethodRow {
        method: "datePickerSetDate",
        runtime: "perry_ui_date_picker_set_date",
        args: &[
            ArgKind::Widget,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
            ArgKind::I64Raw,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "datePickerGetSelectedDate",
        runtime: "perry_ui_date_picker_get_selected_date",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Str,
    },
];
