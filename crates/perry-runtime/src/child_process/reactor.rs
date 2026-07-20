//! Async subprocess reactor for `child_process.spawn` — #1934.
//!
//! The buffered `spawn` (pre-#1934) ran the child to completion synchronously
//! and replayed a single `data` chunk + `exit`/`close` on the next tick. That
//! is observationally correct for short commands but precludes a *live* child:
//! `stdin.write()` had nothing to write to, `kill()` had nothing to signal, and
//! stdout arrived in one post-exit lump.
//!
//! This reactor launches the child without blocking and wires three background
//! threads per child:
//!   * a stdout reader and a stderr reader that read pipe chunks and push them
//!     onto [`CP_EVENT_QUEUE`], and
//!   * a waiter that blocks on `Child::wait()` and pushes the exit status.
//! Each producer calls [`js_notify_main_thread`] so the event loop wakes
//! immediately. The main-thread [`cp_reactor_pump`] (driven from
//! `js_run_stdlib_pump`, which both the end-of-main event loop and the `await`
//! poll loop call) drains the queue, builds `Buffer`s, and emits `spawn` /
//! `data` / `end` / `exit` / `close` on the ChildProcess. Live children keep
//! the event loop alive via [`cp_reactor_has_live`] (wired into
//! `js_stdlib_has_active_handles`).
//!
//! Background threads never touch JSValues (those are thread-local); they move
//! only raw bytes / exit codes across the boundary. The ChildProcess object is
//! kept reachable across ticks by [`cp_reactor_scan_roots_mut`], a registered
//! GC mutable-root scanner.

use std::collections::HashMap;
#[cfg(unix)]
use std::io::BufRead;
use std::io::{Read, Write};
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

// Brings in the `cp_*` helpers, `CpFn`, `CP_SHAPE_ID`, the `TAG_*` consts, and
// (since this is a descendant module) `mod.rs`'s `use` bindings such as
// `ClosureHeader` / `JSValue`.
use super::*;

#[cfg(unix)]
type IpcStream = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type IpcStream = ();

/// Monotonic registry key for live children.
static CP_NEXT_LIVE_ID: AtomicU64 = AtomicU64::new(1);

/// Number of live (spawned, not-yet-`close`d) children. A lock-free fast-path
/// gate: the pump and the active-handle check both bail on zero without taking
/// any lock, so the hot async loop pays a single relaxed load per tick.
static CP_LIVE_COUNT: AtomicU64 = AtomicU64::new(0);

/// An event produced by a child's background thread, consumed by the pump.
enum CpEvent {
    /// A stdout (`stderr == false`) or stderr chunk.
    Data {
        handle: u64,
        stderr: bool,
        bytes: Vec<u8>,
    },
    /// End-of-file on a stream — the reader thread finished.
    Eof { handle: u64, stderr: bool },
    /// The child process terminated (`code` xor `signal`).
    Exited {
        handle: u64,
        code: Option<i32>,
        signal: Option<i32>,
    },
    /// #1933: one IPC line (`process.send` on the child side) — raw JSON to be
    /// parsed + delivered as a `'message'` event on the main thread.
    Message { handle: u64, json: String },
    /// #2130: one IPC frame under `serialization: 'advanced'` — the raw V8
    /// structured-clone payload (header included, length prefix stripped) to be
    /// deserialized + delivered as a `'message'` event on the main thread.
    MessageAdvanced { handle: u64, bytes: Vec<u8> },
    /// #1933: the IPC channel closed (child disconnected or exited) — flip
    /// `connected`/`channel` and emit `'disconnect'`.
    IpcClosed { handle: u64 },
    /// Timeout expired; terminate the live child with this signal on the main
    /// thread so JS-visible `killed` is updated with the same event ordering.
    Timeout { handle: u64, signal: i32 },
    /// `AbortSignal` attached through `options.signal` fired.
    Abort { handle: u64 },
}

static CP_EVENT_QUEUE: Mutex<Vec<CpEvent>> = Mutex::new(Vec::new());

/// Per-child reactor state. The `Child` itself is owned by the waiter thread
/// (so reaping blocks off the main thread); here we keep only what the main
/// thread needs: the GC-rooted ChildProcess value, the live `pid` for
/// `kill()`, the writable `stdin`, and progress flags.
struct LiveChild {
    /// NaN-boxed ChildProcess object — a GC root (see `cp_reactor_scan_roots_mut`).
    cp_bits: u64,
    pid: i32,
    stdin: Option<ChildStdin>,
    stdout_open: bool,
    stderr_open: bool,
    /// Whether the `spawn` event has been emitted yet.
    spawned: bool,
    /// `Some((code, signal))` once the waiter reported termination.
    exited: Option<(Option<i32>, Option<i32>)>,
    /// Whether `exit`/`close` have been emitted (terminal state).
    closed: bool,
    /// #1933: the parent end of the IPC socket for a `fork()`ed child (a clone
    /// for `child.send()` / `child.disconnect()`; the reader thread owns
    /// another clone). `None` for plain `spawn`.
    ipc_send: Option<IpcStream>,
    /// #2130: whether the IPC channel uses V8 structured-clone framing
    /// (`serialization: 'advanced'`) rather than newline-delimited JSON.
    ipc_advanced: bool,
    /// NaN-boxed AbortSignal value for listener cleanup/GC rooting.
    abort_signal_bits: u64,
    /// NaN-boxed abort-listener closure value for listener cleanup/GC rooting.
    abort_listener_bits: u64,
    /// Signal number used when `options.signal` aborts this child.
    abort_kill_signal: i32,
    /// Windows: the signal number of a successful `kill()` / timeout / abort
    /// termination. On Windows the child dies via `TerminateProcess(h, 1)`, so
    /// the waiter's `Child::wait()` reports the synthetic exit code 1 and no
    /// signal; Node (via libuv's `uv_process_kill`) instead reports
    /// `code: null, signal: <requested>`. Recording the requested signal here
    /// lets the pump substitute the Node shape when the `Exited` event
    /// arrives, keeping the JS-visible exit identical to the unix arm's.
    #[cfg(windows)]
    win_kill_signal: Option<i32>,
    /// Windows: a process handle duplicated at spawn time
    /// (`cp_win_dup_proc_handle`), stored as a raw `isize` so the registry
    /// stays `Send`. The kill paths act on THIS handle, never the pid: the
    /// waiter thread's `Child::wait()` reaps the process (freeing the pid for
    /// OS reuse) *before* its `Exited` event reaches the pump, so a `kill()`
    /// racing into that window could otherwise `OpenProcess` a recycled pid
    /// and terminate an innocent process. A held handle keeps naming the
    /// original process object forever — the same strategy as libuv's
    /// `uv_process_kill`. `0` if duplication failed. Closed by `Drop`.
    #[cfg(windows)]
    win_proc_handle: isize,
    /// #4912: present for children launched by the async `exec`/`execFile`
    /// callback form. When set, the pump buffers stdout/stderr instead of
    /// emitting stream events and fires this single `(err, stdout, stderr)`
    /// callback on `close` — Node's "run off-thread, call back on a later
    /// tick" model. `None` for `spawn`/`fork`.
    exec: Option<Box<CpExecPending>>,
}

#[cfg(windows)]
impl Drop for LiveChild {
    /// Close the process handle duplicated at spawn. The registry entry is
    /// the single owner: Phase B's `map.remove` drops the entry once the
    /// child has fully closed (exit reported + streams at EOF), and process
    /// teardown drops whatever remains.
    fn drop(&mut self) {
        if self.win_proc_handle != 0 {
            unsafe {
                let _ = windows_sys::Win32::Foundation::CloseHandle(
                    self.win_proc_handle as windows_sys::Win32::Foundation::HANDLE,
                );
            }
        }
    }
}

/// Buffered state for an async `exec`/`execFile` child (#4912). Output is
/// accumulated off the main thread and replayed into the same `CpRun`-shaped
/// callback the synchronous path used, so the deferred callback is
/// byte-identical to the former immediate one.
pub(super) struct CpExecPending {
    /// NaN-boxed callback closure — a GC root until the callback fires.
    cb_bits: u64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    /// `maxBuffer` / `timeout` / `killSignal` limits + the error shape source.
    run_options: CpRunOptions,
    /// utf8/buffer encoding for the boxed stdout/stderr.
    mode: CpOutput,
    /// Human-readable command, for the error `.cmd` field.
    cmd: String,
    /// Program actually launched, for the spawn-failure `syscall`/`path`.
    file: String,
    /// Set once accumulated output passed `maxBuffer` (the child is then killed).
    exceeded: bool,
    /// Set when the `timeout` fired and the child was killed.
    timed_out: bool,
}

static CP_LIVE: Mutex<Option<HashMap<u64, LiveChild>>> = Mutex::new(None);

thread_local! {
    /// Re-entrancy guard — an emitted handler may itself drive the event loop
    /// (`await`), which re-enters `js_run_stdlib_pump` → `cp_reactor_pump`.
    static CP_PUMPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline]
fn cp_live_lock() -> std::sync::MutexGuard<'static, Option<HashMap<u64, LiveChild>>> {
    CP_LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

#[inline]
fn cp_queue_lock() -> std::sync::MutexGuard<'static, Vec<CpEvent>> {
    CP_EVENT_QUEUE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn cp_push_event(ev: CpEvent) {
    cp_queue_lock().push(ev);
    crate::event_pump::js_notify_main_thread();
}

#[inline]
fn libc_sigterm() -> i32 {
    #[cfg(unix)]
    {
        libc::SIGTERM
    }
    #[cfg(not(unix))]
    {
        15
    }
}

/// Spawn a reader thread that streams `pipe` to the event queue until EOF.
fn cp_spawn_reader<R: Read + Send + 'static>(handle: u64, mut pipe: R, stderr: bool) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match pipe.read(&mut buf) {
                Ok(0) | Err(_) => {
                    cp_push_event(CpEvent::Eof { handle, stderr });
                    break;
                }
                Ok(n) => {
                    cp_push_event(CpEvent::Data {
                        handle,
                        stderr,
                        bytes: buf[..n].to_vec(),
                    });
                }
            }
        }
    });
}

/// Spawn the waiter thread that reaps `child` and reports its exit status.
fn cp_spawn_waiter(handle: u64, mut child: Child) {
    std::thread::spawn(move || {
        let (code, signal) = match child.wait() {
            Ok(status) => {
                #[cfg(unix)]
                let signal = {
                    use std::os::unix::process::ExitStatusExt;
                    status.signal()
                };
                #[cfg(not(unix))]
                let signal: Option<i32> = None;
                (status.code(), signal)
            }
            Err(_) => (Some(-1), None),
        };
        cp_push_event(CpEvent::Exited {
            handle,
            code,
            signal,
        });
    });
}

fn cp_spawn_timeout(handle: u64, timeout: Duration, signal: i32) {
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        cp_push_event(CpEvent::Timeout { handle, signal });
    });
}

/// IPC reader (#1933): read newline-delimited JSON from the parent socket and
/// push each line for main-thread parse + `'message'` delivery. For
/// `serialization: 'advanced'` (#2130) the framing is instead a 4-byte
/// big-endian length prefix followed by that many V8-serialized payload bytes
/// (Node's `parseChannelMessages` shape).
#[cfg(unix)]
fn cp_spawn_ipc_reader(handle: u64, sock: IpcStream, advanced: bool) {
    if advanced {
        cp_spawn_ipc_reader_advanced(handle, sock);
        return;
    }
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(sock);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => cp_push_event(CpEvent::Message { handle, json: l }),
                Ok(_) => {} // blank keep-alive line
                Err(_) => break,
            }
        }
        // Channel closed (child disconnected / exited).
        cp_push_event(CpEvent::IpcClosed { handle });
    });
}

/// Advanced (#2130) IPC reader: accumulate bytes and emit one
/// [`CpEvent::MessageAdvanced`] per `[u32 BE length][payload]` frame. Robust to
/// a length field or payload split across socket reads (the
/// `advanced-serialization-splitted-length-field` case).
#[cfg(unix)]
fn cp_spawn_ipc_reader_advanced(handle: u64, mut sock: IpcStream) {
    std::thread::spawn(move || {
        let mut acc: Vec<u8> = Vec::with_capacity(8192);
        let mut chunk = [0u8; 8192];
        loop {
            let n = match sock.read(&mut chunk) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => break,
            };
            acc.extend_from_slice(&chunk[..n]);
            // Drain every complete frame currently buffered.
            let mut consumed = 0;
            while acc.len() - consumed >= 4 {
                let len = u32::from_be_bytes([
                    acc[consumed],
                    acc[consumed + 1],
                    acc[consumed + 2],
                    acc[consumed + 3],
                ]) as usize;
                if acc.len() - consumed - 4 < len {
                    break; // payload not fully arrived yet
                }
                let start = consumed + 4;
                let bytes = acc[start..start + len].to_vec();
                cp_push_event(CpEvent::MessageAdvanced { handle, bytes });
                consumed = start + len;
            }
            if consumed > 0 {
                acc.drain(..consumed);
            }
        }
        cp_push_event(CpEvent::IpcClosed { handle });
    });
}

/// Register a freshly-spawned child: take its stdio pipes, insert the registry
/// entry (with the optional IPC socket for `fork`), start the reader/waiter (+
/// IPC reader) threads, and set `pid`/`__cpHandle` on the ChildProcess + stdio
/// sub-objects. Shared by `spawn` (#1934) and `fork` (#1933). Returns the
/// registry handle.
pub(super) fn cp_register_live_child(
    cp: f64,
    stdout_obj: f64,
    stderr_obj: f64,
    stdin_obj: f64,
    mut child: Child,
    ipc: Option<IpcStream>,
    ipc_advanced: bool,
    timeout: Option<Duration>,
    kill_signal: i32,
) -> u64 {
    let pid = child.id();
    cp_set_field(cp, b"pid", pid as f64);
    // Duplicate the process handle BEFORE `child` moves to the waiter thread —
    // the kill paths act on it instead of the recyclable pid.
    #[cfg(windows)]
    let win_proc_handle = cp_win_dup_proc_handle(&child);

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let stdin_pipe = child.stdin.take();
    let stdout_open = stdout_pipe.is_some();
    let stderr_open = stderr_pipe.is_some();

    let handle = CP_NEXT_LIVE_ID.fetch_add(1, Ordering::SeqCst);
    let handle_f = handle as f64;
    cp_set_field(cp, b"__cpHandle", handle_f);
    cp_set_field(stdin_obj, b"__cpHandle", handle_f);
    cp_set_field(stdout_obj, b"__cpHandle", handle_f);
    cp_set_field(stderr_obj, b"__cpHandle", handle_f);

    // For fork, keep a clone of the IPC socket for send/disconnect; the reader
    // thread owns the original.
    #[cfg(unix)]
    let ipc_send = ipc.as_ref().and_then(|s| s.try_clone().ok());
    #[cfg(not(unix))]
    let ipc_send = {
        let _ = &ipc;
        None
    };

    {
        let mut guard = cp_live_lock();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(
            handle,
            LiveChild {
                cp_bits: cp.to_bits(),
                pid: pid as i32,
                stdin: stdin_pipe,
                stdout_open,
                stderr_open,
                spawned: false,
                exited: None,
                closed: false,
                ipc_send,
                ipc_advanced,
                abort_signal_bits: 0,
                abort_listener_bits: 0,
                abort_kill_signal: libc_sigterm(),
                #[cfg(windows)]
                win_kill_signal: None,
                #[cfg(windows)]
                win_proc_handle,
                exec: None,
            },
        );
    }
    CP_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);

    if let Some(o) = stdout_pipe {
        cp_spawn_reader(handle, o, false);
    }
    if let Some(e) = stderr_pipe {
        cp_spawn_reader(handle, e, true);
    }
    cp_spawn_waiter(handle, child);
    if let Some(timeout) = timeout {
        cp_spawn_timeout(handle, timeout, kill_signal);
    }
    #[cfg(unix)]
    {
        if let Some(sock) = ipc {
            cp_spawn_ipc_reader(handle, sock, ipc_advanced);
        }
    }
    // Wake the loop so 'spawn' fires on the next tick.
    crate::event_pump::js_notify_main_thread();
    handle
}

extern "C" fn cp_abort_listener(closure: *const ClosureHeader) -> f64 {
    let handle = js_closure_get_capture_ptr(closure, 0) as u64;
    cp_push_event(CpEvent::Abort { handle });
    cp_undefined()
}

pub(super) fn cp_install_abort_signal(handle: u64, signal: Option<f64>, opts_val: f64) {
    let Some(signal) = signal else {
        return;
    };
    let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return;
    };

    let listener = js_closure_alloc(cp_abort_listener as *const u8, 1);
    js_closure_set_capture_ptr(listener, 0, handle as i64);
    let listener_val = cp_box_ptr(listener as *const u8);
    let kill_signal = cp_read_abort_kill_signal(opts_val);

    if let Some(map) = cp_live_lock().as_mut() {
        if let Some(lc) = map.get_mut(&handle) {
            lc.abort_signal_bits = signal.to_bits();
            lc.abort_listener_bits = listener_val.to_bits();
            lc.abort_kill_signal = kill_signal;
        }
    }

    if crate::url::js_abort_signal_is_aborted(signal_ptr) != 0 {
        cp_push_event(CpEvent::Abort { handle });
    } else {
        crate::url::js_abort_signal_add_listener(signal_ptr, cp_box_string("abort"), listener_val);
    }
}

fn cp_read_abort_kill_signal(opts_val: f64) -> i32 {
    if cp_object_ptr(opts_val).is_none() {
        return libc_sigterm();
    }
    let signal = cp_get_field(opts_val, b"killSignal");
    if JSValue::from_bits(signal.to_bits()).is_undefined() {
        return libc_sigterm();
    }
    cp_signal_number_from_value(signal)
}

fn cp_signal_number_from_value(signal: f64) -> i32 {
    #[cfg(unix)]
    {
        cp_parse_signal(signal)
    }
    #[cfg(not(unix))]
    {
        // Windows: parse names/numbers with the shared cross-platform mapper
        // so `killSignal: 'SIGKILL'` round-trips into the exit event's
        // `signal` field. The number is only a reporting token — every
        // terminating signal degrades to `TerminateProcess` at the kill site.
        cp_signal_from_value(signal)
    }
}

fn cp_take_abort_fields(handle: u64) -> Option<(u64, i32, u64, u64)> {
    let mut guard = cp_live_lock();
    let lc = guard.as_mut()?.get_mut(&handle)?;
    let fields = (
        lc.cp_bits,
        lc.abort_kill_signal,
        lc.abort_signal_bits,
        lc.abort_listener_bits,
    );
    lc.abort_signal_bits = 0;
    lc.abort_listener_bits = 0;
    Some(fields)
}

fn cp_cleanup_abort_listener(signal_bits: u64, listener_bits: u64) {
    if signal_bits == 0 || listener_bits == 0 {
        return;
    }
    let signal = f64::from_bits(signal_bits);
    let Some(signal_ptr) = crate::url::abort::abort_signal_ptr_from_value(signal) else {
        return;
    };
    crate::url::js_abort_signal_remove_listener(
        signal_ptr,
        cp_box_string("abort"),
        f64::from_bits(listener_bits),
    );
}

/// `child.send(message)` — serialize `message` to JSON (main thread) and write
/// it newline-delimited to the IPC socket. Returns whether the write
/// succeeded. #1933.
pub(super) fn cp_ipc_send(handle: u64, message: f64) -> bool {
    #[cfg(not(unix))]
    {
        let _ = (handle, message);
        return false;
    }

    #[cfg(unix)]
    {
        // Determine the channel's framing without holding the lock across the
        // (potentially GC-triggering) serialization below.
        let advanced = {
            let guard = cp_live_lock();
            guard
                .as_ref()
                .and_then(|map| map.get(&handle))
                .map(|lc| lc.ipc_advanced)
                .unwrap_or(false)
        };

        let frame: Vec<u8> = if advanced {
            // #2130: 4-byte big-endian length prefix + V8 structured-clone payload.
            let payload = super::v8_serde::v8_serialize(message);
            let mut f = Vec::with_capacity(payload.len() + 4);
            f.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            f.extend_from_slice(&payload);
            f
        } else {
            let sh = unsafe { crate::json::js_json_stringify(message, 0) };
            if sh.is_null() {
                return false;
            }
            let mut line = unsafe {
                let len = (*sh).byte_len as usize;
                let data = (sh as *const u8).add(std::mem::size_of::<StringHeader>());
                std::slice::from_raw_parts(data, len).to_vec()
            };
            line.push(b'\n');
            line
        };

        let mut guard = cp_live_lock();
        if let Some(map) = guard.as_mut() {
            if let Some(lc) = map.get_mut(&handle) {
                if let Some(sock) = lc.ipc_send.as_mut() {
                    return sock.write_all(&frame).is_ok();
                }
            }
        }
        false
    }
}

/// Write an already-serialized JSON line on a child's IPC channel (newline
/// appended). Used by the cluster primary's `queryServerReply` (#4962); shares
/// the live-child lock so it never interleaves with `cp_ipc_send`.
pub fn cp_ipc_send_raw_json(handle: u64, json: &str) -> bool {
    #[cfg(not(unix))]
    {
        let _ = (handle, json);
        return false;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        let mut frame = Vec::with_capacity(json.len() + 1);
        frame.extend_from_slice(json.as_bytes());
        frame.push(b'\n');
        let mut guard = cp_live_lock();
        if let Some(map) = guard.as_mut() {
            if let Some(lc) = map.get_mut(&handle) {
                if let Some(sock) = lc.ipc_send.as_mut() {
                    return sock.write_all(&frame).is_ok();
                }
            }
        }
        false
    }
}

/// Pass a connection fd to a child over its IPC socketpair via `SCM_RIGHTS`
/// (#4962 SCHED_RR). Holds the live-child lock so the framed send is atomic
/// against `cp_ipc_send`. Returns false (caller closes the fd) when the child
/// is gone.
#[cfg(unix)]
pub fn cp_ipc_send_fd(handle: u64, key_id: u32, payload_fd: std::os::unix::io::RawFd) -> bool {
    use std::os::unix::io::AsRawFd;
    let guard = cp_live_lock();
    let Some(map) = guard.as_ref() else {
        return false;
    };
    let Some(lc) = map.get(&handle) else {
        return false;
    };
    let Some(sock) = lc.ipc_send.as_ref() else {
        return false;
    };
    crate::cluster_sched::send_fd_over(sock.as_raw_fd(), key_id, payload_fd)
}

/// `child.disconnect()` — shut the IPC socket down (the reader thread then sees
/// EOF and exits). Returns whether a channel was present. #1933.
pub(super) fn cp_ipc_disconnect(handle: u64) -> bool {
    #[cfg(not(unix))]
    {
        let _ = handle;
        return false;
    }

    #[cfg(unix)]
    {
        let mut guard = cp_live_lock();
        if let Some(map) = guard.as_mut() {
            if let Some(lc) = map.get_mut(&handle) {
                if let Some(sock) = lc.ipc_send.take() {
                    let _ = sock.shutdown(std::net::Shutdown::Both);
                    return true;
                }
            }
        }
        false
    }
}

/// `child_process.spawn(command[, args][, options])` — returns a live streaming
/// ChildProcess (#1934). `cmd_ptr`/`args_ptr` are raw (unboxed) `StringHeader`
/// / `ArrayHeader` pointers; `opts_ptr` is a raw heap pointer (or 0). The
/// object SHAPE matches the former buffered implementation (`pid`, `exitCode`,
/// `signalCode`, `killed`, `connected`, `spawnargs`, `spawnfile`, `stdout`,
/// `stderr`, `stdin`, `stdio`) plus the hidden `__cpHandle` registry key.
#[no_mangle]
pub extern "C" fn js_child_process_spawn_streams(
    cmd_ptr: i64,
    args_ptr: i64,
    opts_ptr: i64,
) -> f64 {
    cp_register_arities();
    cp_register_reactor_arities();

    let (cmd_str, arg_strs) = unsafe {
        (
            cp_read_string_header(cmd_ptr),
            cp_read_arg_strings(args_ptr),
        )
    };

    // `opts_ptr` arrives as a raw (unboxed) heap pointer; re-box it so the
    // options helpers can read `cwd`/`env`/`shell`. Small values mean
    // no-options (codegen passes 0) — leave it undefined then.
    let opts_val = if opts_ptr > 0x10000 {
        cp_box_ptr(opts_ptr as *const u8)
    } else {
        cp_undefined()
    };
    let abort_signal = cp_read_abort_signal(opts_val);

    // stdout/stderr Readable + stdin Writable sub-objects.
    let stdout_obj = cp_build_readable();
    let stderr_obj = cp_build_readable();
    let stdin_obj = cp_build_writable();
    let stdio_kinds = cp_read_stdio(opts_val, 3);
    let timeout = cp_read_timeout(opts_val);
    let kill_signal = cp_read_kill_signal(opts_val);

    // spawnargs = [argv0 ?? command, ...args]
    let mut spawnargs = crate::array::js_array_alloc((arg_strs.len() + 1) as u32);
    let argv0 = cp_spawnargs_argv0(&cmd_str, opts_val);
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&argv0));
    for a in &arg_strs {
        spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(a));
    }

    let cp_methods: [(&str, CpFn); 11] = [
        ("on", cp_cast2(cp_method_on)),
        ("once", cp_cast2(cp_method_on)),
        ("addListener", cp_cast2(cp_method_on)),
        ("prependListener", cp_cast2(cp_method_on)),
        ("removeListener", cp_cast2(cp_method_remove_listener)),
        ("off", cp_cast2(cp_method_remove_listener)),
        (
            "removeAllListeners",
            cp_cast1(cp_method_remove_all_listeners),
        ),
        ("emit", cp_cast2(cp_method_emit)),
        ("kill", cp_cast1(cp_method_kill)),
        ("ref", cp_cast0(cp_method_this0)),
        ("unref", cp_cast0(cp_method_this0)),
    ];
    let cp_obj = cp_build_object(&cp_methods, CP_SHAPE_ID + cp_methods.len() as u32);
    let cp = cp_box_ptr(cp_obj as *const u8);
    cp_install_dispose(cp);

    cp_set_field(cp, b"stdout", cp_stdio_js_value(stdio_kinds[1], stdout_obj));
    cp_set_field(cp, b"stderr", cp_stdio_js_value(stdio_kinds[2], stderr_obj));
    cp_set_field(cp, b"stdin", cp_stdio_js_value(stdio_kinds[0], stdin_obj));

    let mut stdio = crate::array::js_array_alloc(3);
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[0], stdin_obj));
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[1], stdout_obj));
    stdio = crate::array::js_array_push_f64(stdio, cp_stdio_js_value(stdio_kinds[2], stderr_obj));
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));

    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    cp_set_field(cp, b"spawnargs", cp_box_ptr(spawnargs as *const u8));
    cp_set_field(cp, b"spawnfile", cp_box_string(&cmd_str));

    // Build + launch the child (honoring `shell`/`cwd`/`env`), non-blocking.
    let mut command = cp_build_command(&cmd_str, &arg_strs, opts_val);
    cp_apply_live_stdio(&mut command, &stdio_kinds);

    match command.spawn() {
        Ok(child) => {
            let handle = cp_register_live_child(
                cp,
                stdout_obj,
                stderr_obj,
                stdin_obj,
                child,
                None,
                false,
                timeout,
                kill_signal,
            );
            cp_install_abort_signal(handle, abort_signal, opts_val);
        }
        Err(e) => {
            // Spawn failure (e.g. ENOENT): Node emits a single `error` event and
            // never `spawn`/`exit`. Defer it so a synchronously-registered
            // handler is present. The error carries Node's errno shape:
            // `code`/`errno`/`syscall`/`path`/`spawnargs`, message
            // `spawn <cmd> <CODE>`.
            cp_set_field(cp, b"pid", cp_undefined());
            let code = super::cp_io_error_code(&e);
            let syscall = format!("spawn {cmd_str}");
            let message = format!("{syscall} {code}");
            // Node's error `spawnargs` excludes argv0 (internally `slice(1)`
            // of the handle's spawnargs), unlike `child.spawnargs`.
            let mut err_args = crate::array::js_array_alloc(arg_strs.len() as u32);
            for a in &arg_strs {
                err_args = crate::array::js_array_push_f64(err_args, cp_box_string(a));
            }
            let err = super::cp_make_error(
                &message,
                &[
                    ("errno", super::cp_errno_number(code)),
                    ("code", cp_box_string(code)),
                    ("syscall", cp_box_string(&syscall)),
                    ("path", cp_box_string(&cmd_str)),
                    ("spawnargs", cp_box_ptr(err_args as *const u8)),
                ],
            );
            cp_set_field(cp, b"__cpError", err);
            let emit_closure =
                crate::closure::js_closure_alloc(cp_emit_spawn_error as *const u8, 1);
            crate::closure::js_closure_set_capture_ptr(emit_closure, 0, cp.to_bits() as i64);
            crate::timer::js_set_immediate_callback(emit_closure as i64);
        }
    }

    cp
}

/// Deferred single-`error` emit for the spawn/fork failure path. Slot 0
/// captures the ChildProcess value.
pub(super) extern "C" fn cp_emit_spawn_error(closure: *const ClosureHeader) -> f64 {
    let cp = cp_this(closure);
    let err = cp_get_field(cp, b"__cpError");
    if !JSValue::from_bits(err.to_bits()).is_undefined() {
        cp_emit(cp, "error", &[err]);
    }
    cp_undefined()
}

pub(super) fn cp_register_reactor_arities() {
    crate::closure::js_register_closure_arity(cp_emit_spawn_error as *const u8, 0);
    crate::closure::js_register_closure_arity(cp_abort_listener as *const u8, 0);
    crate::closure::js_register_closure_arity(cp_exec_cb_thunk as *const u8, 0);
}

// ============================================================================
// Async `exec` / `execFile` — run off the main thread, call back on a later
// tick (#4912).
// ============================================================================

/// Launch `command` (already shaped as `sh -c <cmd>` for `exec` or `file
/// args…` for `execFile`, with `cwd`/`env` applied) without blocking the main
/// thread, capturing stdout/stderr on background reader threads. When the child
/// exits, the pump fires `cb_val(err, stdout, stderr)` on a later event-loop
/// tick. A spawn failure (e.g. `ENOENT`) is reported the same way, deferred to
/// the next tick. Always returns `undefined` (the callback form's return).
pub(super) fn cp_exec_async(
    mut command: Command,
    cmd_str: String,
    cb_val: f64,
    run_options: CpRunOptions,
    mode: CpOutput,
) -> f64 {
    cp_register_arities();
    cp_register_reactor_arities();

    // The program actually launched (`sh` for exec, the file for execFile) —
    // Node's spawn-failure error keys `syscall`/`path`/message off this, not
    // off the display command string.
    let file = command.get_program().to_string_lossy().into_owned();

    // exec/execFile capture stdout+stderr and never feed stdin.
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let timeout = run_options.timeout();
    let kill_signal = run_options.kill_signal();

    match command.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            // Duplicate the process handle BEFORE `child` moves to the waiter
            // thread — the kill paths act on it instead of the recyclable pid.
            #[cfg(windows)]
            let win_proc_handle = cp_win_dup_proc_handle(&child);
            let stdout_pipe = child.stdout.take();
            let stderr_pipe = child.stderr.take();
            let stdout_open = stdout_pipe.is_some();
            let stderr_open = stderr_pipe.is_some();
            let handle = CP_NEXT_LIVE_ID.fetch_add(1, Ordering::SeqCst);

            let exec = Box::new(CpExecPending {
                cb_bits: cb_val.to_bits(),
                stdout: Vec::new(),
                stderr: Vec::new(),
                run_options,
                mode,
                cmd: cmd_str,
                file,
                exceeded: false,
                timed_out: false,
            });

            {
                let mut guard = cp_live_lock();
                let map = guard.get_or_insert_with(HashMap::new);
                map.insert(
                    handle,
                    LiveChild {
                        // No JS ChildProcess object for the exec callback form.
                        cp_bits: TAG_UNDEFINED_BITS,
                        pid: pid as i32,
                        stdin: None,
                        stdout_open,
                        stderr_open,
                        spawned: false,
                        exited: None,
                        closed: false,
                        ipc_send: None,
                        ipc_advanced: false,
                        abort_signal_bits: 0,
                        abort_listener_bits: 0,
                        abort_kill_signal: libc_sigterm(),
                        #[cfg(windows)]
                        win_kill_signal: None,
                        #[cfg(windows)]
                        win_proc_handle,
                        exec: Some(exec),
                    },
                );
            }
            CP_LIVE_COUNT.fetch_add(1, Ordering::SeqCst);

            if let Some(o) = stdout_pipe {
                cp_spawn_reader(handle, o, false);
            }
            if let Some(e) = stderr_pipe {
                cp_spawn_reader(handle, e, true);
            }
            cp_spawn_waiter(handle, child);
            if let Some(timeout) = timeout {
                cp_spawn_timeout(handle, timeout, kill_signal);
            }
            crate::event_pump::js_notify_main_thread();
        }
        Err(e) => {
            // Could not spawn at all (ENOENT, EACCES…). Build the same callback
            // error the sync path produced and fire it on a later tick.
            let run = CpRun {
                stdout: Vec::new(),
                stderr: Vec::new(),
                stdout_piped: true,
                stderr_piped: true,
                code: None,
                signal: None,
                pid: None,
                spawn_error: Some((super::cp_io_error_code(&e), e.to_string())),
                run_error: None,
            };
            let (err, out, errout) =
                super::cp_exec_callback_args(&run, &run_options, &cmd_str, &file, &mode);
            cp_defer_exec_callback(cb_val, err, out, errout);
        }
    }

    cp_undefined()
}

/// Append a chunk to an exec child's captured stdout/stderr. Returns
/// `Some(kill_signal)` the first time the buffer overruns `maxBuffer`, so the
/// caller terminates the child (Node kills on a `maxBuffer` breach). Once
/// overrun, further chunks are dropped to bound memory — the one chunk that
/// crossed the limit keeps `len > maxBuffer`, which the output/error shapers
/// use to truncate and to name the offending stream.
fn cp_exec_accumulate(exec: &mut CpExecPending, stderr: bool, bytes: &[u8]) -> Option<i32> {
    if exec.exceeded {
        return None;
    }
    let max = exec.run_options.max_buffer;
    let buf = if stderr {
        &mut exec.stderr
    } else {
        &mut exec.stdout
    };
    buf.extend_from_slice(bytes);
    if buf.len() > max {
        exec.exceeded = true;
        return Some(exec.run_options.kill_signal());
    }
    None
}

/// Build the `CpRun` for a finished exec child and fire its callback.
fn cp_exec_fire_close(exec: Box<CpExecPending>, code: Option<i32>, signal: Option<i32>, pid: i32) {
    let exec = *exec;
    let run_error = if exec.exceeded {
        Some(CpRunError::MaxBuffer)
    } else if exec.timed_out {
        Some(CpRunError::Timeout)
    } else {
        None
    };
    // A timeout maps to Node's `(code: null, signal: killSignal)` shape.
    let (code, signal) = if exec.timed_out {
        (None, Some(exec.run_options.kill_signal()))
    } else {
        (code, signal)
    };
    let run = CpRun {
        stdout: exec.stdout,
        stderr: exec.stderr,
        stdout_piped: true,
        stderr_piped: true,
        code,
        signal,
        pid: Some(pid as u32),
        spawn_error: None,
        run_error,
    };
    let (err, out, errout) =
        super::cp_exec_callback_args(&run, &exec.run_options, &exec.cmd, &exec.file, &exec.mode);
    let cb = crate::fs::extract_closure_ptr(f64::from_bits(exec.cb_bits));
    if !cb.is_null() {
        crate::closure::js_closure_call3(cb, err, out, errout);
    }
}

/// Schedule a deferred `cb(err, stdout, stderr)` on the next `setImmediate`
/// macrotask — used for the empty-command, already-aborted, and spawn-failure
/// exec paths, which have no live child to ride the reactor close. The boxed
/// values are kept reachable by the immediate closure's captures.
pub(super) fn cp_defer_exec_callback(cb_val: f64, err: f64, stdout: f64, stderr: f64) {
    cp_register_reactor_arities();
    let deferred = js_closure_alloc(cp_exec_cb_thunk as *const u8, 4);
    js_closure_set_capture_ptr(deferred, 0, cb_val.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 1, err.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 2, stdout.to_bits() as i64);
    js_closure_set_capture_ptr(deferred, 3, stderr.to_bits() as i64);
    crate::timer::js_set_immediate_callback(deferred as i64);
}

/// The deferred-callback thunk: slots 0..4 capture `cb`, `err`, `stdout`,
/// `stderr`. Takes no JS args.
extern "C" fn cp_exec_cb_thunk(closure: *const ClosureHeader) -> f64 {
    let cb = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let err = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let out = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    let errout = f64::from_bits(js_closure_get_capture_ptr(closure, 3) as u64);
    let cbptr = crate::fs::extract_closure_ptr(cb);
    if !cbptr.is_null() {
        crate::closure::js_closure_call3(cbptr, err, out, errout);
    }
    cp_undefined()
}

// ============================================================================
// Main-thread pump
// ============================================================================

/// Drive the reactor one tick: emit pending `spawn`/`data`/`end`/`exit`/`close`
/// for all live children. Called from `js_run_stdlib_pump`.
pub(crate) fn cp_reactor_pump() {
    if CP_LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if CP_PUMPING.with(|p| p.replace(true)) {
        return; // already pumping (re-entrant await inside a handler)
    }
    cp_reactor_pump_inner();
    CP_PUMPING.with(|p| p.set(false));
}

fn cp_reactor_pump_inner() {
    // --- Phase 0: emit `spawn` for newly-registered children. ---
    // Snapshot under a brief lock, then emit OUTSIDE the lock (emit calls user
    // callbacks which allocate / can trigger GC; the GC root scanner also locks
    // CP_LIVE, so holding it across an allocation would deadlock the same
    // thread).
    // exec/execFile children (#4912) have no JS ChildProcess, so they get no
    // `spawn` event — just mark them spawned so Phase B can close them.
    let to_spawn: Vec<(u64, u64, bool)> = {
        let guard = cp_live_lock();
        match guard.as_ref() {
            Some(map) => map
                .iter()
                .filter(|(_, lc)| !lc.spawned)
                .map(|(h, lc)| (*h, lc.cp_bits, lc.exec.is_some()))
                .collect(),
            None => Vec::new(),
        }
    };
    for (handle, cp_bits, is_exec) in to_spawn {
        if !is_exec {
            cp_emit(f64::from_bits(cp_bits), "spawn", &[]);
        }
        if let Some(map) = cp_live_lock().as_mut() {
            if let Some(lc) = map.get_mut(&handle) {
                lc.spawned = true;
            }
        }
    }

    // --- Phase A: drain queued data/eof/exited events. ---
    let events = std::mem::take(&mut *cp_queue_lock());
    for ev in events {
        match ev {
            CpEvent::Data {
                handle,
                stderr,
                bytes,
            } => {
                // exec/execFile (#4912): buffer the bytes (off-JS, under the
                // lock) instead of emitting a stream `data` event. A `maxBuffer`
                // breach kills the child.
                let mut emit_cp_bits = None;
                let mut kill_sig = None;
                {
                    let mut guard = cp_live_lock();
                    if let Some(lc) = guard.as_mut().and_then(|m| m.get_mut(&handle)) {
                        match lc.exec.as_mut() {
                            Some(exec) => kill_sig = cp_exec_accumulate(exec, stderr, &bytes),
                            None => emit_cp_bits = Some(lc.cp_bits),
                        }
                    }
                }
                if let Some(sig) = kill_sig {
                    cp_live_kill_signal(handle, sig);
                }
                if let Some(cp_bits) = emit_cp_bits {
                    let cp = f64::from_bits(cp_bits);
                    let stream = cp_get_field(cp, cp_stream_field(stderr));
                    if super::cp_object_ptr(stream).is_some() {
                        let buf = cp_make_buffer(&bytes);
                        cp_emit(stream, "data", &[buf]);
                    }
                }
            }
            CpEvent::Eof { handle, stderr } => {
                if let Some(map) = cp_live_lock().as_mut() {
                    if let Some(lc) = map.get_mut(&handle) {
                        if stderr {
                            lc.stderr_open = false;
                        } else {
                            lc.stdout_open = false;
                        }
                    }
                }
                if let Some(cp_bits) = cp_lookup_cp_bits(handle) {
                    let cp = f64::from_bits(cp_bits);
                    let stream = cp_get_field(cp, cp_stream_field(stderr));
                    if super::cp_object_ptr(stream).is_some() {
                        cp_emit(stream, "end", &[]);
                    }
                }
            }
            CpEvent::Exited {
                handle,
                code,
                signal,
            } => {
                if let Some(map) = cp_live_lock().as_mut() {
                    if let Some(lc) = map.get_mut(&handle) {
                        // Windows: a kill()/timeout/abort termination surfaces
                        // here as the synthetic `TerminateProcess` exit code
                        // with no signal. Substitute the signal recorded at
                        // kill time so the exit event carries Node's
                        // `(code: null, signal: <requested>)` shape — see
                        // `LiveChild::win_kill_signal`.
                        #[cfg(windows)]
                        let (code, signal) = match lc.win_kill_signal {
                            Some(sig) if signal.is_none() => (None, Some(sig)),
                            _ => (code, signal),
                        };
                        lc.exited = Some((code, signal));
                    }
                }
            }
            CpEvent::Message { handle, json } => {
                // #1933: parse the IPC line on the main thread and deliver it as
                // a `'message'` event on the ChildProcess.
                if let Some(cp_bits) = cp_lookup_cp_bits(handle) {
                    let cp = f64::from_bits(cp_bits);
                    let sh = crate::string::js_string_from_bytes(json.as_ptr(), json.len() as u32);
                    let msg = f64::from_bits(unsafe { crate::json::js_json_parse(sh) }.bits());
                    cp_emit(cp, "message", &[msg]);
                }
            }
            CpEvent::MessageAdvanced { handle, bytes } => {
                // #2130: deserialize the V8 structured-clone payload on the main
                // thread and deliver it as a `'message'` event.
                if let Some(cp_bits) = cp_lookup_cp_bits(handle) {
                    let cp = f64::from_bits(cp_bits);
                    let msg = super::v8_serde::v8_deserialize(&bytes);
                    cp_emit(cp, "message", &[msg]);
                }
            }
            CpEvent::IpcClosed { handle } => {
                // #1933: child disconnected / exited — flip connected/channel
                // and emit `disconnect` once.
                if let Some(cp_bits) = cp_lookup_cp_bits(handle) {
                    let cp = f64::from_bits(cp_bits);
                    if cp_get_field(cp, b"connected").to_bits() == TAG_TRUE_F64.to_bits() {
                        cp_set_field(cp, b"connected", TAG_FALSE_F64);
                        cp_set_field(cp, b"channel", TAG_NULL_F64);
                        if let Some(map) = cp_live_lock().as_mut() {
                            if let Some(lc) = map.get_mut(&handle) {
                                lc.ipc_send = None;
                            }
                        }
                        cp_emit(cp, "disconnect", &[]);
                    }
                }
            }
            CpEvent::Timeout { handle, signal } => {
                // Mark an exec child as timed-out so its callback carries the
                // timeout error shape (#4912); a `spawn` child flips `killed`.
                let mut is_exec = false;
                {
                    let mut guard = cp_live_lock();
                    if let Some(lc) = guard.as_mut().and_then(|m| m.get_mut(&handle)) {
                        if let Some(exec) = lc.exec.as_mut() {
                            exec.timed_out = true;
                            is_exec = true;
                        }
                    }
                }
                if let Some(cp_bits) = cp_live_kill_signum(handle, signal) {
                    if !is_exec {
                        cp_set_field(f64::from_bits(cp_bits), b"killed", TAG_TRUE_F64);
                    }
                }
            }
            CpEvent::Abort { handle } => {
                let Some((cp_bits, kill_signal, signal_bits, listener_bits)) =
                    cp_take_abort_fields(handle)
                else {
                    continue;
                };
                cp_cleanup_abort_listener(signal_bits, listener_bits);
                if cp_live_kill_signal(handle, kill_signal) {
                    let cp = f64::from_bits(cp_bits);
                    cp_set_field(cp, b"killed", TAG_TRUE_F64);
                    cp_emit(cp, "error", &[cp_abort_error(None)]);
                }
            }
        }
    }

    // --- Phase B: emit `exit`+`close` once a child has exited AND both
    // streams have hit EOF, so all `data`/`end` have already fired. For an
    // exec/execFile child (#4912) the terminal step is its buffered callback
    // instead of `exit`/`close` events. ---
    let to_close: Vec<CpCloseItem> = {
        let mut guard = cp_live_lock();
        let mut out = Vec::new();
        if let Some(map) = guard.as_mut() {
            for (h, lc) in map.iter_mut() {
                if lc.closed || !lc.spawned {
                    continue;
                }
                if let Some((code, signal)) = lc.exited {
                    if !lc.stdout_open && !lc.stderr_open {
                        lc.closed = true;
                        out.push(CpCloseItem {
                            handle: *h,
                            cp_bits: lc.cp_bits,
                            code,
                            signal,
                            pid: lc.pid,
                            abort_signal_bits: lc.abort_signal_bits,
                            abort_listener_bits: lc.abort_listener_bits,
                            exec: lc.exec.take(),
                        });
                        lc.abort_signal_bits = 0;
                        lc.abort_listener_bits = 0;
                    }
                }
            }
        }
        out
    };
    for item in to_close {
        cp_cleanup_abort_listener(item.abort_signal_bits, item.abort_listener_bits);
        if let Some(exec) = item.exec {
            cp_exec_fire_close(exec, item.code, item.signal, item.pid);
        } else {
            let cp = f64::from_bits(item.cp_bits);
            let code_f = item.code.map(|c| c as f64).unwrap_or(TAG_NULL_F64);
            let signal_f = item
                .signal
                .map(|s| cp_box_string(cp_signal_name(s)))
                .unwrap_or(TAG_NULL_F64);
            // Node populates exitCode/signalCode before emitting `exit`, then `close`.
            cp_set_field(cp, b"exitCode", code_f);
            cp_set_field(cp, b"signalCode", signal_f);
            cp_emit(cp, "exit", &[code_f, signal_f]);
            cp_emit(cp, "close", &[code_f, signal_f]);
        }
        if let Some(map) = cp_live_lock().as_mut() {
            map.remove(&item.handle);
        }
        CP_LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A child ready to enter its terminal step in Phase B — either `exit`/`close`
/// events (`exec` is `None`) or an exec/execFile callback (`exec` is `Some`).
struct CpCloseItem {
    handle: u64,
    cp_bits: u64,
    code: Option<i32>,
    signal: Option<i32>,
    pid: i32,
    abort_signal_bits: u64,
    abort_listener_bits: u64,
    exec: Option<Box<CpExecPending>>,
}

#[inline]
fn cp_stream_field(stderr: bool) -> &'static [u8] {
    if stderr {
        b"stderr"
    } else {
        b"stdout"
    }
}

fn cp_lookup_cp_bits(handle: u64) -> Option<u64> {
    cp_live_lock()
        .as_ref()
        .and_then(|map| map.get(&handle).map(|lc| lc.cp_bits))
}

// ============================================================================
// Live `stdin.write()` / `kill()` — called from the mod.rs method bodies.
// ============================================================================

/// Write `bytes` to a live child's stdin. Returns whether the write succeeded.
pub(super) fn cp_live_stdin_write(handle: u64, bytes: &[u8]) -> bool {
    let mut guard = cp_live_lock();
    if let Some(map) = guard.as_mut() {
        if let Some(lc) = map.get_mut(&handle) {
            if let Some(stdin) = lc.stdin.as_mut() {
                return stdin.write_all(bytes).is_ok();
            }
        }
    }
    false
}

/// Close a live child's stdin (`stdin.end()`), dropping the pipe so the child
/// sees EOF. No-op if already closed / unknown.
pub(super) fn cp_live_stdin_close(handle: u64) {
    if let Some(map) = cp_live_lock().as_mut() {
        if let Some(lc) = map.get_mut(&handle) {
            lc.stdin = None;
        }
    }
}

/// Duplicate `child`'s process handle before the `Child` moves to the waiter
/// thread, so the kill paths can act on the process object itself rather than
/// the recyclable pid (see `cp_win_kill`). The registry entry owns the
/// duplicate; `LiveChild::drop` closes it. Returns `0` when duplication fails
/// — kills on that child then report undelivered rather than falling back to
/// a racy pid-based kill.
#[cfg(windows)]
fn cp_win_dup_proc_handle(child: &Child) -> isize {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, HANDLE};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut dup: HANDLE = std::ptr::null_mut();
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            child.as_raw_handle() as HANDLE,
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
fn cp_win_kill(proc_handle: isize, signum: i32) -> bool {
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
fn cp_note_win_kill(handle: u64, signum: i32) {
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
fn cp_kill_target(lc: &LiveChild) -> i32 {
    lc.pid
}
#[cfg(windows)]
#[inline]
fn cp_kill_target(lc: &LiveChild) -> isize {
    lc.win_proc_handle
}

fn cp_live_kill_signum(handle: u64, signum: i32) -> Option<u64> {
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
pub(super) fn cp_live_kill(handle: u64, signal: f64) -> bool {
    cp_live_kill_signum(handle, cp_signal_from_value(signal)).is_some()
}

fn cp_live_kill_signal(handle: u64, signum: i32) -> bool {
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
fn cp_parse_signal(signal: f64) -> i32 {
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
fn cp_signal_number(name: &str) -> Option<i32> {
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

// ============================================================================
// Event-loop integration hooks (wired from lib.rs / gc/mod.rs).
// ============================================================================

/// Whether any live child is keeping the event loop alive — OR'd into
/// `js_stdlib_has_active_handles`.
pub(crate) fn cp_reactor_has_live() -> bool {
    CP_LIVE_COUNT.load(Ordering::Relaxed) > 0
}

/// GC mutable-root scanner: keep every live ChildProcess (and its reachable
/// stdio sub-objects + listener arrays) alive across collections, and rewrite
/// the stored pointer on evacuation.
pub(crate) fn cp_reactor_scan_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if CP_LIVE_COUNT.load(Ordering::Relaxed) == 0 {
        return;
    }
    if let Some(map) = cp_live_lock().as_mut() {
        for lc in map.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut lc.cp_bits);
            if lc.abort_signal_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut lc.abort_signal_bits);
            }
            if lc.abort_listener_bits != 0 {
                visitor.visit_nanbox_u64_slot(&mut lc.abort_listener_bits);
            }
            // #4912: keep the async exec/execFile callback closure alive until
            // it fires on `close`.
            if let Some(exec) = lc.exec.as_mut() {
                visitor.visit_nanbox_u64_slot(&mut exec.cb_bits);
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

/// Windows termination tests. `child.kill()`, spawn `{ timeout }`,
/// `AbortSignal`, and the exec `maxBuffer` breach all funnel through
/// `cp_live_kill_signum` / `cp_live_kill_signal` → `cp_win_kill`, so
/// terminating one live child through the shared path exercises the machinery
/// all of them rely on.
#[cfg(all(test, windows))]
mod windows_kill_tests {
    use super::*;

    /// Raw `cp_win_dup_proc_handle` + `cp_win_kill`: sig-0 existence probe on
    /// a live child, terminate through the duplicated handle, probe + kill
    /// failure after death. The held duplicate keeps naming the original
    /// process object even once the child is reaped — exactly the property
    /// that closes the pid-reuse race.
    #[test]
    fn win_kill_probe_and_terminate() {
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn ping");
        let proc_handle = cp_win_dup_proc_handle(&child);
        assert_ne!(proc_handle, 0, "DuplicateHandle should succeed");

        // POSIX `kill(pid, 0)` analogue: existence probe, no side effect.
        assert!(
            cp_win_kill(proc_handle, 0),
            "probe should see the live child"
        );

        // Any terminating signal degrades to `TerminateProcess(handle, 1)`.
        assert!(cp_win_kill(proc_handle, 15), "terminate should succeed");
        let status = child.wait().expect("wait after TerminateProcess");
        assert_eq!(status.code(), Some(1), "TerminateProcess exit code");

        // The duplicate still names the original (now-dead) process after the
        // reap, so both the probe and a second kill deterministically fail —
        // no pid-recycling flake window exists for a handle.
        assert!(!cp_win_kill(proc_handle, 0), "probe should fail once dead");
        assert!(
            !cp_win_kill(proc_handle, 15),
            "kill after death reports undelivered"
        );

        // The test owns this duplicate (no LiveChild registry entry) — close
        // it by hand.
        unsafe {
            let _ = windows_sys::Win32::Foundation::CloseHandle(
                proc_handle as windows_sys::Win32::Foundation::HANDLE,
            );
        }
    }

    /// Registry-level liveness: the pump removes the entry once the child has
    /// fully closed (exit reported + both streams at EOF).
    fn handle_is_live(handle: u64) -> bool {
        cp_live_lock()
            .as_ref()
            .is_some_and(|map| map.contains_key(&handle))
    }

    /// End-to-end through the reactor: spawn a long-running child via the
    /// spawn FFI, terminate it through the same shared path `child.kill()` /
    /// `{ timeout }` / `AbortSignal` use, and drive the pump until the exit
    /// machinery completes. Asserts the Node-shaped exit for a Windows kill:
    /// `exitCode: null`, `signalCode: 'SIGTERM'`.
    #[test]
    fn reactor_kill_terminates_live_child() {
        // `ping -n 30 127.0.0.1` runs ~29s if not killed — long enough that a
        // pass can only come from the kill path, short enough to bound a
        // failure without hanging the suite.
        let cmd = "ping";
        let cmd_ptr = crate::string::js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
        let mut args = crate::array::js_array_alloc(3);
        for a in ["-n", "30", "127.0.0.1"] {
            let s = crate::string::js_string_from_bytes(a.as_ptr(), a.len() as u32);
            args = crate::array::js_array_push_f64(args, crate::value::js_nanbox_string(s as i64));
        }
        let start = std::time::Instant::now();
        let cp = js_child_process_spawn_streams(cmd_ptr as i64, args as i64, 0);

        let pid = cp_get_field(cp, b"pid");
        assert!(pid > 0.0, "spawn should set a real pid, got {pid}");
        let handle = cp_get_field(cp, b"__cpHandle") as u64;
        assert!(handle_is_live(handle));

        // Phase 0 marks `spawned` — Phase B refuses to close before that.
        cp_reactor_pump();

        // Kill through the shared path (undefined signal → SIGTERM).
        assert!(
            cp_live_kill(handle, cp_undefined()),
            "kill should be delivered"
        );

        // Drive the pump until the exit machinery completes: waiter reaps →
        // `Exited` event → exit/close emitted → registry entry removed.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while handle_is_live(handle) {
            assert!(
                std::time::Instant::now() < deadline,
                "child did not close within 15s of kill()"
            );
            cp_reactor_pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        // Well under ping's ~29s natural runtime — it died from the kill.
        assert!(start.elapsed() < Duration::from_secs(20));

        // Node's Windows kill shape: exitCode null, signalCode 'SIGTERM'
        // (the requested signal, not TerminateProcess's synthetic exit code).
        assert_eq!(
            cp_get_field(cp, b"exitCode").to_bits(),
            TAG_NULL_F64.to_bits(),
            "exitCode should be null after a signal kill"
        );
        assert_eq!(
            cp_value_to_string(cp_get_field(cp, b"signalCode")).as_deref(),
            Some("SIGTERM")
        );

        // The child is reaped and deregistered — a second kill reports false.
        assert!(!cp_live_kill(handle, cp_undefined()));
    }
}
