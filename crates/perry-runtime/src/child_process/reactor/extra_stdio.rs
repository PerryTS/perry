//! Writable side of a child's extra `stdio` pipes (index >= 3), which are socketpair
//! ends. Writes go through a lazily started drain thread so the main thread never blocks
//! on a full socket; completion (write callbacks, `'drain'`, end callbacks, `'finish'`) is
//! reported back through the event queue. Stream state (`writableLength`, the pending
//! callbacks) lives on the JS stream object, which the child keeps reachable.

use super::*;

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_native_call_value,
    ClosureHeader, JsThis,
};
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::mpsc::{channel, Receiver, Sender};

const WRITE_CALLBACKS_KEY: &[u8] = b"__cpWcb";
const END_CALLBACKS_KEY: &[u8] = b"__cpEcb";

enum Msg {
    Data(Vec<u8>),
    Shutdown,
}

/// The parent's write handle on one extra pipe: a dup of the socket the reader owns.
pub(super) struct ExtraWriter {
    fd: usize,
    sock: Option<OwnedFd>,
    tx: Option<Sender<Msg>>,
}

impl ExtraWriter {
    pub(super) fn new(fd: usize, parent_end: &std::fs::File) -> Option<Self> {
        let sock = parent_end.try_clone().ok()?;
        Some(Self {
            fd,
            sock: Some(OwnedFd::from(sock)),
            tx: None,
        })
    }

    /// Queue `bytes` behind the drain thread. `false` when the socket is already shut down
    /// or the thread ended on a broken pipe.
    fn write(&mut self, handle: u64, bytes: Vec<u8>) -> bool {
        if self.tx.is_none() {
            let Some(sock) = self.sock.take() else {
                return false;
            };
            let (tx, rx) = channel();
            spawn_drain(handle, self.fd, sock, rx);
            self.tx = Some(tx);
        }
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.send(Msg::Data(bytes)).is_ok())
    }

    /// Half-close the socket once everything queued is written: the child sees EOF while
    /// its replies keep arriving on the read side.
    fn shutdown(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Msg::Shutdown);
        } else if let Some(sock) = self.sock.take() {
            unsafe {
                libc::shutdown(sock.as_raw_fd(), libc::SHUT_WR);
            }
        }
    }
}

/// Drain thread owning the dup. The dup closes when the channel ends.
fn spawn_drain(handle: u64, fd: usize, sock: OwnedFd, rx: Receiver<Msg>) {
    std::thread::spawn(move || {
        let raw = sock.as_raw_fd();
        for msg in rx {
            let chunk = match msg {
                Msg::Shutdown => {
                    unsafe {
                        libc::shutdown(raw, libc::SHUT_WR);
                    }
                    break;
                }
                Msg::Data(chunk) => chunk,
            };
            let mut offset = 0;
            let mut broken = false;
            while !broken && offset < chunk.len() {
                let n = unsafe {
                    libc::write(
                        raw,
                        chunk[offset..].as_ptr() as *const libc::c_void,
                        chunk.len() - offset,
                    )
                };
                if n >= 0 {
                    offset += n as usize;
                    continue;
                }
                match std::io::Error::last_os_error().raw_os_error() {
                    Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => {
                        let mut pfd = libc::pollfd {
                            fd: raw,
                            events: libc::POLLOUT,
                            revents: 0,
                        };
                        unsafe {
                            libc::poll(&mut pfd, 1, -1);
                        }
                    }
                    Some(code) if code == libc::EINTR => {}
                    _ => broken = true,
                }
            }
            cp_push_event(CpEvent::ExtraWritten {
                handle,
                fd,
                len: chunk.len(),
                broken,
            });
            if broken {
                break;
            }
        }
    });
}

fn with_writer<R>(handle: u64, fd: usize, f: impl FnOnce(&mut ExtraWriter) -> R) -> Option<R> {
    let mut guard = cp_live_lock();
    let lc = guard.as_mut()?.get_mut(&handle)?;
    lc.extra_writers.iter_mut().find(|w| w.fd == fd).map(f)
}

fn flag(stream: f64, key: &[u8]) -> bool {
    cp_get_field(stream, key).to_bits() == TAG_TRUE_F64.to_bits()
}

fn queued_len(stream: f64) -> usize {
    cp_get_field(stream, b"writableLength") as usize
}

fn is_callable(value: f64) -> bool {
    !crate::fs::extract_closure_ptr(value).is_null()
}

fn push_callback(stream: f64, key: &[u8], callback: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let callback = scope.root_nanbox_f64(callback);
    let arr = match cp_array_ptr(cp_get_field(stream.get_nanbox_f64(), key)) {
        Some(arr) => arr,
        None => crate::array::js_array_alloc(2),
    };
    let arr = crate::array::js_array_push_f64(arr, callback.get_nanbox_f64());
    cp_set_field(stream.get_nanbox_f64(), key, cp_box_ptr(arr as *const u8));
}

fn take_callbacks(stream: f64, key: &[u8]) -> Vec<f64> {
    let Some(arr) = cp_array_ptr(cp_get_field(stream, key)) else {
        return Vec::new();
    };
    let callbacks = (0..crate::array::js_array_length(arr))
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect();
    cp_set_field(stream, key, cp_undefined());
    callbacks
}

fn call_all(callbacks: &[crate::gc::RuntimeHandle<'_>]) {
    for callback in callbacks {
        let args: [f64; 0] = [];
        unsafe {
            let _ = js_native_call_value(
                callback.get_nanbox_f64(),
                crate::closure::plain_call_receiver(),
                args.as_ptr(),
                0,
            );
        }
    }
}

/// Count `bytes` into `writableLength`, queue them, and return Node's `write()` result.
fn write_bytes(handle: u64, fd: usize, stream: f64, bytes: Vec<u8>, callback: Option<f64>) -> bool {
    let len = bytes.len();
    let queued = queued_len(stream).saturating_add(len);
    let below_high_water_mark = queued < CP_STDIN_HIGH_WATER_MARK;
    cp_set_field(stream, b"writableLength", queued as f64);
    if !below_high_water_mark {
        cp_set_field(stream, b"writableNeedDrain", TAG_TRUE_F64);
    }
    if let Some(callback) = callback {
        push_callback(stream, WRITE_CALLBACKS_KEY, callback);
    }
    if !with_writer(handle, fd, |writer| writer.write(handle, bytes)).unwrap_or(false) {
        cp_push_event(CpEvent::ExtraWritten {
            handle,
            fd,
            len,
            broken: true,
        });
    }
    below_high_water_mark
}

fn stream_target(stream: f64) -> Option<(u64, usize)> {
    let fd = cp_get_field(stream, b"__cpFd");
    if !fd.is_finite() || fd < 3.0 {
        return None;
    }
    Some((super::super::emitter::cp_handle_of(stream)?, fd as usize))
}

fn callback_arg(arg2: f64, arg3: f64) -> Option<f64> {
    [arg3, arg2].into_iter().find(|value| is_callable(*value))
}

/// `child.stdio[n].write(chunk[, encoding][, callback])` for an extra pipe.
pub(crate) extern "C" fn cp_method_extra_write(
    closure: *const ClosureHeader,
    this: JsThis,
    chunk: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    let this = cp_this(this, closure);
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = callback_arg(arg2, arg3).map(|cb| scope.root_nanbox_f64(cb));
    let this = scope.root_nanbox_f64(this);
    let Some((handle, fd)) = stream_target(this.get_nanbox_f64()) else {
        return TAG_FALSE_F64;
    };
    let accepted = write_bytes(
        handle,
        fd,
        this.get_nanbox_f64(),
        cp_value_to_bytes(chunk),
        callback.map(|cb| cb.get_nanbox_f64()),
    );
    if accepted {
        TAG_TRUE_F64
    } else {
        TAG_FALSE_F64
    }
}

/// `child.stdio[n].end([chunk][, encoding][, callback])` for an extra pipe: the child sees
/// EOF once the queued bytes are written, then the end callbacks run and `'finish'` fires.
pub(crate) extern "C" fn cp_method_extra_end(
    closure: *const ClosureHeader,
    this: JsThis,
    chunk: f64,
    arg2: f64,
    arg3: f64,
) -> f64 {
    let this = cp_this(this, closure);
    if flag(this, b"writableEnded") {
        return this;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = callback_arg(arg2, arg3)
        .or_else(|| is_callable(chunk).then_some(chunk))
        .map(|cb| scope.root_nanbox_f64(cb));
    let this = scope.root_nanbox_f64(this);
    let target = stream_target(this.get_nanbox_f64());
    let chunk_bits = chunk.to_bits();
    if let Some((handle, fd)) = target {
        if !JSValue::from_bits(chunk_bits).is_undefined() && chunk_bits != 0 && !is_callable(chunk)
        {
            let bytes = cp_value_to_bytes(chunk);
            if !bytes.is_empty() {
                write_bytes(handle, fd, this.get_nanbox_f64(), bytes, None);
            }
        }
    }
    cp_set_field(this.get_nanbox_f64(), b"writableEnded", TAG_TRUE_F64);
    cp_set_field(this.get_nanbox_f64(), b"writable", TAG_FALSE_F64);
    if let Some(callback) = callback {
        push_callback(
            this.get_nanbox_f64(),
            END_CALLBACKS_KEY,
            callback.get_nanbox_f64(),
        );
    }
    if queued_len(this.get_nanbox_f64()) == 0 {
        if let Some((handle, fd)) = target {
            with_writer(handle, fd, ExtraWriter::shutdown);
        }
        defer_finish(this.get_nanbox_f64());
    }
    this.get_nanbox_f64()
}

fn defer_finish(stream: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let deferred = scope.root_raw_mut_ptr(js_closure_alloc(
        crate::fn_info!(cp_finish_thunk, 0; with_declared(0)),
        1,
    ));
    deferred.with_mut_ptr(|deferred: *mut ClosureHeader| {
        js_closure_set_capture_ptr(deferred, 0, stream.get_nanbox_f64().to_bits() as i64);
    });
    deferred.with_mut_ptr(|deferred: *mut ClosureHeader| {
        crate::timer::js_set_immediate_callback(deferred as i64);
    });
}

extern "C" fn cp_finish_thunk(closure: *const ClosureHeader, _this: JsThis) -> f64 {
    finish(f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64));
    cp_undefined()
}

/// End callbacks run before `'finish'`, as in Node.
fn finish(stream: f64) {
    if flag(stream, b"writableFinished") {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    cp_set_field(stream.get_nanbox_f64(), b"writableFinished", TAG_TRUE_F64);
    let callbacks =
        scope.root_nanbox_f64_slice(&take_callbacks(stream.get_nanbox_f64(), END_CALLBACKS_KEY));
    call_all(&callbacks);
    cp_emit(stream.get_nanbox_f64(), "finish", &[]);
}

/// Pump-side completion of `len` queued bytes: once the queue is empty emit `'drain'`, run
/// the write callbacks in order, and complete an `end()` that was waiting on the queue.
pub(super) fn cp_extra_written(handle: u64, fd: usize, len: usize, broken: bool) {
    let Some(cp_bits) = cp_lookup_cp_bits(handle) else {
        return;
    };
    let stream = cp_stdio_stream(f64::from_bits(cp_bits), fd);
    if cp_object_ptr(stream).is_none() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let queued = if broken {
        0
    } else {
        queued_len(stream.get_nanbox_f64()).saturating_sub(len)
    };
    cp_set_field(stream.get_nanbox_f64(), b"writableLength", queued as f64);
    if queued > 0 {
        return;
    }
    let need_drain = flag(stream.get_nanbox_f64(), b"writableNeedDrain");
    cp_set_field(stream.get_nanbox_f64(), b"writableNeedDrain", TAG_FALSE_F64);
    let ended = flag(stream.get_nanbox_f64(), b"writableEnded");
    let callbacks = scope.root_nanbox_f64_slice(&take_callbacks(
        stream.get_nanbox_f64(),
        WRITE_CALLBACKS_KEY,
    ));
    if need_drain && !ended && !broken {
        cp_emit(stream.get_nanbox_f64(), "drain", &[]);
    }
    call_all(&callbacks);
    if ended {
        with_writer(handle, fd, ExtraWriter::shutdown);
        finish(stream.get_nanbox_f64());
    }
}
