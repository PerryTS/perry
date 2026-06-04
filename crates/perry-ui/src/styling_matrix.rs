//! perry/ui styling parity matrix — single source of truth for issue #185.
//!
//! Every styling-relevant FFI symbol gets one row here, with a per-platform
//! status. The matrix is authored by hand (this file is the spec), and the
//! `--check` mode of `crates/perry-ui/src/bin/styling-matrix.rs` plus the
//! integration test in `crates/perry-ui/tests/styling_matrix_drift.rs`
//! cross-check that each platform's `lib.rs` actually exports the symbols
//! this matrix claims `Wired`. The generator (same binary, `--gen`) writes
//! `docs/src/ui/styling-matrix.md`; CI fails if the file drifts.
//!
//! Sibling matrix: `crates/perry-ui-test/src/lib.rs::FEATURES` covers the
//! broader FFI-parity surface (app lifecycle, widget creation, child mgmt,
//! etc.) across an older 5-platform set. This file is the styling-focused
//! subset structured for issue #185's audit + Phase B gap-closure work,
//! and adds tvOS / visionOS / watchOS columns. Both are valid; they will
//! likely consolidate when Phase C lands the `style: { ... }` API.
//!
//! Adding a new styling FFI: add the row here AND export the symbol from
//! every Wired platform's `lib.rs`. The conformance test catches mismatches.
//!
//! Removing or renaming a styling FFI: update this file in the same commit.
//! The drift check is the safety net.

#![allow(dead_code)]

/// All UI backend platforms Perry currently supports.
///
/// Variant order is the matrix column order in `MatrixRow::statuses` and in
/// the generated markdown. Don't reorder without regenerating the matrix.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    MacOS = 0,
    IOS = 1,
    TVOS = 2,
    VisionOS = 3,
    WatchOS = 4,
    Android = 5,
    Gtk4 = 6,
    Windows = 7,
    Web = 8,
}

impl Platform {
    pub const ALL: &'static [Platform] = &[
        Platform::MacOS,
        Platform::IOS,
        Platform::TVOS,
        Platform::VisionOS,
        Platform::WatchOS,
        Platform::Android,
        Platform::Gtk4,
        Platform::Windows,
        Platform::Web,
    ];

    pub const COUNT: usize = 9;

    pub fn name(self) -> &'static str {
        match self {
            Platform::MacOS => "macOS",
            Platform::IOS => "iOS",
            Platform::TVOS => "tvOS",
            Platform::VisionOS => "visionOS",
            Platform::WatchOS => "watchOS",
            Platform::Android => "Android",
            Platform::Gtk4 => "GTK4",
            Platform::Windows => "Windows",
            Platform::Web => "Web",
        }
    }

    /// Path to the platform's `lib.rs`, relative to the workspace root.
    /// `None` for Web — that backend uses CSS string emission, not a Rust
    /// FFI surface, so the conformance test treats it specially.
    pub fn lib_rs_path(self) -> Option<&'static str> {
        match self {
            Platform::MacOS => Some("crates/perry-ui-macos/src/lib.rs"),
            Platform::IOS => Some("crates/perry-ui-ios/src/lib.rs"),
            Platform::TVOS => Some("crates/perry-ui-tvos/src/lib.rs"),
            Platform::VisionOS => Some("crates/perry-ui-visionos/src/lib.rs"),
            Platform::WatchOS => Some("crates/perry-ui-watchos/src/lib.rs"),
            Platform::Android => Some("crates/perry-ui-android/src/lib.rs"),
            Platform::Gtk4 => Some("crates/perry-ui-gtk4/src/lib.rs"),
            Platform::Windows => Some("crates/perry-ui-windows/src/lib.rs"),
            Platform::Web => None,
        }
    }
}

/// Implementation status of a single (prop × platform) cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Real native impl — symbol exists in `lib.rs` and does the work
    /// against the platform's native APIs (CALayer / GradientDrawable /
    /// GtkCssProvider / etc.).
    Wired,

    /// Symbol exists in `lib.rs` but is a no-op or only partially
    /// implemented. Used when the platform's native APIs make full
    /// support hard but a placeholder lets cross-platform code compile.
    /// Tracked here so Phase B knows what to fix.
    Stub,

    /// Symbol does not exist in `lib.rs` for this platform. User code
    /// that calls the corresponding setter will fail to link unless the
    /// codegen routes it through a no-op shim (which currently it
    /// doesn't — calling a Missing setter is a build error).
    Missing,

    /// The prop genuinely doesn't apply to this platform — e.g., a
    /// macOS-only modal sheet on watchOS. Distinguished from Missing
    /// so the matrix doesn't keep flagging it as a gap.
    NotApplicable,
}

use Status::*;

/// One styling capability — a (widget, prop) pair backed by an FFI symbol.
///
/// `widget = "*"` means the prop applies to every widget kind (the
/// generic `widget_*` setters). Otherwise it's a widget-specific
/// setter like `text_set_color` or `button_set_text_color`.
#[derive(Debug)]
pub struct MatrixRow {
    pub widget: &'static str,
    pub prop: &'static str,
    pub ffi: &'static str,
    /// Per-platform status, indexed by `Platform as usize`. Length = 9.
    pub statuses: [Status; Platform::COUNT],
}

impl MatrixRow {
    pub fn status(&self, plat: Platform) -> Status {
        self.statuses[plat as usize]
    }
}

/// Wired everywhere — including Web, which uses CSS via the WASM
/// emitter dispatch table at `crates/perry-codegen-wasm/src/emit.rs`.
const W_ALL_NATIVE_WEB_TODO: [Status; 9] = [
    Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
];

/// Wired everywhere except GTK4 + Windows (the two desktop backends with
/// the most native-API rough edges) and Web.
const W_NO_GTK4_WIN_WEB: [Status; 9] = [
    Wired, Wired, Wired, Wired, Wired, Wired, Missing, Missing, Missing,
];

/// Wired only on GTK4 + Windows + Apple desktop — the desktop-class targets.
const W_DESKTOP_ONLY: [Status; 9] = [
    Wired,
    NotApplicable,
    NotApplicable,
    NotApplicable,
    NotApplicable,
    NotApplicable,
    Wired,
    Wired,
    NotApplicable,
];

/// Missing everywhere — aspirational rows surfaced for Phase B (shadow,
/// text decoration, etc.).
const M_ALL: [Status; 9] = [Missing; 9];

/// The styling matrix. Each row encodes one (widget, prop, ffi) triple
/// and the per-platform implementation status as of the file's last
/// edit. The conformance test in `perry-ui-test` cross-checks Wired
/// entries against the actual `lib.rs` exports of each platform.
///
/// Rows are grouped by widget for readability:
///   1. `*` — generic widget_* setters
///   2. text, textfield, button, image, stack — widget-specific setters
///   3. Aspirational (Missing-everywhere) rows surfacing Phase B targets
pub const MATRIX: &[MatrixRow] = &[
    // ---- Generic widget styling (apply to any widget) ------------------
    MatrixRow {
        widget: "*",
        prop: "background_color",
        ffi: "perry_ui_widget_set_background_color",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "background_gradient",
        ffi: "perry_ui_widget_set_background_gradient",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "border_color",
        ffi: "perry_ui_widget_set_border_color",
        // Apple + Android wired since baseline. GTK4 wired via per-handle
        // CSS class with joint (color, width) state (v0.5.298). Web wired
        // via new `perry_ui_widget_set_border_color` JS function with
        // module-level Map cache (v0.5.298). Windows wired (#210) via
        // a one-time `SetWindowSubclass` per HWND that draws a `Rectangle`
        // outline in the WM_PAINT subclass proc using the joint
        // `BORDER_STATE` color + width.
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "*",
        prop: "border_width",
        ffi: "perry_ui_widget_set_border_width",
        // Symmetric with `border_color` — both setters share the joint
        // state on each backend so calling either one alone produces a
        // visible border with sensible defaults (black / 1px).
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "*",
        prop: "corner_radius",
        ffi: "perry_ui_widget_set_corner_radius",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "edge_insets",
        ffi: "perry_ui_widget_set_edge_insets",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "opacity",
        ffi: "perry_ui_widget_set_opacity",
        // Apple platforms + Android wired since the audit baseline.
        // GTK4 wired via `Widget::set_opacity` (v0.5.298). Windows wired
        // (#210) via `WS_EX_LAYERED` extended style + per-child
        // `SetLayeredWindowAttributes(LWA_ALPHA)`. Web aliased to the
        // existing `perry_ui_set_opacity` JS function via the WASM
        // emitter dispatch table — no new JS code needed.
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "*",
        prop: "tooltip",
        ffi: "perry_ui_widget_set_tooltip",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "hidden",
        // Canonical FFI name is `set_widget_hidden` (matches the codegen
        // dispatch table at `lower_call.rs::NATIVE_MODULE_TABLE` row for
        // `widgetSetHidden`). The Phase A matrix initially listed the
        // inverted-word-order name `widget_set_hidden`, which only Windows
        // exports as a secondary alias — that mis-named the row and made
        // every other platform appear Missing despite all 8 backends
        // having `set_widget_hidden`. Fixed in v0.5.301.
        ffi: "perry_ui_set_widget_hidden",
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "*",
        prop: "enabled",
        ffi: "perry_ui_widget_set_enabled",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "control_size",
        ffi: "perry_ui_widget_set_control_size",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "hugging",
        ffi: "perry_ui_widget_set_hugging",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "width",
        ffi: "perry_ui_widget_set_width",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "height",
        ffi: "perry_ui_widget_set_height",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "match_parent_width",
        ffi: "perry_ui_widget_match_parent_width",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "match_parent_height",
        ffi: "perry_ui_widget_match_parent_height",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "on_click",
        ffi: "perry_ui_widget_set_on_click",
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "*",
        prop: "on_double_click",
        ffi: "perry_ui_widget_set_on_double_click",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "on_hover",
        ffi: "perry_ui_widget_set_on_hover",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    // Continuous pointer events (issue #1868). Callback receives a
    // PointerEvent { x, y, button, pointerType } object. macOS uses a
    // transparent overlay NSView that subclasses NSView and overrides
    // mouseDown:/mouseUp:/mouseMoved:; other platforms use the native
    // primitive listed in the issue's implementation sketch table.
    MatrixRow {
        widget: "*",
        prop: "on_mouse_down",
        ffi: "perry_ui_widget_set_on_mouse_down",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "on_mouse_up",
        ffi: "perry_ui_widget_set_on_mouse_up",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "on_mouse_move",
        ffi: "perry_ui_widget_set_on_mouse_move",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "animate_opacity",
        ffi: "perry_ui_widget_animate_opacity",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "animate_position",
        ffi: "perry_ui_widget_animate_position",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "*",
        prop: "context_menu",
        ffi: "perry_ui_widget_set_context_menu",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    // ---- text widget styling ------------------------------------------
    MatrixRow {
        widget: "text",
        prop: "color",
        ffi: "perry_ui_text_set_color",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "font_size",
        ffi: "perry_ui_text_set_font_size",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "font_weight",
        ffi: "perry_ui_text_set_font_weight",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "font_family",
        ffi: "perry_ui_text_set_font_family",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "selectable",
        ffi: "perry_ui_text_set_selectable",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "wraps",
        ffi: "perry_ui_text_set_wraps",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "text",
        prop: "text_alignment",
        ffi: "perry_ui_text_set_text_alignment",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    // ---- textfield widget styling -------------------------------------
    MatrixRow {
        widget: "textfield",
        prop: "background_color",
        ffi: "perry_ui_textfield_set_background_color",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "textfield",
        prop: "text_color",
        ffi: "perry_ui_textfield_set_text_color",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "textfield",
        prop: "font_size",
        ffi: "perry_ui_textfield_set_font_size",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "textfield",
        prop: "borderless",
        ffi: "perry_ui_textfield_set_borderless",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    // ---- button widget styling ----------------------------------------
    MatrixRow {
        widget: "button",
        prop: "text_color",
        ffi: "perry_ui_button_set_text_color",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "button",
        prop: "content_tint_color",
        ffi: "perry_ui_button_set_content_tint_color",
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "button",
        prop: "bordered",
        ffi: "perry_ui_button_set_bordered",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "button",
        prop: "image_position",
        ffi: "perry_ui_button_set_image_position",
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    // ---- image widget styling -----------------------------------------
    MatrixRow {
        widget: "image",
        prop: "tint",
        ffi: "perry_ui_image_set_tint",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "image",
        prop: "size",
        ffi: "perry_ui_image_set_size",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    // ---- stack widget styling -----------------------------------------
    MatrixRow {
        widget: "stack",
        prop: "alignment",
        ffi: "perry_ui_stack_set_alignment",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "stack",
        prop: "distribution",
        ffi: "perry_ui_stack_set_distribution",
        statuses: W_ALL_NATIVE_WEB_TODO,
    },
    MatrixRow {
        widget: "stack",
        prop: "detaches_hidden",
        ffi: "perry_ui_stack_set_detaches_hidden",
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    // ---- Aspirational (Phase B targets) -------------------------------
    // (Aspirational rows surface gaps that Phase B will close; their FFI
    // symbol doesn't exist anywhere yet so the drift check skips them.)
    // These FFI symbols don't exist anywhere yet. Listing them surfaces
    // Phase B's gap-closure work. When a platform implements them, flip
    // its cell from Missing to Wired and add the symbol to that
    // backend's lib.rs.
    MatrixRow {
        widget: "*",
        prop: "shadow",
        ffi: "perry_ui_widget_set_shadow",
        // Phase B shadow closure — landed across all 9 platforms in
        // v0.5.296 (Apple CALayer.shadow*) → v0.5.297 (Web CSS
        // `box-shadow`) → v0.5.298 (GTK4 CSS box-shadow + Android
        // `setElevation`). Windows wired in #210 closure (v0.5.401)
        // via a parent-window WM_PAINT subclass that renders the
        // shadow into a 32bpp DIB section + AlphaBlends onto the
        // parent's surface; per-pixel quadratic Gaussian-approx
        // falloff. See `crates/perry-ui-windows/src/widgets/mod.rs`'s
        // `paint_shadow_for_child`.
        // Android honors blur via Material elevation + (API 28+) shadow
        // tinting; offset is intentionally ignored because Android's
        // device-level light source owns shadow direction.
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
    MatrixRow {
        widget: "text",
        prop: "decoration",
        ffi: "perry_ui_text_set_decoration",
        // Issue #185 Phase B closure (v0.5.298). Cross-platform `text-
        // decoration` mapped to each backend's native mechanism: Apple
        // (macOS / iOS / tvOS / visionOS) via `NSAttributedString` with
        // NSUnderline / NSStrikethrough keys; watchOS stores a
        // `text_decoration: i64` field in `NodeData` and the SwiftUI
        // host applies `.underline()` / `.strikethrough()` modifiers;
        // Android via `View.getPaint().setFlags(UNDERLINE_TEXT_FLAG |
        // STRIKE_THRU_TEXT_FLAG)`; GTK4 via Pango `AttrInt::new_underline`
        // and `new_strikethrough`; Web via CSS `text-decoration`.
        // Windows wired (#210) via GetObjectW reading the current LOGFONT,
        // mutating `lfUnderline` / `lfStrikeOut`, recreating via
        // CreateFontIndirectW, and re-emitting through the existing
        // `apply_font` lifecycle (handles old-HFONT DeleteObject).
        statuses: [
            Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired, Wired,
        ],
    },
];

/// Drift-check helpers. Used by the `styling-matrix` binary's `--check`
/// mode and by the `styling_matrix_drift` integration test. Kept in the
/// library so both callers share one canonical scanner implementation.
pub mod drift {
    use super::{Platform, Status, MATRIX};
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    /// Scan a platform crate's `src` tree and return every `perry_ui_*`
    /// symbol it exports as a `pub extern "C" fn`.
    ///
    /// Historically the native backends kept all FFI entry points directly
    /// in `lib.rs`, but the large-file split moved many `#[no_mangle]`
    /// functions into topical `src/ffi*/` or `src/lib_ffi/` modules that are
    /// re-exported or otherwise linker-visible from the crate. Start from the
    /// supplied `lib.rs`, then scan sibling Rust source files recursively so
    /// the drift check follows that split while preserving the old call site.
    /// Handles both multi-line and single-line `#[no_mangle] pub extern "C"
    /// fn ...` styles (watchOS uses single-line for compactness).
    pub fn scan_exports(lib_rs: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let needle = "pub extern \"C\" fn perry_ui_";

        fn scan_file(path: &Path, needle: &str, out: &mut Vec<String>) {
            let Ok(src) = fs::read_to_string(path) else {
                return;
            };
            for line in src.lines() {
                let mut start = 0;
                while let Some(idx) = line[start..].find(needle) {
                    let abs = start + idx + needle.len() - "perry_ui_".len();
                    let rest = &line[abs..];
                    let after_prefix = &rest["perry_ui_".len()..];
                    let end = after_prefix
                        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .unwrap_or(after_prefix.len());
                    out.push(format!("perry_ui_{}", &after_prefix[..end]));
                    start = abs + "perry_ui_".len() + end;
                }
            }
        }

        fn scan_dir(dir: &Path, needle: &str, out: &mut Vec<String>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    scan_dir(&path, needle, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    scan_file(&path, needle, out);
                }
            }
        }

        let Some(src_dir) = lib_rs.parent() else {
            return out;
        };
        scan_dir(src_dir, needle, &mut out);
        out.sort();
        out.dedup();
        out
    }

    /// Per-platform drift report.
    #[derive(Default, Debug)]
    pub struct PlatformDrift {
        pub platform: Option<Platform>,
        /// Matrix says Wired/Stub but the symbol isn't exported.
        pub wired_but_missing: Vec<String>,
        /// Matrix says Missing/NotApplicable but the symbol IS exported.
        /// Only flagged for FFI symbols the matrix tracks — unrelated FFI
        /// (camera, app lifecycle, etc.) intentionally isn't drift here.
        pub present_but_marked_missing: Vec<String>,
    }

    impl PlatformDrift {
        pub fn is_clean(&self) -> bool {
            self.wired_but_missing.is_empty() && self.present_but_marked_missing.is_empty()
        }
    }

    /// Run the drift check for a single platform. Returns `None` for Web
    /// (which has no `lib.rs`).
    pub fn check_platform(plat: Platform, workspace_root: &Path) -> Option<PlatformDrift> {
        let rel = plat.lib_rs_path()?;
        let path = workspace_root.join(rel);
        let actual: Vec<String> = scan_exports(&path);
        let actual_set: HashSet<&str> = actual.iter().map(|s| s.as_str()).collect();

        let mut drift = PlatformDrift {
            platform: Some(plat),
            ..Default::default()
        };
        for row in MATRIX {
            let s = row.status(plat);
            let present = actual_set.contains(row.ffi);
            match (s, present) {
                (Status::Wired | Status::Stub, false) => {
                    drift.wired_but_missing.push(row.ffi.to_string());
                }
                (Status::Missing | Status::NotApplicable, true) => {
                    drift.present_but_marked_missing.push(row.ffi.to_string());
                }
                _ => {}
            }
        }
        Some(drift)
    }

    /// Run the drift check across every platform. The returned vec
    /// excludes Web (which has no `lib.rs`) and any platform whose
    /// `lib.rs` doesn't exist on disk (e.g., during partial workspace
    /// checkouts). Drift-clean platforms are also included so callers
    /// can render a status line per platform.
    pub fn check_all(workspace_root: &Path) -> Vec<PlatformDrift> {
        let mut out = Vec::new();
        for plat in Platform::ALL {
            if let Some(d) = check_platform(*plat, workspace_root) {
                out.push(d);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_has_no_duplicate_ffi_symbols() {
        let mut seen = Vec::new();
        for row in MATRIX {
            assert!(
                !seen.contains(&row.ffi),
                "duplicate FFI symbol in MATRIX: {}",
                row.ffi
            );
            seen.push(row.ffi);
        }
    }

    #[test]
    fn matrix_status_array_length() {
        for row in MATRIX {
            assert_eq!(
                row.statuses.len(),
                Platform::COUNT,
                "row {}: status array wrong length",
                row.ffi
            );
        }
    }

    #[test]
    fn platform_all_count_matches_count_const() {
        assert_eq!(Platform::ALL.len(), Platform::COUNT);
    }

    #[test]
    fn platform_indices_match_enum_values() {
        for (i, p) in Platform::ALL.iter().enumerate() {
            assert_eq!(*p as usize, i);
        }
    }
}
