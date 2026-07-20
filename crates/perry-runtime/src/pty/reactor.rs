//! Event reactor for the node-pty surface (#6563) — the pty sibling of
//! `child_process::reactor`.
//!
//! Per live pty, two background threads move raw bytes / exit status across
//! the thread boundary (JSValues are thread-local and never leave the main
//! thread):
//!   * a reader that blocks on `read(master)` and pushes [`PtyEvent::Data`]
//!     chunks until EOF (macOS) / EIO (Linux, after the child exits), then
//!     pushes [`PtyEvent::Eof`];
//!   * a waiter that blocks in `waitpid` and pushes [`PtyEvent::Exited`].
//! Both call [`js_notify_main_thread`] so the event loop wakes immediately.
//!
//! The main-thread [`pty_reactor_pump`] (driven from `js_run_stdlib_pump`)
//! drains the queue, decodes chunks to JS strings (with UTF-8 carry-over for
//! sequences split across reads — node-pty delivers *strings*, not Buffers),
//! and fires the `onData` listeners. Once a pty has BOTH exited and hit EOF
//! (all buffered output already delivered), the pump fires `onExit` with
//! node-pty's `{ exitCode, signal }` shape, closes the master fd, and drops
//! the registry entry. Live ptys keep the event loop alive via
//! [`pty_reactor_has_live`] and are GC roots via [`pty_reactor_scan_roots_mut`].

use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use super::native;
use crate::child_process::{cp_set_field, cp_undefined, make_two_field_object};

/// Monotonic registry key for live ptys.
static PTY_NEXT_LIVE_ID: AtomicU64 = AtomicU64::new(1);

/// Number of live (spawned, not-yet-closed) ptys — the lock-free fast-path
/// gate for the pump and the active-handle check.
static PTY_LIVE_COUNT: AtomicU64 = AtomicU64::new(0);

/// An event produced by a pty's background threads, consumed by the pump.
enum PtyEvent {
    /// One master-side read chunk.
    Data { handle: u64, bytes: Vec<u8> },
    /// The reader thread finished (EOF / EIO after child exit).
    Eof { handle: u64 },
    /// The child was reaped (`code` xor `signal`).
    Exited {
        handle: u64,
        code: Option<i32>,
        signal: Option<i32>,
    },
}

static PTY_EVENT_QUEUE: Mutex<Vec<PtyEvent>> = Mutex::new(Vec::new());

/// Per-pty reactor state owned by the main thread.
struct LivePty {
    /// NaN-boxed IPty object — a GC root (see `pty_reactor_scan_roots_mut`).
    ipty_bits: u64,
    pid: i32,
    master: RawFd,
    /// Bytes of an incomplete trailing UTF-8 sequence from the previous
    /// chunk, prepended to the next one so multi-byte characters split
    /// across `read` boundaries decode intact.
    utf8_carry: Vec<u8>,
    /// The reader thread saw EOF (no more output will arrive).
    eof: bool,
    /// `Some((code, signal))` once the waiter reported termination.
    exited: Option<(Option<i32>, Option<i32>)>,
    /// Whether `onExit` has been fired (terminal state).
    closed: bool,
}

static PTY_LIVE: Mutex<Option<HashMap<u64, LivePty>>> = Mutex::new(None);

thread_local! {
    /// Re-entrancy guard — an emitted handler may itself drive the event
    /// loop (`await`), which re-enters `js_run_stdlib_pump` → this pump.
    static PTY_PUMPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn pty_live_lock() -> std::sync::MutexGuard<'static, Option<HashMap<u64, LivePty>>> {
    PTY_LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[inline]
fn pty_queue_lock() -> std::sync::MutexGuard<'static, Vec<PtyEvent>> {
    PTY_EVENT_QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn pty_push_event(ev: PtyEvent) {
    pty_queue_lock().push(ev);
    crate::event_pump::js_notify_main_thread();
}

/// Spawn the blocking reader thread for `master`. The fd stays owned by the
/// registry entry (closed by the pump after EOF+exit), so the raw `read` here
/// never races a close: the pump only closes once `Eof` has been consumed,
/// i.e. after this thread has already returned.
fn pty_spawn_reader(handle: u64, master: RawFd) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            let n = unsafe { libc::read(master, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n > 0 {
                pty_push_event(PtyEvent::Data {
                    handle,
                    bytes: buf[..n as usize].to_vec(),
                });
                continue;
            }
            if n < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            // 0 = EOF (macOS), <0 = EIO (Linux, child gone). Either way the
            // pty has no more output.
            pty_push_event(PtyEvent::Eof { handle });
            break;
        }
    });
}

/// Spawn the waiter thread that reaps `pid` and reports its exit status.
fn pty_spawn_waiter(handle: u64, pid: i32) {
    std::thread::spawn(move || {
        let (code, signal) = native::wait_child(pid);
        pty_push_event(PtyEvent::Exited {
            handle,
            code,
            signal,
        });
    });
}

/// Register a freshly-spawned pty child: insert the registry entry, start the
/// reader + waiter threads and wake the loop. Returns the registry handle.
pub(super) fn pty_register_live(ipty: f64, child: native::PtyChild) -> u64 {
    let handle = PTY_NEXT_LIVE_ID.fetch_add(1, Ordering::SeqCst);
    {
        let mut guard = pty_live_lock();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            handle,
            LivePty {
                ipty_bits: ipty.to_bits(),
                pid: child.pid,
                master: child.master,
                utf8_carry: Vec::new(),
                eof: false,
                exited: None,
                closed: false,
            },
        );
    }
    PTY_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    pty_spawn_reader(handle, child.master);
    pty_spawn_waiter(handle, child.pid);
    crate::event_pump::js_notify_main_thread();
    handle
}

/// Write `bytes` to a live pty's master. Returns whether the write succeeded.
pub(super) fn pty_live_write(handle: u64, bytes: &[u8]) -> bool {
    let master = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if !lp.closed => lp.master,
            _ => return false,
        }
    };
    // Plain blocking write outside the lock (a full pty output buffer must
    // not wedge the registry). Shells drain fast; matching node-pty's
    // synchronous unix write path.
    let mut off = 0;
    while off < bytes.len() {
        let n = unsafe {
            libc::write(
                master,
                bytes[off..].as_ptr() as *const libc::c_void,
                bytes.len() - off,
            )
        };
        if n < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return false;
        }
        off += n as usize;
    }
    true
}

/// `TIOCSWINSZ` a live pty. Returns whether the ioctl succeeded.
pub(super) fn pty_live_resize(handle: u64, cols: u16, rows: u16) -> bool {
    let master = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if !lp.closed => lp.master,
            _ => return false,
        }
    };
    native::resize_pty(master, cols, rows)
}

/// Signal a live pty child. Skipped once reaped (the pid may be recycled).
pub(super) fn pty_live_kill(handle: u64, signo: i32) -> bool {
    let pid = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if lp.exited.is_none() => lp.pid,
            _ => return false,
        }
    };
    native::signal_pid(pid, signo)
}

/// Decode `bytes` (with the pty's carry-over prefix) into a String, saving an
/// incomplete trailing UTF-8 sequence back into `carry` for the next chunk.
/// Interior invalid bytes are replaced with U+FFFD.
fn pty_decode_utf8(carry: &mut Vec<u8>, bytes: &[u8]) -> String {
    let mut data = std::mem::take(carry);
    data.extend_from_slice(bytes);
    let mut out = String::with_capacity(data.len());
    let mut rest: &[u8] = &data;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(unsafe { std::str::from_utf8_unchecked(&rest[..valid]) });
                match e.error_len() {
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        rest = &rest[valid + bad..];
                    }
                    None => {
                        // Incomplete trailing sequence — hold it for the
                        // next chunk.
                        *carry = rest[valid..].to_vec();
                        break;
                    }
                }
            }
        }
    }
    out
}

// ============================================================================
// Main-thread pump
// ============================================================================

/// Drive the reactor one tick: deliver pending `data` and terminal `exit`
/// for all live ptys. Called from `js_run_stdlib_pump`.
pub(crate) fn pty_reactor_pump() {
    if PTY_LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if PTY_PUMPING.with(|p| p.replace(true)) {
        return; // re-entrant await inside a handler
    }
    pty_reactor_pump_inner();
    PTY_PUMPING.with(|p| p.set(false));
}

fn pty_reactor_pump_inner() {
    // --- Phase A: drain queued data/eof/exited events. Snapshot state under
    // a brief lock, emit OUTSIDE it (handlers allocate / can trigger GC, and
    // the GC root scanner takes the same lock on this thread). ---
    let events = std::mem::take(&mut *pty_queue_lock());
    for ev in events {
        match ev {
            PtyEvent::Data { handle, bytes } => {
                let decoded = {
                    let mut guard = pty_live_lock();
                    match guard.as_mut().and_then(|m| m.get_mut(&handle)) {
                        Some(lp) => {
                            let text = pty_decode_utf8(&mut lp.utf8_carry, &bytes);
                            Some((lp.ipty_bits, text))
                        }
                        None => None,
                    }
                };
                if let Some((ipty_bits, text)) = decoded {
                    if !text.is_empty() {
                        let ipty = f64::from_bits(ipty_bits);
                        super::pty_emit(
                            ipty,
                            "data",
                            &[crate::child_process::cp_box_string(&text)],
                        );
                    }
                }
            }
            PtyEvent::Eof { handle } => {
                let flush = {
                    let mut guard = pty_live_lock();
                    match guard.as_mut().and_then(|m| m.get_mut(&handle)) {
                        Some(lp) => {
                            lp.eof = true;
                            // Whatever is still in the carry can never
                            // complete — flush it lossily.
                            let tail = std::mem::take(&mut lp.utf8_carry);
                            Some((lp.ipty_bits, tail))
                        }
                        None => None,
                    }
                };
                if let Some((ipty_bits, tail)) = flush {
                    if !tail.is_empty() {
                        let text = String::from_utf8_lossy(&tail).into_owned();
                        let ipty = f64::from_bits(ipty_bits);
                        super::pty_emit(
                            ipty,
                            "data",
                            &[crate::child_process::cp_box_string(&text)],
                        );
                    }
                }
            }
            PtyEvent::Exited {
                handle,
                code,
                signal,
            } => {
                if let Some(map) = pty_live_lock().as_mut() {
                    if let Some(lp) = map.get_mut(&handle) {
                        lp.exited = Some((code, signal));
                    }
                }
            }
        }
    }

    // --- Phase B: fire `onExit` once a pty has exited AND its reader hit
    // EOF, so every `data` chunk has already been delivered. ---
    struct PtyCloseItem {
        handle: u64,
        ipty_bits: u64,
        master: RawFd,
        code: Option<i32>,
        signal: Option<i32>,
    }
    let to_close: Vec<PtyCloseItem> = {
        let mut guard = pty_live_lock();
        let mut out = Vec::new();
        if let Some(map) = guard.as_mut() {
            for (h, lp) in map.iter_mut() {
                if lp.closed {
                    continue;
                }
                if let Some((code, signal)) = lp.exited {
                    if lp.eof {
                        lp.closed = true;
                        out.push(PtyCloseItem {
                            handle: *h,
                            ipty_bits: lp.ipty_bits,
                            master: lp.master,
                            code,
                            signal,
                        });
                    }
                }
            }
        }
        out
    };
    for item in to_close {
        unsafe {
            libc::close(item.master);
        }
        let ipty = f64::from_bits(item.ipty_bits);
        // node-pty's exit payload: `{ exitCode: number, signal?: number }` —
        // signal is the numeric signo for a signal death, undefined otherwise.
        let exit_code = item.code.unwrap_or(0) as f64;
        let signal_val = match item.signal {
            Some(s) => s as f64,
            None => cp_undefined(),
        };
        let payload = unsafe {
            crate::value::js_nanbox_pointer(make_two_field_object(
                "exitCode", exit_code, "signal", signal_val,
            ) as i64)
        };
        // Mirror the terminal state onto the IPty object before emitting so
        // a handler reading `pty.process` state observes post-exit values.
        cp_set_field(ipty, b"exitCode", exit_code);
        super::pty_emit(ipty, "exit", &[payload]);
        if let Some(map) = pty_live_lock().as_mut() {
            map.remove(&item.handle);
        }
        PTY_LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

// ============================================================================
// Event-loop integration hooks (wired from lib.rs / gc/mod.rs).
// ============================================================================

/// Whether any live pty is keeping the event loop alive — OR'd into
/// `js_stdlib_has_active_handles`.
pub(crate) fn pty_reactor_has_live() -> bool {
    PTY_LIVE_COUNT.load(Ordering::Relaxed) > 0
}

/// GC mutable-root scanner: keep every live IPty (and, through its fields,
/// the registered listener arrays + closures) alive across collections, and
/// rewrite the stored pointer on evacuation.
pub(crate) fn pty_reactor_scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if PTY_LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(map) = pty_live_lock().as_mut() {
        for lp in map.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut lp.ipty_bits);
        }
    }
}

#[cfg(test)]
pub(crate) fn pty_live_count_for_test() -> u64 {
    PTY_LIVE_COUNT.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_carry_reassembles_split_sequences() {
        let mut carry = Vec::new();
        // "é" (0xC3 0xA9) split across two chunks.
        let first = pty_decode_utf8(&mut carry, &[b'a', 0xC3]);
        assert_eq!(first, "a");
        assert_eq!(carry, vec![0xC3]);
        let second = pty_decode_utf8(&mut carry, &[0xA9, b'b']);
        assert_eq!(second, "éb");
        assert!(carry.is_empty());
    }

    #[test]
    fn utf8_interior_garbage_is_replaced() {
        let mut carry = Vec::new();
        let out = pty_decode_utf8(&mut carry, &[b'x', 0xFF, b'y']);
        assert_eq!(out, "x\u{FFFD}y");
        assert!(carry.is_empty());
    }
}
