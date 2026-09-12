//! Nonblocking child stdin and asynchronous drain completion.

use super::*;

/// Node's default `writableHighWaterMark` for a child's stdin socket.
pub(crate) const CP_STDIN_HIGH_WATER_MARK: usize = 64 * 1024;

/// #9493: the writable side of a live child's stdin.
///
/// `stdin.write()` used to `write_all` inline: it parked the main thread on a
/// full pipe until the child read (an LSP that stops reading hangs the
/// program), always returned `true`, never emitted `'drain'`, and committed
/// every byte before a `process.exit()` in the same tick. Node â€” libuv's
/// `uv_try_write` â€” commits what the pipe accepts right now, queues the
/// remainder for the loop, and judges the return value against the queued
/// length. This is that shape: the synchronous try-write is on the main
/// thread (bytes below pipe capacity land exactly as they did, including at
/// `process.exit()`), the remainder goes to a drain thread, and completion
/// (callbacks, `'drain'`, the deferred close for `end()`) is reported back
/// through the event queue like every other child event.
pub(super) struct CpStdin {
    /// Owns the pipe end; dropping it is the EOF the child sees.
    writer: Option<CpWriter>,
    /// Raw descriptor the drain thread dups. Unix only.
    #[cfg(unix)]
    fd: i32,
    /// Bytes handed to the drain thread and not yet reported written â€”
    /// `writableLength`.
    pub(super) queued: usize,
    /// A `write()` returned `false` and no `'drain'` has fired since.
    pub(super) need_drain: bool,
    /// `end()` ran while bytes were queued: close once they are written.
    pub(super) end_pending: bool,
    /// Write/end callbacks that fire, in order, once the queue drains
    /// (NaN-boxed closures; rooted by `cp_reactor_scan_roots_mut`).
    pub(super) callbacks: Vec<u64>,
    /// Sender to the lazily-started drain thread.
    tx: Option<std::sync::mpsc::Sender<Vec<u8>>>,
}

impl CpStdin {
    #[cfg(unix)]
    pub(super) fn new(writer: CpWriter, fd: i32) -> Self {
        // `O_NONBLOCK` is a property of this open file description alone â€”
        // the child's read end is a separate description â€” so the try-write
        // reports `WouldBlock` instead of parking the main thread.
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            if flags >= 0 {
                let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
        Self {
            writer: Some(writer),
            fd,
            queued: 0,
            need_drain: false,
            end_pending: false,
            callbacks: Vec::new(),
            tx: None,
        }
    }

    #[cfg(not(unix))]
    pub(super) fn new(writer: CpWriter) -> Self {
        Self {
            writer: Some(writer),
            queued: 0,
            need_drain: false,
            end_pending: false,
            callbacks: Vec::new(),
            tx: None,
        }
    }

    /// libuv's `uv__try_write`: write until the pipe would block. Returns how
    /// many bytes were committed. A broken pipe counts the whole chunk as
    /// consumed â€” `SIGPIPE` is ignored process-wide (#9402), the reader is
    /// gone, and there is nobody left to deliver to.
    #[cfg(unix)]
    pub(super) fn try_write(&mut self, bytes: &[u8]) -> usize {
        let mut offset = 0;
        while offset < bytes.len() {
            match self.writer.as_mut().unwrap().write(&bytes[offset..]) {
                Ok(0) => break,
                Ok(n) => offset += n,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return bytes.len(),
            }
        }
        offset
    }

    #[cfg(unix)]
    pub(super) fn enqueue(&mut self, handle: u64, bytes: Vec<u8>) {
        if self.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            cp_spawn_stdin_drain(handle, self.fd, rx);
            self.tx = Some(tx);
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(bytes);
        }
    }

    // Windows anonymous pipes are synchronous. The worker owns the write
    // handle; the main thread only queues bytes, so timers and abort handlers
    // can run even when an LSP has stopped consuming its stdin.
    #[cfg(not(unix))]
    pub(super) fn try_write(&mut self, _bytes: &[u8]) -> usize {
        0
    }

    #[cfg(not(unix))]
    pub(super) fn enqueue(&mut self, handle: u64, bytes: Vec<u8>) {
        if self.tx.is_none() {
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            let mut writer = self
                .writer
                .take()
                .expect("stdin worker owns the writer once");
            std::thread::spawn(move || {
                for chunk in rx {
                    let broken = writer.write_all(&chunk).is_err();
                    cp_push_event(CpEvent::StdinWritten {
                        handle,
                        len: chunk.len(),
                        broken,
                    });
                    if broken {
                        break;
                    }
                }
            });
            self.tx = Some(tx);
        }
        if let Some(tx) = &self.tx {
            let len = bytes.len();
            if tx.send(bytes).is_err() {
                cp_push_event(CpEvent::StdinWritten {
                    handle,
                    len,
                    broken: true,
                });
            }
        }
    }
}

/// Drain thread for the bytes the pipe would not take synchronously. It
/// owns a `dup` of the descriptor, so the registry's writer can be dropped
/// (`end()`, child close, teardown) without pulling the fd out from under
/// an in-flight write; the dup closes when the channel ends, which is what
/// finally delivers EOF after an `end()` on a backed-up pipe.
#[cfg(unix)]
fn cp_spawn_stdin_drain(handle: u64, fd: i32, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    let dup = unsafe { libc::dup(fd) };
    std::thread::spawn(move || {
        let mut broken = dup < 0;
        for chunk in rx {
            let mut offset = 0;
            while !broken && offset < chunk.len() {
                let n = unsafe {
                    libc::write(
                        dup,
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
                            fd: dup,
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
            cp_push_event(CpEvent::StdinWritten {
                handle,
                len: chunk.len(),
                broken,
            });
            if broken {
                break;
            }
        }
        if dup >= 0 {
            unsafe {
                libc::close(dup);
            }
        }
    });
}
