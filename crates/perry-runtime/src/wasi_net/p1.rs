//! The `wasm32-wasip1` event-loop park and the embedder's signal stream.
//!
//! A preview 1 guest has no helper threads, so the loop sleeps in one
//! `poll_oneoff`: until a socket it waits on is ready, a host signal arrives,
//! stdin has data (when the program reads it) or the next timer deadline
//! passes. WASIp2 keeps its bounded 1 ms park.

use super::STATE;
use std::time::Duration;

extern "C" {
    fn perry_wasi_signal_fd() -> i32;
    fn perry_wasi_signal_retire();
    fn perry_wasi_stream_read(fd: i32, bytes: *mut u8, len: u32) -> i32;
    fn perry_wasi_poll(
        fds: *const i32,
        kinds: *const u8,
        n: u32,
        timeout_ns: u64,
        ready: *mut u8,
    ) -> i32;
}

const READ: u8 = 1;
const WRITE: u8 = 2;
const STDIN_FD: i32 = 0;

/// Read the signals the embedder queued (one byte each) and deliver them. On
/// EOF or a read failure the stream is closed and never polled again.
pub(crate) fn drain_signals() {
    let fd = unsafe { perry_wasi_signal_fd() };
    if fd < 0 {
        return;
    }
    let mut bytes = [0u8; 32];
    loop {
        let n = unsafe { perry_wasi_stream_read(fd, bytes.as_mut_ptr(), bytes.len() as u32) };
        if n > 0 {
            for &code in &bytes[..n as usize] {
                crate::os::signal::wasi_deliver(code);
            }
            continue;
        }
        if n == -libc::EAGAIN || n == -libc::EINTR {
            return;
        }
        unsafe { perry_wasi_signal_retire() };
        return;
    }
}

/// Service stdin without waiting: a loop that never parks (a self-rearming
/// `setImmediate`) must still receive piped input and its EOF, so the ordinary
/// pump asks fd 0 whether it is readable with a zero-timeout `poll_oneoff`.
pub(crate) fn poll_stdin() {
    if !crate::os::p1_stdin_wanted() {
        return;
    }
    let (kind, mut ready) = (READ, 0u8);
    let result = unsafe { perry_wasi_poll(&STDIN_FD, &kind, 1, 0, &mut ready) };
    if result > 0 && ready != 0 {
        crate::os::p1_stdin_ready();
    }
}

/// Sleep for at most `budget` in one `poll_oneoff`. Returns false, having slept
/// nothing, when there is nothing to wait on or the poll failed; the caller
/// then uses its ordinary timed sleep.
pub(crate) fn park(budget: Duration) -> bool {
    let signal = unsafe { perry_wasi_signal_fd() };
    let stdin = crate::os::p1_stdin_wanted();
    let mut fds = Vec::new();
    let mut kinds = Vec::new();
    STATE.with(|s| {
        for h in s.borrow().handles.values() {
            let mut kind = 0u8;
            // A pending connect wakes the loop when the host settles it; the
            // host reports a connecting descriptor writable only then.
            if !h.writes.is_empty() || h.connecting {
                kind |= WRITE;
            }
            if h.reading || (h.listener && h.accepting) {
                kind |= READ;
            }
            if kind != 0 {
                fds.push(h.fd);
                kinds.push(kind);
            }
        }
    });
    if signal >= 0 {
        fds.push(signal);
        kinds.push(READ);
    }
    if stdin {
        fds.push(STDIN_FD);
        kinds.push(READ);
    }
    if fds.is_empty() {
        return false;
    }
    let mut ready = vec![0u8; fds.len()];
    let timeout = budget.as_nanos().min(u64::MAX as u128) as u64;
    let result = unsafe {
        perry_wasi_poll(
            fds.as_ptr(),
            kinds.as_ptr(),
            fds.len() as u32,
            timeout,
            ready.as_mut_ptr(),
        )
    };
    if result > 0 && stdin && ready[fds.len() - 1] != 0 {
        crate::os::p1_stdin_ready();
    }
    // A failed poll slept nothing; let the caller's timed sleep bound the wait.
    result >= 0
}
