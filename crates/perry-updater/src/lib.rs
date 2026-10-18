//! Perry auto-updater.
//!
//! Desktop-only auto-update primitives: manifest semver compare, SHA-256 +
//! Ed25519 signature verification, atomic install with `<exe>.prev` backup,
//! sentinel-file state for crash-loop rollback, and detached relaunch.
//!
//! **Why no mobile**: iOS apps are distributed via App Store / TestFlight
//! (the OS rejects unsigned binary swaps) and Android via Play Store /
//! sideloaded APK installer flows. Self-update is structurally impossible
//! on those platforms — the OS owns the install pipeline. So this crate
//! is unapologetically desktop-only; the same binary still compiles on
//! mobile targets but the FFI symbols are no-ops there (callers should
//! gate the feature with `os.platform()`).
//!
//! Code is split into two internal modules:
//! - `core`: pure cross-platform helpers (no I/O on the executable itself)
//! - `desktop`: per-OS install / relaunch / path resolution, gated by
//!   `cfg(target_os)` and reduced to no-ops on mobile so cross-platform
//!   user code still links.
//!
//! Download lives in TS (using existing `fetch()`) — Rust only handles the
//! security-critical and platform-touching pieces, keeping this crate
//! small and audit-friendly.

mod core;
mod desktop;

// Re-export every FFI symbol so the staticlib bundle picks them up.
// (Rust drops unused module-level pub items from rlib→staticlib unless
// they are reachable from the crate root.)
pub use crate::core::*;
pub use crate::desktop::*;
