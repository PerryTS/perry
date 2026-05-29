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
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

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
) -> u64 {
    let pid = child.id();
    cp_set_field(cp, b"pid", pid as f64);

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

    // stdout/stderr Readable + stdin Writable sub-objects.
    let stdout_obj = cp_build_readable();
    let stderr_obj = cp_build_readable();
    let stdin_obj = cp_build_writable();

    // spawnargs = [command, ...args]
    let mut spawnargs = crate::array::js_array_alloc((arg_strs.len() + 1) as u32);
    spawnargs = crate::array::js_array_push_f64(spawnargs, cp_box_string(&cmd_str));
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

    cp_set_field(cp, b"stdout", stdout_obj);
    cp_set_field(cp, b"stderr", stderr_obj);
    cp_set_field(cp, b"stdin", stdin_obj);

    let mut stdio = crate::array::js_array_alloc(3);
    stdio = crate::array::js_array_push_f64(stdio, stdin_obj);
    stdio = crate::array::js_array_push_f64(stdio, stdout_obj);
    stdio = crate::array::js_array_push_f64(stdio, stderr_obj);
    cp_set_field(cp, b"stdio", cp_box_ptr(stdio as *const u8));

    cp_set_field(cp, b"exitCode", TAG_NULL_F64);
    cp_set_field(cp, b"signalCode", TAG_NULL_F64);
    cp_set_field(cp, b"killed", TAG_FALSE_F64);
    cp_set_field(cp, b"connected", TAG_FALSE_F64);
    cp_set_field(cp, b"spawnargs", cp_box_ptr(spawnargs as *const u8));
    cp_set_field(cp, b"spawnfile", cp_box_string(&cmd_str));

    // Build + launch the child (honoring `shell`/`cwd`/`env`), non-blocking.
    let mut command = cp_build_command(&cmd_str, &arg_strs, opts_val);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    match command.spawn() {
        Ok(child) => {
            cp_register_live_child(cp, stdout_obj, stderr_obj, stdin_obj, child, None, false);
        }
        Err(e) => {
            // Spawn failure (e.g. ENOENT): Node emits a single `error` event and
            // never `spawn`/`exit`. Defer it so a synchronously-registered
            // handler is present.
            cp_set_field(cp, b"pid", cp_undefined());
            let msg = e.to_string();
            let mp = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err = crate::error::js_error_new_with_message(mp);
            cp_set_field(
                cp,
                b"__cpError",
                crate::value::js_nanbox_pointer(err as i64),
            );
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
    let to_spawn: Vec<(u64, u64)> = {
        let guard = cp_live_lock();
        match guard.as_ref() {
            Some(map) => map
                .iter()
                .filter(|(_, lc)| !lc.spawned)
                .map(|(h, lc)| (*h, lc.cp_bits))
                .collect(),
            None => Vec::new(),
        }
    };
    for (handle, cp_bits) in to_spawn {
        cp_emit(f64::from_bits(cp_bits), "spawn", &[]);
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
                if let Some(cp_bits) = cp_lookup_cp_bits(handle) {
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
        }
    }

    // --- Phase B: emit `exit`+`close` once a child has exited AND both
    // streams have hit EOF, so all `data`/`end` have already fired. ---
    let to_close: Vec<(u64, u64, Option<i32>, Option<i32>)> = {
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
                        out.push((*h, lc.cp_bits, code, signal));
                    }
                }
            }
        }
        out
    };
    for (handle, cp_bits, code, signal) in to_close {
        let cp = f64::from_bits(cp_bits);
        let code_f = code.map(|c| c as f64).unwrap_or(TAG_NULL_F64);
        let signal_f = signal
            .map(|s| cp_box_string(cp_signal_name(s)))
            .unwrap_or(TAG_NULL_F64);
        // Node populates exitCode/signalCode before emitting `exit`, then `close`.
        cp_set_field(cp, b"exitCode", code_f);
        cp_set_field(cp, b"signalCode", signal_f);
        cp_emit(cp, "exit", &[code_f, signal_f]);
        cp_emit(cp, "close", &[code_f, signal_f]);
        if let Some(map) = cp_live_lock().as_mut() {
            map.remove(&handle);
        }
        CP_LIVE_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
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

/// Signal a live child. `signal` is the JS `kill([signal])` argument (a signal
/// name string, a number, or — for the no-arg / default case — undefined or the
/// `0.0` arg-padding, both treated as `SIGTERM`). Returns whether the signal
/// was delivered.
pub(super) fn cp_live_kill(handle: u64, signal: f64) -> bool {
    let pid = {
        let guard = cp_live_lock();
        match guard.as_ref().and_then(|map| map.get(&handle)) {
            // Skip if already reaped — the pid may have been recycled by the OS.
            Some(lc) if lc.exited.is_none() => lc.pid,
            _ => return false,
        }
    };
    #[cfg(unix)]
    {
        let signum = cp_parse_signal(signal);
        unsafe { libc::kill(pid, signum) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
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
    if let Some(name) = cp_value_to_string(signal) {
        return cp_signal_number(&name).unwrap_or(SIGTERM);
    }
    if signal.is_finite() {
        let n = signal as i32;
        // 0 is the "no-arg" padding sentinel — treat as the default SIGTERM.
        return if n == 0 { SIGTERM } else { n };
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
        }
    }
}
