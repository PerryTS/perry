//! The turnloop half of `http.Server` (P5): the listen decision, Node's idle
//! close arithmetic, and the two queues the connection layer feeds.
//!
//! Split out of `server.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap.

use perry_ffi::{get_handle, get_handle_mut};

use super::{HttpPendingUpgrade, HttpServer, PENDING_CONNECTION_EVENTS, TURNLOOP_UPGRADES};

/// Queue the `'connection'` event for a turnloop-accepted connection (P5).
///
/// Shares `PENDING_CONNECTION_EVENTS` with the hyper accept loop, so the
/// pump's drain and Node's "listeners fire with no args" shape are unchanged.
pub(crate) fn queue_turnloop_connection_event(server_handle: i64) {
    if let Ok(mut q) = PENDING_CONNECTION_EVENTS.lock() {
        q.push(server_handle);
    }
}

/// Queue a turnloop `'upgrade'` for the main-thread pump (P5).
pub(crate) fn queue_turnloop_upgrade(pending: HttpPendingUpgrade) {
    if let Ok(mut q) = TURNLOOP_UPGRADES.lock() {
        q.push_back(pending);
    }
}

/// A turnloop connection reached its terminal `Closed` (P5). Parked requests
/// on it can never flush, so they are reaped exactly as a dropped hyper
/// connection's are.
pub(crate) fn turnloop_connection_closed(_conn_id: i64) {}

/// Bind and accept on the agent's turnloop loop, when this thread has one
/// (P5). Returns the listener id, or `None` when the caller must keep the
/// hyper path.
///
/// Three reasons to decline, each a real hole rather than a preference:
///
/// * **No loop.** A `worker_threads` agent has none before P3/P4, exactly as
///   P1's net transport declines there. This is why the hyper accept loop is
///   narrowed rather than deleted.
/// * **A cluster worker.** SCHED_RR fd passing and the SO_REUSEPORT bind both
///   need the `std::net::TcpListener` the hyper path builds; turnloop's
///   `ListenOpts` exposes no `reuse_port` through Perry's binding yet.
/// * **An attached `WebSocketServer`.** Its handshake is completed by
///   `tokio_tungstenite` over an owned stream, which a turnloop connection
///   cannot produce; a `server.on('upgrade')` listener needs no such thing and
///   is served on turnloop through `turnloop_net::transfer`.
pub(super) fn try_listen_on_turnloop(
    server_handle: i64,
    host: &str,
    port: u16,
    resolved: Option<u16>,
) -> Option<i64> {
    if resolved.is_some() || crate::server::cluster_bind::is_cluster_worker() {
        return None;
    }
    if perry_ext_ws::has_attached_server(server_handle) {
        return None;
    }
    if !crate::server::turnloop_serve::enabled() {
        return None;
    }
    let (no_delay, idle_close_ms) = {
        let server = get_handle::<HttpServer>(server_handle)?;
        (server.no_delay, idle_close_ms(server))
    };
    match crate::server::turnloop_serve::listen(
        server_handle,
        host,
        port,
        511,
        None,
        no_delay,
        idle_close_ms,
    ) {
        Ok((id, bound_port, _bound_host)) => {
            crate::server::cluster_bind::notify_listening(host, bound_port);
            let server = get_handle_mut::<HttpServer>(server_handle)?;
            server.bound_port = bound_port;
            server.bound_host = host.to_string();
            server.listening = true;
            Some(id)
        }
        Err(err) if err.no_loop => None,
        Err(err) => {
            eprintln!(
                "[node:http] bind {}:{} failed: {}",
                host,
                port,
                err.message()
            );
            // Returning the id-less `Some` would be a lie; the hyper path
            // would then bind the same address and fail the same way, so the
            // failure is reported once and the listen ends here.
            Some(0)
        }
    }
}

/// Node's idle close for a keep-alive connection: `keepAliveTimeout +
/// keepAliveTimeoutBuffer`, in ms, with **zero meaning never**.
///
/// Measured on Node 26.5.1: `keepAliveTimeout = 300` FINs at 1303 ms and
/// `= 1000` at 2002 ms with the default 1000 ms buffer, while `= 0` never
/// closes at all (still open after 2 s) and simply omits the `Keep-Alive`
/// header. See `ServerResponse::apply_default_connection_headers_for`.
pub(crate) fn idle_close_ms(server: &HttpServer) -> u64 {
    if !(server.keep_alive_timeout > 0.0) {
        return 0;
    }
    let buffer = if server.keep_alive_timeout_buffer > 0.0 {
        server.keep_alive_timeout_buffer
    } else {
        0.0
    };
    (server.keep_alive_timeout + buffer).max(0.0) as u64
}
