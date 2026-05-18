//! Shared dispatch tables for Perry's three codegen backends.
//!
//! Adding a new perry/ui, perry/system, or perry/i18n function used to
//! require synchronized edits in **four** files (LLVM `lower_call.rs`'s
//! tables, JS `emit.rs`'s `emit_ui_method_call` match arm, WASM
//! `emit.rs`'s `map_ui_method` match arm, plus runtime stubs). Drift was
//! silent — a missing JS arm produced an `unknown function` only when a
//! user compiled for that target (issue #191 CameraView is the canonical
//! example). This crate centralises the (TS-name → runtime-symbol)
//! mapping so the JS and WASM backends can derive their dispatch from
//! the same table the LLVM backend uses, and a drift test can fail CI
//! when an LLVM row lacks JS/WASM coverage.
//!
//! ## Tables exported
//!
//! - `PERRY_UI_TABLE` — receiver-less perry/ui calls (constructors +
//!   setters: `Text`, `Button`, `widgetSetBackgroundColor`, …).
//! - `PERRY_UI_INSTANCE_TABLE` — receiver-based perry/ui method calls
//!   (`window.show()`, `state.value()`, `canvas.fillRect(...)`).
//! - `PERRY_SYSTEM_TABLE` — perry/system calls (`isDarkMode`,
//!   `keychainSave`, `notificationSend`, …).
//! - `PERRY_I18N_TABLE` — perry/i18n format wrappers (`Currency`,
//!   `Percent`, `ShortDate`, …).
//!
//! All four tables share `MethodRow`. The args / return kinds matter
//! only to the LLVM backend (it needs them for ABI-correct call
//! emission); JS and WASM consume the (method → runtime) mapping via
//! [`ui_method_to_runtime`].
//!
//! ## Adding a new method
//!
//! Add one row to the appropriate table here. The LLVM backend picks it
//! up automatically; JS and WASM emit fall through to a
//! `ui_method_to_runtime` lookup before hitting their per-backend
//! extras, so a new row resolves on every target with no further edits.
//!
//! `NATIVE_MODULE_TABLE` (a different shape — has `module`,
//! `has_receiver`, `class_filter`) lives in `perry-codegen` for now;
//! moving it here is a follow-up.

/// How a perry/ui FFI function expects each argument to be passed.
/// Used by the LLVM backend for ABI-correct call emission. The JS and
/// WASM backends ignore this — they pass arguments through their own
/// conversion conventions.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// Widget handle: lower the JSValue, unbox the POINTER bits as i64.
    Widget,
    /// String pointer: lower the JSValue, then call
    /// `js_get_string_pointer_unified` to extract the underlying
    /// StringHeader pointer as i64. Handles SSO + heap strings.
    Str,
    /// Raw f64 number. Already NaN-boxed for numbers; pass through.
    F64,
    /// Closure handle: lower the JSValue (a `js_closure_alloc` pointer
    /// NaN-boxed as POINTER) and pass it as a raw f64. Runtime extracts
    /// the closure pointer via the same NaN-boxing convention.
    Closure,
    /// Raw i64 (rare; some setters take an enum tag as i64).
    I64Raw,
}

/// What the perry/ui FFI function returns and how to box it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ReturnKind {
    /// Widget handle: NaN-box the i64 result with POINTER_TAG.
    Widget,
    /// Raw f64: pass through unchanged.
    F64,
    /// Void return: emit `call void` and return the `0.0` sentinel.
    Void,
    /// `*mut StringHeader` (i64 ptr) → NaN-box with `STRING_TAG`.
    Str,
    /// i64 result converted to plain JS number via `sitofp`.
    I64AsF64,
}

/// A single dispatch row: TS method name → runtime symbol + ABI shape.
#[derive(Copy, Clone, Debug)]
pub struct MethodRow {
    /// TypeScript method name as it appears in the import (e.g.
    /// `"Text"`, `"textSetFontSize"`, `"isDarkMode"`).
    pub method: &'static str,
    /// Runtime function symbol the call lowers to (`perry_ui_*`,
    /// `perry_system_*`, `perry_i18n_*`).
    pub runtime: &'static str,
    /// Per-argument coercion (LLVM-only).
    pub args: &'static [ArgKind],
    /// Return-value boxing (LLVM-only).
    pub ret: ReturnKind,
}

pub const PERRY_UI_TABLE: &[MethodRow] = &[
    // ---- Constructors (return widget handle) ----
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
    // ---- Canvas ----
    MethodRow {
        method: "Canvas",
        runtime: "perry_ui_canvas_create",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Widget,
    },
];
pub const PERRY_UI_INSTANCE_TABLE: &[MethodRow] = &[
    // ---- Window instance methods ----
    MethodRow {
        method: "show",
        runtime: "perry_ui_window_show",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "hide",
        runtime: "perry_ui_window_hide",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "close",
        runtime: "perry_ui_window_close",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setBody",
        runtime: "perry_ui_window_set_body",
        args: &[ArgKind::Widget],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setSize",
        runtime: "perry_ui_window_set_size",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onFocusLost",
        runtime: "perry_ui_window_on_focus_lost",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- State instance methods ----
    MethodRow {
        method: "value",
        runtime: "perry_ui_state_get",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "set",
        runtime: "perry_ui_state_set",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Canvas instance methods ----
    MethodRow {
        method: "setFillColor",
        runtime: "perry_ui_canvas_set_fill_color",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setStrokeColor",
        runtime: "perry_ui_canvas_set_stroke_color",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setLineWidth",
        runtime: "perry_ui_canvas_set_line_width",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fillRect",
        runtime: "perry_ui_canvas_fill_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "strokeRect",
        runtime: "perry_ui_canvas_stroke_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "clearRect",
        runtime: "perry_ui_canvas_clear_rect",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "beginPath",
        runtime: "perry_ui_canvas_begin_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "moveTo",
        runtime: "perry_ui_canvas_move_to",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "lineTo",
        runtime: "perry_ui_canvas_line_to",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "arc",
        runtime: "perry_ui_canvas_arc",
        args: &[
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "closePath",
        runtime: "perry_ui_canvas_close_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fill",
        runtime: "perry_ui_canvas_fill",
        args: &[],
        ret: ReturnKind::Void,
    },
    // `stroke()` maps to perry_ui_canvas_stroke_path (no-arg stateful form).
    // The older perry_ui_canvas_stroke(h,r,g,b,a,lw) stateless form is kept
    // for the legacy fill_gradient API and is not removed.
    MethodRow {
        method: "stroke",
        runtime: "perry_ui_canvas_stroke_path",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "fillText",
        runtime: "perry_ui_canvas_fill_text",
        args: &[ArgKind::Str, ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setFont",
        runtime: "perry_ui_canvas_set_font",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
];
pub static PERRY_SYSTEM_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "isDarkMode",
        runtime: "perry_system_is_dark_mode",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDeviceIdiom",
        runtime: "perry_get_device_idiom",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "openURL",
        runtime: "perry_system_open_url",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "keychainSave",
        runtime: "perry_system_keychain_save",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "keychainGet",
        runtime: "perry_system_keychain_get",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "keychainDelete",
        runtime: "perry_system_keychain_delete",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "preferencesGet",
        runtime: "perry_system_preferences_get",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "preferencesSet",
        runtime: "perry_system_preferences_set",
        args: &[ArgKind::Str, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationSend",
        runtime: "perry_system_notification_send",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationRegisterRemote",
        runtime: "perry_system_notification_register_remote",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnReceive",
        runtime: "perry_system_notification_on_receive",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnBackgroundReceive",
        runtime: "perry_system_notification_on_background_receive",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationCancel",
        runtime: "perry_system_notification_cancel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "notificationOnTap",
        runtime: "perry_system_notification_on_tap",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStart",
        runtime: "perry_system_audio_start",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioStop",
        runtime: "perry_system_audio_stop",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioGetLevel",
        runtime: "perry_system_audio_get_level",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioGetPeak",
        runtime: "perry_system_audio_get_peak",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioGetWaveform",
        runtime: "perry_system_audio_get_waveform",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDeviceModel",
        runtime: "perry_system_get_device_model",
        args: &[],
        ret: ReturnKind::F64,
    },
    // Bug-report-flow utility: stable OS-version string per
    // platform. Returns a NaN-boxed JS string the user can splice
    // into crash reports / telemetry. Same dispatch shape as
    // getDeviceModel.
    MethodRow {
        method: "getOSVersion",
        runtime: "perry_system_get_os_version",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "audioSetOutputFilename",
        runtime: "perry_system_audio_set_output_filename",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStartRecording",
        runtime: "perry_system_audio_start_recording",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "audioStopRecording",
        runtime: "perry_system_audio_stop_recording",
        args: &[],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "getLocale",
        runtime: "perry_system_get_locale",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppVersion",
        runtime: "perry_system_get_app_version",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppBuildNumber",
        runtime: "perry_system_get_app_build_number",
        args: &[],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getBundleId",
        runtime: "perry_system_get_bundle_id",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getAppIcon",
        runtime: "perry_system_get_app_icon",
        args: &[ArgKind::Str],
        ret: ReturnKind::Widget,
    },
    // ---- Geolocation (issue #552) ----
    MethodRow {
        method: "geolocationGetCurrent",
        runtime: "perry_system_geolocation_get_current",
        args: &[ArgKind::Closure, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "geolocationWatch",
        runtime: "perry_system_geolocation_watch",
        args: &[ArgKind::Closure],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "geolocationStopWatch",
        runtime: "perry_system_geolocation_stop_watch",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "geolocationRequestPermission",
        runtime: "perry_system_geolocation_request_permission",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- Photo-library image picker (issue #552) ----
    MethodRow {
        method: "imagePickerPick",
        runtime: "perry_system_image_picker_pick",
        args: &[ArgKind::F64, ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    // ---- In-app screen capture (issue #918) ----
    MethodRow {
        method: "takeScreenshot",
        runtime: "perry_system_take_screenshot",
        args: &[],
        ret: ReturnKind::Str,
    },
    // ---- Network reachability (issue #582) ----
    MethodRow {
        method: "networkGetStatus",
        runtime: "perry_system_network_get_status",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "networkOnChange",
        runtime: "perry_system_network_on_change",
        args: &[ArgKind::Closure],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "networkStopOnChange",
        runtime: "perry_system_network_stop_on_change",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    // ---- Deep links: Universal Links / App Links / URL schemes (issue #583) ----
    MethodRow {
        method: "appOnOpenUrl",
        runtime: "perry_system_app_on_open_url",
        args: &[ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "appGetLaunchUrl",
        runtime: "perry_system_app_get_launch_url",
        args: &[],
        ret: ReturnKind::Str,
    },
];

/// perry/background — deferred / periodic background work (issue #538).
/// iOS BGTaskScheduler + Android WorkManager. Handler closures arrive via
/// `registerTask` and are persisted in a runtime-side identifier→closure
/// table so the platform's launchHandler / Worker can dispatch them.
/// `kind` is passed as a NaN-boxed string ("appRefresh" | "processing");
/// `earliestStartMs` is f64 (epoch ms or 0); `requiresNetwork` /
/// `requiresCharging` are NaN-boxed booleans.
pub static PERRY_BACKGROUND_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "registerTask",
        runtime: "perry_background_register_task",
        args: &[ArgKind::Str, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "schedule",
        runtime: "perry_background_schedule",
        args: &[
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::F64,
            ArgKind::F64,
            ArgKind::F64,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "cancel",
        runtime: "perry_background_cancel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Void,
    },
];
pub static PERRY_I18N_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "Currency",
        runtime: "perry_i18n_format_currency_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "Percent",
        runtime: "perry_i18n_format_percent_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "FormatNumber",
        runtime: "perry_i18n_format_number_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "ShortDate",
        runtime: "perry_i18n_format_date_short",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "LongDate",
        runtime: "perry_i18n_format_date_long",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "FormatTime",
        runtime: "perry_i18n_format_time_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "Raw",
        runtime: "perry_i18n_format_raw",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
];

/// Maps the TS exports from `types/perry/updater/index.d.ts` to their
/// `perry_updater_*` runtime symbols. Desktop-only by design — mobile
/// updates go through the OS store, not self-update. The runtime
/// symbols live in `perry-updater` (split internally into `core` for
/// cross-platform helpers and `desktop` for per-OS install/relaunch).
///
/// i64 returns use `I64AsF64` because the Rust impls return `i64` and the
/// codegen converts via `sitofp` to a NaN-boxable JS number. Strings flow
/// through `Str` (raw `*StringHeader` ptr extracted via
/// `js_get_string_pointer_unified` on the codegen side).
pub static PERRY_UPDATER_TABLE: &[MethodRow] = &[
    // perry-updater::core — pure cross-platform helpers.
    MethodRow {
        method: "compareVersions",
        runtime: "perry_updater_compare_versions",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifyHash",
        runtime: "perry_updater_verify_hash",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifySignature",
        runtime: "perry_updater_verify_signature",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "verifySignatureV2",
        runtime: "perry_updater_verify_signature_v2",
        args: &[ArgKind::Str, ArgKind::Str, ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "computeFileSha256",
        runtime: "perry_updater_compute_file_sha256",
        args: &[ArgKind::Str],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "writeSentinel",
        runtime: "perry_updater_write_sentinel",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "readSentinel",
        runtime: "perry_updater_read_sentinel",
        args: &[ArgKind::Str],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "clearSentinel",
        runtime: "perry_updater_clear_sentinel",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    // perry-updater::desktop — platform-touching helpers.
    MethodRow {
        method: "getExePath",
        runtime: "perry_updater_get_exe_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getBackupPath",
        runtime: "perry_updater_get_backup_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "getSentinelPath",
        runtime: "perry_updater_get_sentinel_path",
        args: &[],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "installUpdate",
        runtime: "perry_updater_install",
        args: &[ArgKind::Str, ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "performRollback",
        runtime: "perry_updater_perform_rollback",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    // relaunch returns the spawned PID as f64 (or -1.0 on error).
    MethodRow {
        method: "relaunch",
        runtime: "perry_updater_relaunch",
        args: &[ArgKind::Str],
        ret: ReturnKind::F64,
    },
];

/// perry/media — streaming media playback (`createPlayer`, `play`, `pause`,
/// `seek`, `setVolume`, `onStateChange`, `onTimeUpdate`, `setNowPlaying`,
/// `destroy`). Backed by AVPlayer on Apple, MediaPlayer/JNI on Android,
/// GStreamer on GTK4/Linux, Media Foundation on Windows, and `<audio>` on
/// the web target.
pub static PERRY_MEDIA_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "createPlayer",
        runtime: "perry_media_create_player",
        args: &[ArgKind::Str],
        ret: ReturnKind::I64AsF64,
    },
    MethodRow {
        method: "play",
        runtime: "perry_media_play",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "pause",
        runtime: "perry_media_pause",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "stop",
        runtime: "perry_media_stop",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "seek",
        runtime: "perry_media_seek",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setVolume",
        runtime: "perry_media_set_volume",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setRate",
        runtime: "perry_media_set_rate",
        args: &[ArgKind::F64, ArgKind::F64],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "getCurrentTime",
        runtime: "perry_media_get_current_time",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getDuration",
        runtime: "perry_media_get_duration",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "getState",
        runtime: "perry_media_get_state",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "isPlaying",
        runtime: "perry_media_is_playing",
        args: &[ArgKind::F64],
        ret: ReturnKind::F64,
    },
    MethodRow {
        method: "onStateChange",
        runtime: "perry_media_on_state_change",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "onTimeUpdate",
        runtime: "perry_media_on_time_update",
        args: &[ArgKind::F64, ArgKind::Closure],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "setNowPlaying",
        runtime: "perry_media_set_now_playing",
        args: &[
            ArgKind::F64,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
            ArgKind::Str,
        ],
        ret: ReturnKind::Void,
    },
    MethodRow {
        method: "destroy",
        runtime: "perry_media_destroy",
        args: &[ArgKind::F64],
        ret: ReturnKind::Void,
    },
];

// ─── Lookup helpers ──────────────────────────────────────────────────

/// Look up a TS method name in the receiver-less perry/ui table.
pub fn perry_ui_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_UI_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the receiver-based perry/ui instance table.
pub fn perry_ui_instance_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_UI_INSTANCE_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the perry/system table.
pub fn perry_system_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_SYSTEM_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the perry/i18n table.
pub fn perry_i18n_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_I18N_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the perry/updater table.
pub fn perry_updater_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_UPDATER_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the perry/media table.
pub fn perry_media_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_MEDIA_TABLE.iter().find(|s| s.method == method)
}

/// Look up a TS method name in the perry/background table (issue #538).
pub fn perry_background_lookup(method: &str) -> Option<&'static MethodRow> {
    PERRY_BACKGROUND_TABLE.iter().find(|s| s.method == method)
}

/// Resolve a TS method name to its runtime symbol across the perry/ui +
/// perry/ui-instance + perry/system tables (the surfaces JS and WASM
/// currently dispatch on). Returns the **first** matching runtime
/// symbol — table search order is UI → UI_INSTANCE → SYSTEM, mirroring
/// how the LLVM backend tries each table in turn.
///
/// JS / WASM emit code calls this before falling through to its
/// per-backend extras. New methods added to any table here resolve on
/// every target with no further edits.
pub fn ui_method_to_runtime(method: &str) -> Option<&'static str> {
    if let Some(row) = perry_ui_lookup(method) {
        return Some(row.runtime);
    }
    if let Some(row) = perry_ui_instance_lookup(method) {
        return Some(row.runtime);
    }
    if let Some(row) = perry_system_lookup(method) {
        return Some(row.runtime);
    }
    if let Some(row) = perry_media_lookup(method) {
        return Some(row.runtime);
    }
    None
}
