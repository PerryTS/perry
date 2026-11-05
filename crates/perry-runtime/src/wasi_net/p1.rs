//! The `wasm32-wasip1` embedder's signal stream. The event-loop park
//! (`super::park`, one `poll_oneoff` on preview 1) waits on it beside the
//! sockets and stdin.

extern "C" {
    fn perry_wasi_signal_fd() -> i32;
    fn perry_wasi_signal_retire();
    fn perry_wasi_stream_read(fd: i32, bytes: *mut u8, len: u32) -> i32;
}

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

/// The embedder's signal stream while it is open; the park waits on it.
pub(crate) fn signal_fd() -> Option<i32> {
    let fd = unsafe { perry_wasi_signal_fd() };
    (fd >= 0).then_some(fd)
}
