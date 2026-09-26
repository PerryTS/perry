//! Local IPC transport for `node:net` path overloads.
//!
//! Node maps `server.listen(path)` and `net.connect({ path })` to named pipes
//! on Windows and Unix-domain sockets on Unix. Both run on the agent's
//! turnloop loop (`turnloop_io::listen_pipe` / `connect_pipe`), so data, end,
//! error, close, and server connection events share one implementation with
//! TCP.

use crate::{
    dispatch, ensure_gc_scanner_registered, mark_closed, next_id_or_throw, push_event,
    server_state, statics, PendingNetEvent, SocketState, TlsSocketMetadata,
};

fn allocate_socket() -> i64 {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    let id = next_id_or_throw();
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id: 0,
            owner_agent: perry_ffi::agent_post::current_agent(),
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect: false,
            unconnected_write_failed: false,
            is_open: false,
            raw_fd: None,
            refed: true,
            local_addr: None,
            remote_addr: None,
            raw: None,
            destroyed: false,
            connecting: true,
            has_opened: false,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            need_drain: false,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
            turnloop: false,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, Default::default());
    id
}

/// Read a JS string without coercing closures or option objects through a
/// StringHeader layout.
pub(crate) unsafe fn string_value(value: f64) -> Option<String> {
    crate::jsvalue_to_owned_string(value)
}

pub(crate) fn register_connect_cb(handle: i64, cb_f64: f64) {
    if handle == 0 || !crate::is_nanboxed_pointer(cb_f64) {
        return;
    }
    let cb_ptr = unsafe { crate::unbox_pointer(cb_f64) } as i64;
    if cb_ptr == 0 {
        return;
    }
    statics::listeners()
        .lock()
        .unwrap()
        .entry(handle)
        .or_default()
        .entry("connect".to_string())
        .or_default()
        .push(cb_ptr);
}

pub(crate) fn spawn_socket(path: String) -> i64 {
    let id = allocate_socket();
    spawn_connect(id, path);
    id
}

pub(crate) fn connect_existing(handle: i64, path: String) {
    {
        let mut sockets = statics::sockets().lock().unwrap();
        match sockets.get_mut(&handle) {
            Some(socket) if socket.awaiting_connect => {
                socket.awaiting_connect = false;
                // #10465 — `socket.connect(path)` on a `new net.Socket()`
                // starts connecting synchronously, same as the TCP path.
                socket.connecting = true;
                if socket.unconnected_write_failed {
                    return;
                }
            }
            _ => {
                push_event(PendingNetEvent::Error(
                    handle,
                    "socket already connected (or unknown handle)".to_string(),
                ));
                return;
            }
        }
    }
    spawn_connect(handle, path);
}

fn spawn_connect(id: i64, path: String) {
    let local_server = server_state::begin_local_path_connect(&path);
    // A local socket connects on the loop. It can never be TLS-upgraded
    // (`upgradeToTLS` reports "unsupported for IPC sockets").
    set_turnloop(id, true);
    let target = path.clone();
    let submitted = crate::turnloop_io::on_loop(move || {
        crate::turnloop_io::note_local_connect(id, local_server);
        if let Err(error) = crate::turnloop_io::connect_pipe(id, &path) {
            refuse_connect(id, &error.message(), &path, local_server);
        }
    });
    if !submitted {
        refuse_connect(
            id,
            &format!("connect {}", crate::turnloop_io::NO_LOOP_CODE),
            &target,
            local_server,
        );
    }
}

/// libuv's shape (`connect ENOENT /tmp/x.sock`), which is what
/// `build_error_object` parses into code/errno/syscall.
fn refuse_connect(id: i64, message: &str, path: &str, local_server: Option<(i64, bool)>) {
    set_turnloop(id, false);
    server_state::cancel_local_connect(local_server);
    push_event(PendingNetEvent::Error(id, format!("{message} {path}")));
    push_event(PendingNetEvent::Close(id));
    mark_closed(id);
}

fn set_turnloop(id: i64, on_loop: bool) {
    if let Ok(mut sockets) = statics::sockets().lock() {
        if let Some(socket) = sockets.get_mut(&id) {
            socket.turnloop = on_loop;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nanboxed_string_is_recognized_as_an_ipc_path() {
        let path = std::env::temp_dir()
            .join(format!("perry-ext-net-{}-1.sock", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let header = perry_ffi::alloc_string(&path).as_raw();
        let value = f64::from_bits(perry_ffi::nanbox_string_bits(header));
        assert_eq!(unsafe { super::string_value(value) }, Some(path));
    }
}
