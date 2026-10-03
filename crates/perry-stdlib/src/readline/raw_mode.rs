//! Raw-mode toggle backends, split out of `readline/mod.rs` (#10750).

// ---------------------------------------------------------------------------
// Raw-mode toggle (Unix termios; Windows / non-Unix is currently a no-op
// since iOS/Android stdlib stubs handle those targets and Windows raw mode
// needs the windows-rs `Console` API which isn't a stdlib dep yet).
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[path = "termios_unix.rs"]
pub(super) mod termios_impl;

#[cfg(all(windows, not(unix)))]
#[path = "termios_windows.rs"]
pub(super) mod termios_impl;

#[cfg(not(any(unix, windows)))]
pub(super) mod termios_impl {
    pub fn enable() -> bool {
        // Raw mode unsupported on this platform (e.g. wasm32). The
        // flag still flips so the reader switches to byte-chunk
        // dispatch, but stdin remains line-cooked.
        false
    }
    pub fn disable() -> bool {
        false
    }
}
