//! Kill and signal delivery machinery, split out of `reactor.rs` to keep it under the 2000-line cap.
//! A child module, so `use super::*` reaches the reactor's private state.

use super::*;

/// Duplicate `child`'s process handle before the `Child` moves to the waiter
/// thread, so the kill paths can act on the process object itself rather than
/// the recyclable pid (see `cp_win_kill`). The registry entry owns the
/// duplicate; `LiveChild::drop` closes it. Returns `0` when duplication fails
/// — kills on that child then report undelivered rather than falling back to
/// a racy pid-based kill.
#[cfg(windows)]
pub(super) fn cp_win_dup_proc_handle(child: &Child) -> isize {
    use std::os::windows::io::AsRawHandle;
    cp_win_dup_raw_proc_handle(child.as_raw_handle())
}

#[cfg(windows)]
pub(super) fn cp_win_dup_raw_proc_handle(raw: std::os::windows::io::RawHandle) -> isize {
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut dup: HANDLE = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            raw as HANDLE,
            GetCurrentProcess(),
            &mut dup,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok != 0 {
        dup as isize
    } else {
        0
    }
}

/// Terminate (or probe) a live child on Windows through the process handle
/// duplicated at spawn time — the structural analogue of the unix arm's
/// `libc::kill(pid, sig)`, and the same strategy as libuv's
/// `uv_process_kill`. Acting on the held handle (never the pid) closes the
/// pid-reuse race: the waiter thread may already have reaped the `Child` —
/// freeing the pid for OS reuse — before its `Exited` event reaches the pump,
/// but the duplicate keeps naming the original process object forever, so a
/// recycled pid can never be terminated by mistake.
///
/// * `signum == 0` — the POSIX existence probe: no side effect, just "is the
///   process still alive?" (`GetExitCodeProcess` still reporting
///   `STILL_ACTIVE`).
/// * any other signal — degrades to `TerminateProcess(handle, 1)`, exactly
///   like Node on Windows (there are no POSIX signals to deliver). On an
///   already-exited process `TerminateProcess` fails, so the kill correctly
///   reports undelivered.
///
/// Returns whether the operation succeeded (the `libc::kill(..) == 0`
/// analogue). The terminated child is reaped by the waiter thread as usual —
/// `Child::wait()` returns once the process dies — so the existing
/// Eof → Exited → exit/close pipeline completes naturally.
#[cfg(windows)]
pub(super) fn cp_win_kill(proc_handle: isize, signum: i32) -> bool {
    use windows_sys::Win32::Foundation::{HANDLE, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, TerminateProcess};
    if proc_handle == 0 {
        return false; // spawn-time DuplicateHandle failed — nothing to act on
    }
    let handle = proc_handle as HANDLE;
    if signum == 0 {
        let mut code: u32 = 0;
        return unsafe { GetExitCodeProcess(handle, &mut code) } != 0
            && code == STILL_ACTIVE as u32;
    }
    unsafe { TerminateProcess(handle, 1) != 0 }
}

/// Record a successful Windows termination so the waiter's eventual `Exited`
/// event reports Node's `(code: null, signal: <requested>)` shape instead of
/// the synthetic `TerminateProcess` exit code — see
/// `LiveChild::win_kill_signal`.
#[cfg(windows)]
pub(super) fn cp_note_win_kill(handle: u64, signum: i32) {
    if signum == 0 {
        return; // sig-0 probe — nothing was terminated
    }
    if let Some(map) = cp_live_lock().as_mut() {
        if let Some(lc) = map.get_mut(&handle) {
            lc.win_kill_signal = Some(signum);
        }
    }
}

/// What the platform kill primitive acts on: the pid for unix `libc::kill`,
/// the spawn-time duplicated process handle on Windows (immune to pid reuse —
/// see `cp_win_kill`).
#[cfg(not(windows))]
#[inline]
pub(super) fn cp_kill_target(lc: &LiveChild) -> i32 {
    lc.pid
}
#[cfg(windows)]
#[inline]
pub(super) fn cp_kill_target(lc: &LiveChild) -> isize {
    lc.win_proc_handle
}

pub(super) fn cp_live_kill_signum(handle: u64, signum: i32) -> Option<u64> {
    let (target, cp_bits) = {
        let guard = cp_live_lock();
        match guard.as_ref().and_then(|map| map.get(&handle)) {
            // Skip if already reaped — the pid may have been recycled by the OS.
            Some(lc) if lc.exited.is_none() => (cp_kill_target(lc), lc.cp_bits),
            _ => return None,
        }
    };
    #[cfg(unix)]
    {
        if unsafe { libc::kill(target, signum) == 0 } {
            Some(cp_bits)
        } else {
            None
        }
    }
    #[cfg(windows)]
    {
        if cp_win_kill(target, signum) {
            cp_note_win_kill(handle, signum);
            Some(cp_bits)
        } else {
            None
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, signum, cp_bits);
        None
    }
}

/// Signal a live child. `signal` is the JS `kill([signal])` argument (a signal
/// name string, a number, or — for the no-arg / default case — undefined or the
/// `0.0` arg-padding, both treated as `SIGTERM`). Returns whether the signal
/// was delivered.
pub(crate) fn cp_live_kill(handle: u64, signal: f64) -> bool {
    cp_live_kill_signum(handle, cp_signal_from_value(signal)).is_some()
}

pub(super) fn cp_live_kill_signal(handle: u64, signum: i32) -> bool {
    let target = {
        let guard = cp_live_lock();
        match guard.as_ref().and_then(|map| map.get(&handle)) {
            // Skip if already reaped — the pid may have been recycled by the OS.
            Some(lc) if lc.exited.is_none() => cp_kill_target(lc),
            _ => return false,
        }
    };
    #[cfg(unix)]
    {
        unsafe { libc::kill(target, signum) == 0 }
    }
    #[cfg(windows)]
    {
        if cp_win_kill(target, signum) {
            cp_note_win_kill(handle, signum);
            true
        } else {
            false
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (target, signum);
        false
    }
}

/// Map a JS `kill` signal argument to a Unix signal number. Default / no-arg
/// (`undefined` or the `0.0` padding) → `SIGTERM`.
#[cfg(unix)]
pub(super) fn cp_parse_signal(signal: f64) -> i32 {
    const SIGTERM: i32 = libc::SIGTERM;
    if JSValue::from_bits(signal.to_bits()).is_undefined() {
        return SIGTERM;
    }
    // Numeric forms BEFORE the string lookup: the unified string accessor
    // coerces numbers to "9"-style strings, which are not signal names; an
    // int32 can also arrive NaN-boxed, which `is_finite()` alone misses.
    let js = JSValue::from_bits(signal.to_bits());
    if js.is_int32() {
        let n = js.as_int32();
        return if n == 0 { SIGTERM } else { n };
    }
    if signal.is_finite() {
        let n = signal as i32;
        // 0 is the "no-arg" padding sentinel — treat as the default SIGTERM.
        return if n == 0 { SIGTERM } else { n };
    }
    if let Some(name) = cp_value_to_string(signal) {
        return cp_signal_number(&name).unwrap_or(SIGTERM);
    }
    SIGTERM
}

/// Inverse of `super::cp_signal_name` for the common signals.
#[cfg(unix)]
pub(super) fn cp_signal_number(name: &str) -> Option<i32> {
    Some(match name {
        "SIGHUP" => libc::SIGHUP,
        "SIGINT" => libc::SIGINT,
        "SIGQUIT" => libc::SIGQUIT,
        "SIGABRT" => libc::SIGABRT,
        "SIGKILL" => libc::SIGKILL,
        "SIGTERM" => libc::SIGTERM,
        "SIGUSR1" => libc::SIGUSR1,
        "SIGUSR2" => libc::SIGUSR2,
        "SIGSTOP" => libc::SIGSTOP,
        "SIGCONT" => libc::SIGCONT,
        _ => return None,
    })
}
