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

// Topical sub-modules — each owns one dispatch table. Public items are
// re-exported below so consumers keep using `perry_dispatch::PERRY_*`.
mod background_table;
mod i18n_table;
mod media_table;
mod system_table;
mod ui_instance_table;
mod ui_table;
mod updater_table;

pub use background_table::PERRY_BACKGROUND_TABLE;
pub use i18n_table::PERRY_I18N_TABLE;
pub use media_table::PERRY_MEDIA_TABLE;
pub use system_table::PERRY_SYSTEM_TABLE;
pub use ui_instance_table::PERRY_UI_INSTANCE_TABLE;
pub use ui_table::PERRY_UI_TABLE;
pub use updater_table::PERRY_UPDATER_TABLE;

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
