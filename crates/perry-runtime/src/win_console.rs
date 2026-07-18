//! Shared Win32 console helpers (Windows-only).
//!
//! Single home for the `GetStdHandle` / `GetConsoleMode` / `SetConsoleMode` /
//! `GetConsoleScreenBufferInfo` plumbing that was previously duplicated
//! between `perry-stdlib/src/readline.rs` and `perry-runtime/src/tui/input.rs`
//! (#406) and missing entirely from `tty.rs` (its `#[cfg(not(unix))]` arms
//! hardwired isatty → false / winsize → None). Consumers:
//!
//!   - `tty.rs` — `isatty(fd)`, `process.stdout.columns/.rows`,
//!     `process.stdin.setRawMode()`
//!   - `tui/input.rs` — raw-mode toggle for the perry/tui render loop
//!   - `gc::js_gc_init` — one-shot `enable_vt_output()` at program startup
//!
//! Everything here is best-effort and console-only: on piped or redirected
//! std handles `GetConsoleMode` fails, every probe returns `None`/`false`,
//! and nothing is modified — matching Node, where a non-console std stream
//! is a pipe/file, `isTTY` is falsy, and no console mode is ever touched.

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle, SetConsoleMode,
    CONSOLE_SCREEN_BUFFER_INFO, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_ERROR_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

/// Saved cooked-mode console mode for the stdin handle. Set on the first
/// successful `set_stdin_raw(true)`; restored by `set_stdin_raw(false)`.
/// Mirrors the Unix arms' saved-`termios` contract (save once, restore on
/// disable, "never enabled" disable is a successful no-op).
static SAVED_STDIN_MODE: Mutex<Option<u32>> = Mutex::new(None);

fn std_handle(which: u32) -> Option<HANDLE> {
    // windows-sys 0.61 (#720) made HANDLE a `*mut c_void` (was `isize` in
    // 0.52) — check `.is_null()` + `INVALID_HANDLE_VALUE`, not raw integers.
    let handle = unsafe { GetStdHandle(which) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        None
    } else {
        Some(handle)
    }
}

pub fn stdin_handle() -> Option<HANDLE> {
    std_handle(STD_INPUT_HANDLE)
}

pub fn stdout_handle() -> Option<HANDLE> {
    std_handle(STD_OUTPUT_HANDLE)
}

pub fn stderr_handle() -> Option<HANDLE> {
    std_handle(STD_ERROR_HANDLE)
}

/// Map a Node-style fd number to its process std handle. Only 0/1/2 have
/// std handles; anything else has no console mapping here.
fn handle_for_fd(fd: i32) -> Option<HANDLE> {
    match fd {
        0 => stdin_handle(),
        1 => stdout_handle(),
        2 => stderr_handle(),
        _ => None,
    }
}

/// `GetConsoleMode` probe: `Some(mode)` when the handle is a real console,
/// `None` for pipes, files and the null device. This is the same probe
/// `std::io::IsTerminal` uses (see `builtins/console.rs`), and the same
/// criterion libuv uses to classify a std handle as `UV_TTY`.
pub fn console_mode(handle: HANDLE) -> Option<u32> {
    let mut mode: u32 = 0;
    if unsafe { GetConsoleMode(handle, &mut mode) } != 0 {
        Some(mode)
    } else {
        None
    }
}

fn set_console_mode(handle: HANDLE, mode: u32) -> bool {
    unsafe { SetConsoleMode(handle, mode) != 0 }
}

/// `isatty(fd)` for fd 0/1/2: true iff the std handle is a real console.
/// Piped/redirected handles fail `GetConsoleMode` → false, matching Node.
pub fn is_console_fd(fd: i32) -> bool {
    handle_for_fd(fd).and_then(console_mode).is_some()
}

/// The raw-input flag math shared by every raw-mode consumer, mirroring
/// `perry-stdlib/src/readline.rs` / `tui/input.rs` (#406): drop line
/// buffering, echo and ^C cooking; turn on VT input so arrow keys arrive
/// as ANSI `\x1b[A..D` matching the Unix parsers.
pub fn raw_input_mode(mode: u32) -> u32 {
    (mode & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
        | ENABLE_VIRTUAL_TERMINAL_INPUT
}

/// `process.stdin.setRawMode(enable)` backend. Saves the cooked mode on the
/// first successful enable and restores it on disable. Returns whether the
/// console mode change took effect; always `false` when stdin is not a
/// console (piped/redirected), in which case nothing is saved or modified.
pub fn set_stdin_raw(enable: bool) -> bool {
    if enable {
        let Some(handle) = stdin_handle() else {
            return false;
        };
        let Some(current) = console_mode(handle) else {
            return false;
        };
        {
            let mut saved = SAVED_STDIN_MODE.lock().unwrap();
            if saved.is_none() {
                *saved = Some(current);
            }
        }
        set_console_mode(handle, raw_input_mode(current))
    } else {
        let saved = SAVED_STDIN_MODE.lock().unwrap();
        match saved.as_ref() {
            Some(mode) => match stdin_handle() {
                Some(handle) => set_console_mode(handle, *mode),
                None => false,
            },
            // Never enabled — nothing to restore (matches the Unix arm).
            None => true,
        }
    }
}

/// Terminal window extents from `GetConsoleScreenBufferInfo`'s `srWindow`
/// (the visible window, not the scrollback buffer — matches libuv's
/// `uv_tty_get_winsize`, hence Node's `process.stdout.columns/.rows`).
fn screen_buffer_size(handle: HANDLE) -> Option<(i32, i32)> {
    let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
    if unsafe { GetConsoleScreenBufferInfo(handle, &mut info) } == 0 {
        return None;
    }
    let cols = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
    let rows = i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1;
    (cols > 0 && rows > 0).then_some((cols, rows))
}

/// `(columns, rows)` for an output fd (1 = stdout, 2 = stderr), or `None`
/// when the fd isn't an output std handle or isn't a console. Screen-buffer
/// info only exists for output handles, so fd 0 (and any other fd) is
/// `None` — same shape as Node, where only `WriteStream` has dimensions.
pub fn window_size(fd: i32) -> Option<(i32, i32)> {
    let handle = match fd {
        1 => stdout_handle()?,
        2 => stderr_handle()?,
        _ => return None,
    };
    screen_buffer_size(handle)
}

/// One-shot startup hook: opt console stdout/stderr into VT/ANSI escape
/// processing so runtime-emitted escapes (`console.clear`, the tty cursor
/// ops, color libraries keying off `isTTY`) render instead of printing
/// literally. Best-effort by design:
///
///   - piped/redirected handles fail the `GetConsoleMode` probe and are
///     skipped untouched (a no-op for non-console streams), and
///   - a failing `SetConsoleMode` (pre-VT legacy conhost) is ignored —
///     this must never fail program startup.
///
/// Deliberately does NOT set `DISABLE_NEWLINE_AUTO_RETURN`: the runtime
/// writes bare `\n` line endings via Rust std I/O throughout, so it relies
/// on the console's automatic NL→CRNL return.
pub fn enable_vt_output() {
    for handle in [stdout_handle(), stderr_handle()].into_iter().flatten() {
        if let Some(mode) = console_mode(handle) {
            if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING == 0 {
                let _ = set_console_mode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Console::ENABLE_WINDOW_INPUT;

    #[test]
    fn pipe_handle_is_not_a_console() {
        // Deterministic in every environment: a fresh anonymous pipe can
        // never be a console, so the GetConsoleMode probe (the exact path
        // isatty_impl relies on for redirected std handles) must fail.
        let (reader, writer) = std::io::pipe().expect("create anonymous pipe");
        assert_eq!(console_mode(reader.as_raw_handle()), None);
        assert_eq!(console_mode(writer.as_raw_handle()), None);
        // Likewise no screen-buffer info → columns/rows would be None.
        assert_eq!(screen_buffer_size(writer.as_raw_handle()), None);
    }

    #[test]
    fn raw_input_mode_clears_cooked_flags_and_sets_vt_input() {
        let cooked =
            ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_WINDOW_INPUT;
        let raw = raw_input_mode(cooked);
        assert_eq!(raw & ENABLE_LINE_INPUT, 0);
        assert_eq!(raw & ENABLE_ECHO_INPUT, 0);
        assert_eq!(raw & ENABLE_PROCESSED_INPUT, 0);
        assert_ne!(raw & ENABLE_VIRTUAL_TERMINAL_INPUT, 0);
        // Unrelated bits must survive the toggle.
        assert_ne!(raw & ENABLE_WINDOW_INPUT, 0);
    }

    #[test]
    fn stdin_raw_mode_round_trip_when_console_available() {
        // Only meaningful with a real console on stdin. CI runners and
        // piped invocations have no console — skip gracefully there (the
        // enable path correctly reports failure, which the piped-child
        // test in tty.rs asserts).
        let Some(handle) = stdin_handle() else {
            eprintln!("skipped: no stdin std handle");
            return;
        };
        let Some(original) = console_mode(handle) else {
            eprintln!("skipped: stdin is not a console (expected on CI / piped runs)");
            assert!(!set_stdin_raw(true), "enable must fail without a console");
            return;
        };
        assert!(set_stdin_raw(true), "enable on a real console");
        let raw = console_mode(handle).expect("stdin still a console");
        assert_eq!(raw, raw_input_mode(original));
        assert!(set_stdin_raw(false), "disable restores the saved mode");
        assert_eq!(console_mode(handle), Some(original));
    }
}
