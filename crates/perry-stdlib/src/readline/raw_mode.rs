//! Raw-mode toggle for `process.stdin.setRawMode` — the per-platform
//! `termios_impl` backends. Split out of `readline/mod.rs` to stay under the
//! 2,000-line cap (#10750).

// ---------------------------------------------------------------------------
// Raw-mode toggle (Unix termios; Windows / non-Unix is currently a no-op
// since iOS/Android stdlib stubs handle those targets and Windows raw mode
// needs the windows-rs `Console` API which isn't a stdlib dep yet).
// ---------------------------------------------------------------------------

#[cfg(unix)]
pub(super) mod termios_impl {
    use std::sync::Mutex;

    /// Saved cooked-mode termios so we can restore on disable. Lazy-init
    /// on the first enable call; survives toggle cycles.
    static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

    /// Enable raw mode on fd 0 (stdin). Returns true on success.
    pub fn enable() -> bool {
        unsafe {
            let mut current: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut current) != 0 {
                return false;
            }
            // Save the original on first enable so disable can restore.
            {
                let mut saved = SAVED.lock().unwrap_or_else(|p| p.into_inner());
                if saved.is_none() {
                    *saved = Some(current);
                }
            }
            let mut raw = current;
            // cfmakeraw equivalent (Node's setRawMode does roughly this).
            raw.c_iflag &= !(libc::IGNBRK
                | libc::BRKINT
                | libc::PARMRK
                | libc::ISTRIP
                | libc::INLCR
                | libc::IGNCR
                | libc::ICRNL
                | libc::IXON);
            raw.c_oflag &= !libc::OPOST;
            raw.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
            raw.c_cflag &= !(libc::CSIZE | libc::PARENB);
            raw.c_cflag |= libc::CS8;
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSANOW, &raw) == 0
        }
    }

    /// Disable raw mode (restore the saved cooked-mode termios).
    pub fn disable() -> bool {
        unsafe {
            let saved = SAVED.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(t) = saved.as_ref() {
                libc::tcsetattr(0, libc::TCSANOW, t) == 0
            } else {
                // Never enabled — nothing to restore.
                true
            }
        }
    }
}

#[cfg(all(windows, not(unix)))]
pub(super) mod termios_impl {
    use std::sync::Mutex;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT,
        ENABLE_PROCESSED_INPUT, ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// Saved console modes for the input + output handles. Set on first
    /// `enable()`; restored by `disable()`. Two-tuple so we can leave
    /// the output handle's mode untouched if we couldn't read it (e.g.
    /// stdout redirected to a file — `GetConsoleMode` fails on
    /// non-console handles).
    static SAVED: Mutex<Option<(u32, Option<u32>)>> = Mutex::new(None);

    /// Flip stdin into byte-mode + virtual-terminal-input mode (so
    /// arrow keys arrive as ANSI `\x1b[A..D` matching the Unix path's
    /// parser) and stdout into virtual-terminal-processing mode (so the
    /// renderer's CSI escapes actually move the cursor instead of
    /// printing literally). Saves the original modes on first call so
    /// `disable()` restores cleanly. (#406.)
    pub fn enable() -> bool {
        unsafe {
            // windows-sys 0.61 (#720) made HANDLE a `*mut c_void` (was `isize`
            // in 0.52). Use `.is_null()` + `INVALID_HANDLE_VALUE` constant
            // instead of raw integer comparison. (#406 fix updated for
            // windows-sys 0.61.)
            use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
            let h_in = GetStdHandle(STD_INPUT_HANDLE);
            if h_in.is_null() || h_in == INVALID_HANDLE_VALUE {
                return false;
            }
            let mut current_in: u32 = 0;
            if GetConsoleMode(h_in, &mut current_in) == 0 {
                return false;
            }
            let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
            let current_out = if !h_out.is_null() && h_out != INVALID_HANDLE_VALUE {
                let mut m: u32 = 0;
                if GetConsoleMode(h_out, &mut m) != 0 {
                    Some(m)
                } else {
                    None
                }
            } else {
                None
            };

            {
                let mut saved = SAVED.lock().unwrap_or_else(|p| p.into_inner());
                if saved.is_none() {
                    *saved = Some((current_in, current_out));
                }
            }

            let raw_in = (current_in
                & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            if SetConsoleMode(h_in, raw_in) == 0 {
                return false;
            }
            if let Some(out_mode) = current_out {
                let raw_out = out_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
                let _ = SetConsoleMode(h_out, raw_out);
            }
            true
        }
    }

    pub fn disable() -> bool {
        unsafe {
            let saved = SAVED.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((in_mode, out_mode)) = saved.as_ref() {
                use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
                let h_in = GetStdHandle(STD_INPUT_HANDLE);
                if !h_in.is_null() && h_in != INVALID_HANDLE_VALUE {
                    let _ = SetConsoleMode(h_in, *in_mode);
                }
                if let Some(m) = out_mode {
                    let h_out = GetStdHandle(STD_OUTPUT_HANDLE);
                    if !h_out.is_null() && h_out != INVALID_HANDLE_VALUE {
                        let _ = SetConsoleMode(h_out, *m);
                    }
                }
                true
            } else {
                true
            }
        }
    }
}

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
