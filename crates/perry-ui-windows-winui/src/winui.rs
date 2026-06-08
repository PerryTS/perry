//! WinUI 3 / Fluent backend internals — issue #4680.
//!
//! This module is where the Win32 widget creation is progressively replaced by
//! `Microsoft.UI.Xaml` controls. It is empty of real XAML today (scaffold);
//! see the crate-level docs for the incremental plan. Each future widget gets a
//! submodule here that drives the corresponding XAML control and is wired into
//! the dispatch path in place of the `perry-ui-windows` Win32 path.

/// Windows App SDK bootstrap (#4680 step 2).
///
/// A WinUI 3 / unpackaged app must initialize the Windows App SDK runtime
/// before any `Microsoft.UI.Xaml` type is constructed — normally via the
/// bootstrapper API (`MddBootstrapInitialize2` from
/// `Microsoft.WindowsAppRuntime.Bootstrap`, acquired as a redistributable, not
/// a cargo crate).
///
/// This is a **stub**: it links nothing and is a no-op, so the scaffold builds
/// and runs anywhere (the Win32 re-export path needs no runtime). When the
/// XAML mapping lands, this becomes the real `MddBootstrapInitialize2` call,
/// gated behind the WinAppSDK link dependency, and is invoked from app startup
/// before the first XAML object is created.
pub mod bootstrap {
    /// Outcome of attempting to initialize the Windows App SDK runtime.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InitStatus {
        /// Runtime is ready (or not yet required — current scaffold state).
        Ready,
        /// The Windows App SDK runtime is not installed; the caller should
        /// fall back to the Win32 backend rather than crash.
        RuntimeMissing,
    }

    /// Initialize the Windows App SDK runtime. Currently a no-op returning
    /// [`InitStatus::Ready`] because the scaffold renders through Win32 and
    /// needs no WinAppSDK runtime. Replaced by the real bootstrapper call in
    /// #4680 step 2.
    pub fn initialize() -> InitStatus {
        InitStatus::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::bootstrap::{initialize, InitStatus};

    #[test]
    fn bootstrap_scaffold_is_ready() {
        // The scaffold must never report a missing runtime — it renders via
        // the re-exported Win32 backend, which has no WinAppSDK dependency.
        assert_eq!(initialize(), InitStatus::Ready);
    }
}
