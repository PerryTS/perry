//! The WASIp2 stdin service (#13953).
//!
//! A WASI 0.2 component has no threads, so `process.stdin` cannot have a
//! reader thread as it does on native targets: `std::thread::spawn` fails and
//! the spawn panicked. As on preview 1 (`p1.rs`), the event loop watches fd 0
//! itself: the ordinary pump asks with a zero-timeout `poll`, and an otherwise
//! idle loop waits in one `poll` on fd 0 instead of a timed sleep.

use super::STATE;
use std::time::Duration;

extern "C" {
    fn perry_wasi_stdin_poll(timeout_ms: i32) -> i32;
}

/// Service stdin without waiting, so a loop that never parks (a self-rearming
/// `setImmediate`) still receives piped input and its EOF.
pub(crate) fn poll_stdin() {
    if crate::os::wasi_stdin_wanted() && unsafe { perry_wasi_stdin_poll(0) } > 0 {
        crate::os::wasi_stdin_ready();
    }
}

/// Wait at most `budget` for stdin. Returns false, having waited for nothing,
/// when the program does not read stdin, when sockets are open (their bounded
/// 1 ms park also runs the pump, which polls stdin; a `node:dgram` socket
/// bounds it the same way), or when the poll failed; the caller then uses its
/// ordinary timed sleep.
pub(crate) fn park(budget: Duration) -> bool {
    if !crate::os::wasi_stdin_wanted()
        || STATE.with(|s| !s.borrow().handles.is_empty())
        || crate::module_hooks::dgram_next_wake_ms() >= 0.0
    {
        return false;
    }
    let timeout = budget.as_millis().min(i32::MAX as u128) as i32;
    let result = unsafe { perry_wasi_stdin_poll(timeout) };
    if result > 0 {
        crate::os::wasi_stdin_ready();
    }
    result >= 0
}
