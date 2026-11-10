//! A child's stdio pipe slots as `net.Socket`s (unix), as Node's
//! `ChildProcess.spawn` wraps each pipe handle with `createSocket(pipe, i > 0)`
//! once the spawn succeeded. The ChildProcess closes the way Node's
//! `maybeClose` does: `_closesNeeded` is 1 (the exit) + every readable pipe
//! socket + the IPC channel; the exit, each readable socket's `'close'` and
//! the IPC close each bump `_closesGot`, and `'close'` follows at equality.

use super::*;

/// The socket (or `null`) of every stdio slot, and the readable ones whose
/// `'close'` the ChildProcess waits for.
#[cfg(unix)]
pub(in crate::child_process) struct CpPipeSockets {
    pub stdio: Vec<f64>,
    pub readable: Vec<(usize, f64)>,
}

#[cfg(unix)]
impl CpPipeSockets {
    pub fn slot(&self, fd: usize) -> f64 {
        self.stdio.get(fd).copied().unwrap_or(TAG_NULL_F64)
    }
}

/// Wrap each pipe slot's parent end in a socket that owns it, and publish
/// `stdin`/`stdout`/`stderr`/`stdio` on the ChildProcess. Slots that are not
/// pipes read `null`.
#[cfg(unix)]
pub(in crate::child_process) fn cp_attach_pipe_sockets(
    cp: f64,
    kinds: &[CpStdio],
    parents: Vec<(usize, std::os::fd::OwnedFd)>,
) -> CpPipeSockets {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp);
    let count = kinds.len().max(3);
    let mut slots: Vec<_> = (0..count)
        .map(|_| scope.root_nanbox_f64(TAG_NULL_F64))
        .collect();
    for (fd, parent) in parents {
        let socket = super::super::builder::cp_pipe_socket(fd > 0, parent);
        slots[fd] = scope.root_nanbox_f64(socket);
    }
    let mut stdio = scope.root_nanbox_f64(cp_box_ptr(
        crate::array::js_array_alloc(count as u32) as *const u8
    ));
    for slot in &slots {
        let array = cp_array_ptr(stdio.get_nanbox_f64()).expect("stdio array");
        let grown = crate::array::js_array_push_f64(array, slot.get_nanbox_f64());
        stdio = scope.root_nanbox_f64(cp_box_ptr(grown as *const u8));
    }
    let cp_value = cp.get_nanbox_f64();
    cp_set_field(cp_value, b"stdin", slots[0].get_nanbox_f64());
    cp_set_field(cp_value, b"stdout", slots[1].get_nanbox_f64());
    cp_set_field(cp_value, b"stderr", slots[2].get_nanbox_f64());
    cp_set_field(cp_value, b"stdio", stdio.get_nanbox_f64());
    let stdio: Vec<f64> = slots.iter().map(|slot| slot.get_nanbox_f64()).collect();
    let readable = stdio
        .iter()
        .copied()
        .enumerate()
        .skip(1)
        .filter(|(_, socket)| socket.to_bits() != TAG_NULL_F64.to_bits())
        .collect();
    CpPipeSockets { stdio, readable }
}

/// Count each readable pipe socket's `'close'` toward the ChildProcess's.
#[cfg(unix)]
pub(in crate::child_process) fn cp_watch_pipe_closes(handle: u64, sockets: &CpPipeSockets) {
    for &(_, socket) in &sockets.readable {
        let listener = js_closure_alloc(
            crate::fn_info!(cp_pipe_socket_closed, 0; with_declared(0)),
            1,
        );
        crate::closure::closure_set_capture_word(listener, 0, handle as i64);
        cp_listen(socket, "close", cp_box_ptr(listener as *const u8));
    }
}

/// exec/execFile: buffer what the stdout/stderr sockets read into the pending
/// callback's output (`maxBuffer` still kills the child).
#[cfg(unix)]
pub(in crate::child_process) fn cp_watch_exec_output(handle: u64, sockets: &CpPipeSockets) {
    for &(fd, socket) in &sockets.readable {
        let listener =
            js_closure_alloc(crate::fn_info!(cp_exec_socket_data, 1; with_declared(1)), 2);
        crate::closure::closure_set_capture_word(listener, 0, handle as i64);
        crate::closure::closure_set_capture_word(listener, 1, fd as i64);
        cp_listen(socket, "data", cp_box_ptr(listener as *const u8));
    }
}

#[cfg(unix)]
fn cp_listen(socket: f64, event: &str, listener: f64) {
    let args = [cp_box_string(event), listener];
    // SAFETY: `socket` is a live net.Socket; `on` is its EventEmitter method.
    unsafe {
        crate::object::js_native_call_method(
            socket,
            b"on".as_ptr() as *const i8,
            2,
            args.as_ptr(),
            2,
        );
    }
}

#[cfg(unix)]
extern "C" fn cp_pipe_socket_closed(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let handle = js_closure_get_capture_ptr(closure, 0) as u64;
    cp_count_close(handle);
    cp_undefined()
}

#[cfg(unix)]
extern "C" fn cp_exec_socket_data(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    chunk: f64,
) -> f64 {
    let handle = js_closure_get_capture_ptr(closure, 0) as u64;
    let fd = js_closure_get_capture_ptr(closure, 1) as usize;
    let mut bytes = Vec::new();
    crate::node_stream::chunk_bytes(chunk, &mut bytes);
    let kill = cp_live_lock()
        .as_mut()
        .and_then(|map| map.get_mut(&handle))
        .and_then(|lc| lc.exec.as_mut())
        .and_then(|exec| cp_exec_accumulate(exec, fd == 2, &bytes));
    if let Some(signal) = kill {
        cp_live_kill_signal(handle, signal);
    }
    cp_undefined()
}

/// One of the ChildProcess's closes (exit, a readable pipe socket, the IPC
/// channel) happened: bump `_closesGot`. On unix the close that completes the
/// count after `exit` runs the terminal step right here, as Node's
/// `maybeClose` emits `'close'` inside that socket's `'close'` (before the
/// program's own listeners on it) or right after `'exit'`. Otherwise the pump
/// emits `'close'`.
pub(in crate::child_process) fn cp_count_close(handle: u64) {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut finish: Option<CpCloseItem> = None;
    let counted = cp_live_lock()
        .as_mut()
        .and_then(|map| map.get_mut(&handle))
        .map(|lc| {
            lc.closes_got += 1;
            #[cfg(unix)]
            if lc.exit_emitted
                && !lc.closed
                && lc.closes_got >= lc.closes_needed
                && crate::agent::owns(lc.owner)
            {
                lc.closed = true;
                let (code, signal) = lc.exited.unwrap_or((None, None));
                finish = Some(CpCloseItem {
                    handle,
                    code,
                    signal,
                    pid: lc.pid,
                    abort_signal_bits: std::mem::take(&mut lc.abort_signal_bits),
                    abort_listener_bits: std::mem::take(&mut lc.abort_listener_bits),
                    exec: lc.exec.take(),
                    process_ids: lc.process_ids,
                    pipe_ids: lc.pipe_ids,
                    refed: lc.refed,
                    close: true,
                });
            }
            (lc.cp_bits, lc.closes_got)
        });
    if let Some((cp_bits, got)) = counted {
        cp_set_field(f64::from_bits(cp_bits), b"_closesGot", got as f64);
        match finish {
            Some(item) => cp_finish_close(item),
            None => crate::event_pump::js_notify_main_thread(),
        }
    }
}

/// A spawn that failed once its pipes existed (ENOENT, EACCES…): Node still
/// wraps every pipe slot in a socket (`createSocket(handle, i > 0)`) and the
/// ChildProcess waits for its exit (the error) and each readable socket's
/// `'close'` before its own `'close'`. The child ends never reached a child,
/// so each readable socket reads EOF and closes. A failed `fork` also waits
/// for its IPC channel, which closes in the check phase after the error.
/// There is no live child: the closes are counted on the ChildProcess.
#[cfg(unix)]
pub(in crate::child_process) fn cp_attach_failed_pipe_sockets(
    cp: f64,
    kinds: &[CpStdio],
    parents: Vec<(usize, std::os::fd::OwnedFd)>,
    ipc: bool,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp);
    let sockets = cp_attach_pipe_sockets(cp.get_nanbox_f64(), kinds, parents);
    let needed = 1 + sockets.readable.len() + ipc as usize;
    cp_set_field(cp.get_nanbox_f64(), b"_closesNeeded", needed as f64);
    cp_set_field(cp.get_nanbox_f64(), b"_closesGot", 0.0);
    let rooted: Vec<_> = sockets
        .readable
        .iter()
        .map(|&(_, socket)| scope.root_nanbox_f64(socket))
        .collect();
    for socket in &rooted {
        let listener = js_closure_alloc(
            crate::fn_info!(cp_failed_socket_closed, 0; with_declared(0)),
            1,
        );
        crate::closure::js_closure_set_capture_ptr(
            listener,
            0,
            cp.get_nanbox_f64().to_bits() as i64,
        );
        cp_listen(
            socket.get_nanbox_f64(),
            "close",
            cp_box_ptr(listener as *const u8),
        );
    }
    if ipc {
        let closed = js_closure_alloc(
            crate::fn_info!(cp_failed_socket_closed, 0; with_declared(0)),
            1,
        );
        crate::closure::js_closure_set_capture_ptr(closed, 0, cp.get_nanbox_f64().to_bits() as i64);
        crate::timer::schedule_native_immediate(closed as i64, &[], "PIPEWRAP");
    }
}

#[cfg(unix)]
extern "C" fn cp_failed_socket_closed(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let cp = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    cp_failed_count_close(cp);
    cp_undefined()
}

/// One close of a failed spawn (its exit, a readable pipe socket, the IPC
/// channel): Node's `maybeClose`, so `'close'` follows the last one at once.
#[cfg(unix)]
pub(in crate::child_process) fn cp_failed_count_close(cp: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let cp = scope.root_nanbox_f64(cp);
    let number = |key: &[u8]| {
        let value = cp_get_field(cp.get_nanbox_f64(), key);
        if JSValue::from_bits(value.to_bits()).is_number() {
            value
        } else {
            0.0
        }
    };
    let got = number(b"_closesGot") + 1.0;
    cp_set_field(cp.get_nanbox_f64(), b"_closesGot", got);
    if got == number(b"_closesNeeded") {
        let code = cp_get_field(cp.get_nanbox_f64(), b"exitCode");
        cp_emit(cp.get_nanbox_f64(), "close", &[code, TAG_NULL_F64]);
    }
}
