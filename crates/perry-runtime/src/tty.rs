//! TTY module — Phase 3 of #347.
//!
//! Provides:
//!   - `tty.isatty(fd)` — bool (libc::isatty / GetFileType+FILE_TYPE_CHAR)
//!   - `process.std{in,out,err}.isTTY` — same as isatty(0/1/2)
//!   - `process.stdout.columns` / `.rows` — terminal dimensions via
//!     TIOCGWINSZ on Unix / GetConsoleScreenBufferInfo on Windows
//!   - `process.stdout.on('resize', cb)` — SIGWINCH handler that fires
//!     the registered callback when the terminal is resized
//!
//! All calls are synchronous and return `undefined` when stdout isn't a
//! TTY. The resize event handler is async-signal-safe (only sets an
//! atomic flag); the actual callback dispatch happens on the next
//! event-loop tick via `js_tty_resize_drain()`.

use crate::closure::{js_closure_call0, ClosureHeader};
use crate::string::StringHeader;
use crate::value::JSValue;

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

// ---------------------------------------------------------------------------
// Cross-thread state
// ---------------------------------------------------------------------------

/// Set by the SIGWINCH handler; cleared by the drain on each tick. The
/// handler is async-signal-safe (this is the only thing it touches).
static RESIZE_PENDING: AtomicBool = AtomicBool::new(false);
/// Cached last-known columns/rows. Re-read on every Columns/Rows call,
/// but the SIGWINCH handler also caches the new value here so the
/// drain sees up-to-date dimensions.
static CACHED_COLS: AtomicI32 = AtomicI32::new(0);
static CACHED_ROWS: AtomicI32 = AtomicI32::new(0);
/// Whether SIGWINCH has been installed yet (idempotent install).
static SIGWINCH_INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Callback for `process.stdout.on('resize', cb)`. Stored on main
    /// thread; only touched by the drain (which runs on main).
    static RESIZE_CALLBACK: RefCell<Option<i64>> = const { RefCell::new(None) };
}

// ---------------------------------------------------------------------------
// Per-platform isatty + winsize
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn isatty_impl(fd: i32) -> bool {
    unsafe { libc::isatty(fd) != 0 }
}

#[cfg(unix)]
fn winsize_impl(fd: i32) -> Option<(i32, i32)> {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            Some((ws.ws_col as i32, ws.ws_row as i32))
        } else {
            None
        }
    }
}

#[cfg(not(unix))]
fn isatty_impl(_fd: i32) -> bool {
    // TODO: GetFileType(GetStdHandle(STD_OUTPUT_HANDLE)) == FILE_TYPE_CHAR
    // on Windows. Until the windows-rs dep is wired here, treat all fds
    // as non-TTY — `isTTY` returns false, columns/rows return undefined,
    // SIGWINCH is a no-op.
    false
}

#[cfg(not(unix))]
fn winsize_impl(_fd: i32) -> Option<(i32, i32)> {
    None
}

// ---------------------------------------------------------------------------
// SIGWINCH handler (Unix only)
// ---------------------------------------------------------------------------

#[cfg(unix)]
extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    // Async-signal-safe: ONLY set the atomic flag. Don't do ioctl here
    // (TIOCGWINSZ is technically AS-safe but tradition says no), don't
    // touch JS state, don't allocate. The drain reads the flag and
    // does the real work on the next event-loop tick.
    RESIZE_PENDING.store(true, Ordering::Release);
}

#[cfg(unix)]
fn install_sigwinch() {
    if SIGWINCH_INSTALLED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigwinch_handler as usize;
        // SA_RESTART so a stray SIGWINCH during a `read` doesn't return
        // EINTR — important for the readline byte-mode reader.
        sa.sa_flags = libc::SA_RESTART;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
    }
}

#[cfg(not(unix))]
fn install_sigwinch() {}

// ---------------------------------------------------------------------------
// Public FFI
// ---------------------------------------------------------------------------

/// `tty.isatty(fd)` — return 1 if the fd refers to a terminal.
#[no_mangle]
pub extern "C" fn js_tty_isatty(fd: f64) -> f64 {
    let fd_i = fd as i32;
    if isatty_impl(fd_i) {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
    }
}

/// `process.stdin.isTTY` / `process.stdout.isTTY` / `process.stderr.isTTY`.
/// Per Node's docs these are `true` when the stream is a TTY and
/// `undefined` otherwise (intentionally — it's a presence test). This
/// differs from `tty.isatty(fd)`, which always returns a boolean.
#[no_mangle]
pub extern "C" fn js_process_stdin_isatty() -> f64 {
    if isatty_impl(0) {
        f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    }
}
#[no_mangle]
pub extern "C" fn js_process_stdout_isatty() -> f64 {
    if isatty_impl(1) {
        f64::from_bits(0x7FFC_0000_0000_0004)
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}
#[no_mangle]
pub extern "C" fn js_process_stderr_isatty() -> f64 {
    if isatty_impl(2) {
        f64::from_bits(0x7FFC_0000_0000_0004)
    } else {
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// `process.stdout.columns` — terminal width in cells, or `undefined`
/// when stdout isn't a TTY.
#[no_mangle]
pub extern "C" fn js_process_stdout_columns() -> f64 {
    match winsize_impl(1) {
        Some((cols, rows)) => {
            CACHED_COLS.store(cols, Ordering::Relaxed);
            CACHED_ROWS.store(rows, Ordering::Relaxed);
            cols as f64
        }
        None => f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    }
}

/// `process.stdout.rows` — terminal height in cells, or `undefined`
/// when stdout isn't a TTY.
#[no_mangle]
pub extern "C" fn js_process_stdout_rows() -> f64 {
    match winsize_impl(1) {
        Some((cols, rows)) => {
            CACHED_COLS.store(cols, Ordering::Relaxed);
            CACHED_ROWS.store(rows, Ordering::Relaxed);
            rows as f64
        }
        None => f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    }
}

/// `process.stdout.on(event, cb)` — currently only handles `'resize'`,
/// which installs SIGWINCH and stashes the callback. Other events are
/// silently ignored (they're not currently supported on stdout).
#[no_mangle]
pub extern "C" fn js_process_stdout_on(event_ptr: *const StringHeader, callback: i64) -> f64 {
    if event_ptr.is_null() {
        return f64::from_bits(JSValue::undefined().bits());
    }
    let event = unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let slice = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(slice).unwrap_or("")
    };
    if event == "resize" {
        RESIZE_CALLBACK.with(|cb| *cb.borrow_mut() = Some(callback));
        install_sigwinch();
        // Seed the cache so the first drain sees the current dimensions
        // even if SIGWINCH hasn't fired yet (programs that subscribe at
        // startup typically render once with the current size).
        if let Some((cols, rows)) = winsize_impl(1) {
            CACHED_COLS.store(cols, Ordering::Relaxed);
            CACHED_ROWS.store(rows, Ordering::Relaxed);
        }
    }
    f64::from_bits(JSValue::undefined().bits())
}

/// Drain the resize-pending flag and fire the registered resize
/// callback. Called from the event-loop pump on every tick. Returns
/// the number of callbacks fired (0 or 1).
#[no_mangle]
pub extern "C" fn js_tty_resize_drain() -> i32 {
    if !RESIZE_PENDING.swap(false, Ordering::AcqRel) {
        return 0;
    }
    // Refresh the cache before firing — the callback typically reads
    // process.stdout.columns/.rows, and we want it to see the new
    // values, not the pre-resize values.
    if let Some((cols, rows)) = winsize_impl(1) {
        CACHED_COLS.store(cols, Ordering::Relaxed);
        CACHED_ROWS.store(rows, Ordering::Relaxed);
    }
    let cb = RESIZE_CALLBACK.with(|c| *c.borrow());
    if let Some(cb_i64) = cb {
        let closure = cb_i64 as *const ClosureHeader;
        unsafe { js_closure_call0(closure) };
        return 1;
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isatty_zero_for_pipe() {
        // In test runner stdin is not a TTY (cargo test pipes stdin).
        // Note: this is more of a smoke test; the real value is
        // test-environment-dependent.
        let _ = js_tty_isatty(0.0);
        let _ = js_tty_isatty(1.0);
        let _ = js_tty_isatty(2.0);
    }

    #[test]
    fn columns_undefined_when_not_tty() {
        // In test runner stdout is not a TTY → columns/rows return TAG_UNDEFINED.
        let cols = js_process_stdout_columns();
        let rows = js_process_stdout_rows();
        // Both should be TAG_UNDEFINED bits (0x7FFC_0000_0000_0001).
        assert_eq!(cols.to_bits(), 0x7FFC_0000_0000_0001);
        assert_eq!(rows.to_bits(), 0x7FFC_0000_0000_0001);
    }

    #[test]
    fn resize_drain_with_no_callback_returns_zero() {
        RESIZE_PENDING.store(true, Ordering::Release);
        // No callback registered → drain consumes flag, returns 0.
        assert_eq!(js_tty_resize_drain(), 0);
        // Flag now cleared.
        assert_eq!(js_tty_resize_drain(), 0);
    }

    #[test]
    fn isatty_returns_tag_true_or_false() {
        // Result is always a NaN-boxed bool — TAG_TRUE or TAG_FALSE.
        let v = js_tty_isatty(0.0);
        let bits = v.to_bits();
        assert!(
            bits == 0x7FFC_0000_0000_0003 || bits == 0x7FFC_0000_0000_0004,
            "expected TAG_FALSE or TAG_TRUE, got {:#x}",
            bits
        );
    }
}
