//! Cross-platform feature parity matrix for Perry UI.
//!
//! Single source of truth for which `perry_ui_*` / `perry_system_*` FFI
//! functions each platform is expected to provide. Tests in
//! `tests/ffi_parity.rs` verify actual source against this matrix.
//!
//! The matrix is split across `features/*.rs` (by topical area) to keep each
//! file readable and well under the 2000-LOC repo limit. `lib.rs` keeps the
//! type definitions, the [`full`] / `S` / `U` shorthands sub-modules import,
//! and the public re-exports.

use std::sync::LazyLock;

mod features;

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

/// One row of the cross-platform support matrix.
///
/// `Copy + Clone` is intentional — the sub-module slices get concatenated
/// into [`FEATURES`] at first access, which needs `Clone`. All fields are
/// `Copy` so the derive is free.
#[derive(Debug, Clone, Copy)]
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

// Shorthand aliases used by the sub-module matrices.
pub(crate) const S: Support = Supported;
pub(crate) const U: Support = Unsupported;

/// Compact constructor for features that are `Supported` on every platform.
/// Keeps each row to a single line when the support row is uniform.
pub(crate) const fn full(name: &'static str, category: Category) -> Feature {
    Feature {
        name,
        category,
        macos: S,
        ios: S,
        android: S,
        gtk4: S,
        windows: S,
        web: S,
        web_name: None,
    }
}

/// Complete feature matrix. Every `perry_ui_*` / `perry_system_*` function
/// across all platforms is listed here. The macOS naming convention is
/// canonical.
///
/// # Conventions
/// - `web_name: Some("...")` when the web runtime uses a different JS
///   function name
/// - `web_name: None` when web uses the same name as native, or when web
///   is `Unsupported`
///
/// # Lazy concatenation
/// The matrix is partitioned across `features::{app,widgets,state,styling,
/// interaction,system}::ROWS`. On first access this `LazyLock` flattens all
/// six slices into a single `Vec<Feature>` — one heap allocation for the
/// life of the process. Consumers use `FEATURES.iter()` / `FEATURES.len()`
/// just like the old `&[Feature]` API.
pub static FEATURES: LazyLock<Vec<Feature>> = LazyLock::new(|| features::ALL.concat());

/// Returns features filtered by category, sorted by name.
pub fn features_by_category(cat: Category) -> Vec<&'static Feature> {
    // FEATURES is a `static`, so each element's reference is `'static`.
    let mut v: Vec<&'static Feature> = FEATURES.iter().filter(|f| f.category == cat).collect();
    v.sort_by_key(|f| f.name);
    v
}

/// All categories in display order.
pub const CATEGORY_ORDER: &[Category] = &[
    AppLifecycle,
    Timer,
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
    Layout,
    ForEach,
    Navigation,
    Picker,
    Image,
    ProgressView,
    Advanced,
    SystemApi,
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
