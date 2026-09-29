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
