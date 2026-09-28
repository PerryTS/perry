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
#[cfg(unix)]
use std::os::unix::io::RawFd;
#[cfg(windows)]
type RawFd = std::sync::Arc<native::PtySession>;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use super::native;
use crate::child_process::{cp_set_field, cp_undefined, make_two_field_object};

/// Monotonic registry key for live ptys.
static PTY_NEXT_LIVE_ID: AtomicU64 = AtomicU64::new(1);

/// Number of live (spawned, not-yet-closed) ptys — the lock-free fast-path
/// gate for the pump and the active-handle check.
static PTY_LIVE_COUNT: AtomicU64 = AtomicU64::new(0);

/// Live PTYs which currently keep the event loop alive. Unreferenced PTYs are
/// still pumped and rooted while another handle drives the runtime.
static PTY_REFED_COUNT: AtomicU64 = AtomicU64::new(0);

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
    #[cfg(unix)]
    pid: i32,
    master: RawFd,
    #[cfg(unix)]
    write_tx: std::sync::mpsc::Sender<Vec<u8>>,
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
    /// Whether this PTY currently contributes an active event-loop handle.
    refed: bool,
    paused: bool,
    pending: Vec<Vec<u8>>,
}

static PTY_LIVE: Mutex<Option<HashMap<u64, LivePty>>> = Mutex::new(None);

thread_local! {
    /// Re-entrancy guard — an emitted handler may itself drive the event
    /// loop (`await`), which re-enters `js_run_stdlib_pump` → this pump.
    static PTY_PUMPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
/// Register [`release_ptys_in_freed_ranges`] before the first `PTY_LIVE`
/// insert (#11541). Every insert calls this first; named in
/// `arena::thread_exit`'s dispatcher instead, the release was linked into
/// every binary.
fn register_thread_exit_release() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        crate::arena::thread_exit::register_thread_exit_range_hook(release_ptys_in_freed_ranges);
    });
}

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
#[cfg(unix)]
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
#[cfg(unix)]
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

#[cfg(windows)]
fn pty_spawn_reader(handle: u64, master: RawFd) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match native::read_pty(&master, &mut buf) {
                Ok(0) => break,
                Ok(n) => pty_push_event(PtyEvent::Data {
                    handle,
                    bytes: buf[..n].to_vec(),
                }),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        pty_push_event(PtyEvent::Eof { handle });
    });
}

#[cfg(windows)]
fn pty_spawn_waiter(handle: u64, master: RawFd) {
    std::thread::spawn(move || {
        let (code, signal) = native::wait_child(master);
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
    register_thread_exit_release();
    {
        let mut guard = pty_live_lock();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            handle,
            LivePty {
                ipty_bits: ipty.to_bits(),
                #[cfg(unix)]
                pid: child.pid,
                master: child.master.clone(),
                #[cfg(unix)]
                write_tx: pty_spawn_writer(child.master),
                utf8_carry: Vec::new(),
                eof: false,
                exited: None,
                closed: false,
                refed: true,
                paused: false,
                pending: Vec::new(),
            },
        );
    }
    crate::stdlib_pump::register_runtime_pump(1, pty_reactor_pump_extern);
    PTY_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    PTY_REFED_COUNT.fetch_add(1, Ordering::SeqCst);
    pty_spawn_reader(handle, child.master.clone());
    #[cfg(unix)]
    pty_spawn_waiter(handle, child.pid);
    #[cfg(windows)]
    pty_spawn_waiter(handle, child.master);
    crate::event_pump::js_notify_main_thread();
    handle
}

/// Queue PTY input without blocking the event loop on terminal backpressure.
pub(crate) fn pty_live_write(handle: u64, bytes: &[u8]) -> bool {
    let guard = pty_live_lock();
    let Some(lp) = guard.as_ref().and_then(|m| m.get(&handle)) else {
        return false;
    };
    if lp.closed || lp.exited.is_some() {
        return false;
    }
    #[cfg(unix)]
    {
        lp.write_tx.send(bytes.to_vec()).is_ok()
    }
    #[cfg(windows)]
    {
        native::write_pty(&lp.master, bytes)
    }
}

#[cfg(unix)]
fn pty_spawn_writer(master: RawFd) -> std::sync::mpsc::Sender<Vec<u8>> {
    use std::io::Write;
    use std::os::unix::io::FromRawFd;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let fd = unsafe { libc::fcntl(master, libc::F_DUPFD_CLOEXEC, 0) };
    if fd >= 0 {
        let mut writer = unsafe { std::fs::File::from_raw_fd(fd) };
        std::thread::spawn(move || {
            for bytes in rx {
                if writer.write_all(&bytes).is_err() {
                    break;
                }
            }
        });
    }
    tx
}

/// `TIOCSWINSZ` a live pty. Returns whether the ioctl succeeded.
pub(crate) fn pty_live_resize(handle: u64, cols: u16, rows: u16) -> bool {
    let master = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if !lp.closed => lp.master.clone(),
            _ => return false,
        }
    };
    native::resize_pty(master, cols, rows)
}

/// Toggle raw mode on a live PTY.
#[cfg(unix)]
pub(crate) fn pty_live_set_raw_mode(handle: u64, enabled: bool) -> bool {
    let master = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if !lp.closed => lp.master.clone(),
            _ => return false,
        }
    };
    native::set_raw_mode(master, enabled)
}

/// Signal a live pty child. Skipped once reaped (the pid may be recycled).
#[cfg(unix)]
pub(crate) fn pty_live_kill(handle: u64, signo: i32) -> bool {
    let pid = {
        let guard = pty_live_lock();
        match guard.as_ref().and_then(|m| m.get(&handle)) {
            Some(lp) if lp.exited.is_none() => lp.pid,
            _ => return false,
        }
    };
    native::signal_pid(pid, signo)
}

#[cfg(windows)]
pub(crate) fn pty_live_kill(handle: u64, signo: i32) -> bool {
    let guard = pty_live_lock();
    match guard.as_ref().and_then(|m| m.get(&handle)) {
        Some(lp) if lp.exited.is_none() => native::signal_pty(&lp.master, signo),
        _ => false,
    }
}

pub(super) fn pty_live_set_paused(handle: u64, paused: bool) {
    if let Some(lp) = pty_live_lock().as_mut().and_then(|m| m.get_mut(&handle)) {
        lp.paused = paused;
    }
    crate::event_pump::js_notify_main_thread();
}

/// Toggle one PTY's event-loop keepalive bit. Calls are idempotent.
#[cfg(unix)]
pub(crate) fn pty_live_set_refed(handle: u64, refed: bool) -> bool {
    {
        let mut guard = pty_live_lock();
        let Some(pty) = guard.as_mut().and_then(|map| map.get_mut(&handle)) else {
            return false;
        };
        if pty.closed {
            return false;
        }
        if pty.refed == refed {
            return true;
        }
        pty.refed = refed;
    }
    if refed {
        PTY_REFED_COUNT.fetch_add(1, Ordering::SeqCst);
        crate::event_pump::js_notify_main_thread();
    } else {
        PTY_REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
    true
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

/// `extern "C"` wrapper registered into the armed runtime-pump slots on the
/// first live pty (see `stdlib_pump::register_runtime_pump`).
extern "C" fn pty_reactor_pump_extern() {
    pty_reactor_pump();
}

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
    let mut events = Vec::new();
    if let Some(map) = pty_live_lock().as_mut() {
        for (handle, lp) in map {
            if !lp.paused {
                events.extend(std::mem::take(&mut lp.pending).into_iter().map(|bytes| {
                    PtyEvent::Data {
                        handle: *handle,
                        bytes,
                    }
                }));
            }
        }
    }
    events.extend(std::mem::take(&mut *pty_queue_lock()));
    for ev in events {
        match ev {
            PtyEvent::Data { handle, bytes } => {
                let decoded = {
                    let mut guard = pty_live_lock();
                    match guard.as_mut().and_then(|m| m.get_mut(&handle)) {
                        // A callback for another PTY may resume this one
                        // after the pending-data snapshot above. Keep newer
                        // chunks behind its older, still-pending output.
                        Some(lp) if lp.paused || !lp.pending.is_empty() => {
                            lp.pending.push(bytes);
                            None
                        }
                        Some(lp) => {
                            let text = pty_decode_utf8(&mut lp.utf8_carry, &bytes);
                            Some((lp.ipty_bits, text))
                        }
                        None => None,
                    }
                };
                if let Some((ipty_bits, text)) = decoded {
                    if !text.is_empty() {
                        let scope = crate::gc::RuntimeHandleScope::new();
                        let ipty = scope.root_nanbox_f64(f64::from_bits(ipty_bits));
                        let chunk = crate::child_process::cp_box_string(&text);
                        super::pty_emit(ipty.get_nanbox_f64(), "data", &[chunk]);
                    }
                }
            }
            PtyEvent::Eof { handle } => {
                if let Some(lp) = pty_live_lock().as_mut().and_then(|m| m.get_mut(&handle)) {
                    lp.eof = true;
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
        #[cfg(unix)]
        master: RawFd,
        tail: Vec<u8>,
        code: Option<i32>,
        signal: Option<i32>,
        refed: bool,
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
                    if lp.eof && !lp.paused && lp.pending.is_empty() {
                        lp.closed = true;
                        out.push(PtyCloseItem {
                            handle: *h,
                            #[cfg(unix)]
                            master: lp.master.clone(),
                            tail: std::mem::take(&mut lp.utf8_carry),
                            code,
                            signal,
                            refed: lp.refed,
                        });
                    }
                }
            }
        }
        out
    };
    for item in to_close {
        #[cfg(unix)]
        unsafe {
            libc::close(item.master);
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        // #11471: a thread exit may have released this entry since the
        // snapshot above (`release_ptys_in_freed_ranges`), and that release
        // owns its counter decrement.
        let Some(bits) = pty_live_lock()
            .as_ref()
            .and_then(|map| map.get(&item.handle).map(|lp| lp.ipty_bits))
        else {
            continue;
        };
        let ipty = scope.root_nanbox_f64(f64::from_bits(bits));
        if !item.tail.is_empty() {
            let chunk = crate::child_process::cp_box_string(&String::from_utf8_lossy(&item.tail));
            super::pty_emit(ipty.get_nanbox_f64(), "data", &[chunk]);
        }
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
        let payload = scope.root_nanbox_f64(payload);
        // Mirror the terminal state onto the IPty object before emitting so
        // a handler reading `pty.process` state observes post-exit values.
        cp_set_field(ipty.get_nanbox_f64(), b"exitCode", exit_code);
        super::pty_emit(ipty.get_nanbox_f64(), "exit", &[payload.get_nanbox_f64()]);
        let removed = pty_live_lock()
            .as_mut()
            .and_then(|map| map.remove(&item.handle))
            .is_some();
        if removed {
            PTY_LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
            if item.refed {
                PTY_REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
        }
    }
}

// ============================================================================
// Event-loop integration hooks (wired from lib.rs / gc/mod.rs).
// ============================================================================

/// Whether any live pty is keeping the event loop alive — OR'd into
/// `js_stdlib_has_active_handles`.
pub(crate) fn pty_reactor_has_live() -> bool {
    PTY_REFED_COUNT.load(Ordering::Relaxed) > 0
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

/// #11471: drop every live pty registered by a thread whose arena is being
/// freed.
///
/// `ipty_bits` is the spawning thread's IPty object, GC-rooted by
/// [`pty_reactor_scan_roots_mut`] and emitted on by the pump. Left behind by an
/// exited thread, later collections would mark/rewrite memory another arena
/// may have reused, the pump would fire `onData`/`onExit` on it, and
/// [`PTY_REFED_COUNT`] would keep the event loop alive forever.
///
/// Runs in the exiting thread's TLS destructor (see `arena::thread_exit`): no
/// thread-locals, no JS. The child is not signalled. The master descriptor is
/// deliberately LEAKED, not closed: the reader thread may still be blocked in
/// `read(master)`, and closing it under that thread would let the descriptor
/// number be reused while the read is in flight — the same race the pump
/// avoids by closing only after `Eof`. The reader/waiter threads finish on the
/// child's exit and their events for this handle are skipped by the pump.
/// Dropping the entry drops the writer channel, which ends the writer thread.
pub(crate) fn release_ptys_in_freed_ranges(freed: &crate::arena::thread_exit::FreedRanges) {
    if PTY_LIVE_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }
    let removed: Vec<LivePty> = {
        let mut guard = pty_live_lock();
        let Some(map) = guard.as_mut() else {
            return;
        };
        let dead: Vec<u64> = map
            .iter()
            .filter(|(_, lp)| freed.holds_bits(lp.ipty_bits))
            .map(|(handle, _)| *handle)
            .collect();
        dead.iter()
            .filter_map(|handle| map.remove(handle))
            .collect()
    };
    for lp in &removed {
        PTY_LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
        if lp.refed {
            PTY_REFED_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }
    drop(removed);
}

/// Test probe (#11471): register a live-pty entry for `ipty` as
/// `pty_register_live` would, minus the child: no descriptor (`-1`), no
/// reader/waiter threads, un-`ref`'d and never exiting, so no pump acts on it
/// and no event loop is held open by it. Returns the registry handle.
#[cfg(unix)]
#[doc(hidden)]
pub fn pty_register_idle_live_for_test(ipty: f64) -> u64 {
    let handle = PTY_NEXT_LIVE_ID.fetch_add(1, Ordering::SeqCst);
    register_thread_exit_release();
    pty_live_lock().get_or_insert_with(HashMap::new).insert(
        handle,
        LivePty {
            ipty_bits: ipty.to_bits(),
            pid: 0,
            master: -1,
            write_tx: std::sync::mpsc::channel().0,
            utf8_carry: Vec::new(),
            eof: false,
            exited: None,
            closed: false,
            refed: false,
            paused: false,
            pending: Vec::new(),
        },
    );
    PTY_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    handle
}

/// Test probe (#11471): is pty `handle` still registered?
#[doc(hidden)]
pub fn pty_live_registered_for_test(handle: u64) -> bool {
    pty_live_lock()
        .as_ref()
        .is_some_and(|map| map.contains_key(&handle))
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
