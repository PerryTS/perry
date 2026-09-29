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
