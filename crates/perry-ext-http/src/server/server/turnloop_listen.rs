//! The turnloop half of `http.Server` (P5): the listen decision, Node's idle
//! close arithmetic, and the two queues the connection layer feeds.
//!
//! Split out of `server.rs` so that file stays under the repository's
//! 2000-line-per-file lint cap.

use perry_ffi::{get_handle, get_handle_mut};

use super::{HttpPendingUpgrade, HttpServer, PENDING_CONNECTION_EVENTS, TURNLOOP_UPGRADES};

/// Fire Node's `'aborted'` on every request whose connection died before its
/// response completed (P5), and report how many listeners ran.
///
/// The completion sink queues the `IncomingMessage` handle rather than firing
/// there: it runs inside `dispatch_staged`, after a turn, and must not run JS.
/// This is the same tick every other server event is dispatched on.
pub(crate) fn drain_aborted_requests() -> i32 {
    let mut fired = 0;
    for request_handle in crate::server::turnloop_serve::take_aborted() {
        let listeners = get_handle_mut::<crate::server::request::IncomingMessage>(request_handle)
            .map(|im| {
                im.aborted = true;
                im.listeners.get("aborted").cloned().unwrap_or_default()
            })
            .unwrap_or_default();
        if !listeners.is_empty() {
            crate::server::request::emit_no_arg_to_listeners(&listeners);
            fired += 1;
        }
    }
    fired
}

/// An HTTP/2 stream died before its response was written: queue Node's
/// `'aborted'` on the request, through the same queue P5's own aborted
/// requests use and the same `drain_aborted_requests` tick.
pub(crate) fn note_turnloop_request_aborted(request_handle: i64) {
    crate::server::turnloop_serve::note_aborted_handle(request_handle);
}

/// Queue the `'connection'` event for a turnloop-accepted connection (P5).
///
/// Drained by the pump, whose listeners fire with no args.
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
/// on it can never flush, so they are reaped like any dropped connection's.
pub(crate) fn turnloop_connection_closed(_conn_id: i64) {}

/// Bind and accept on the agent's turnloop loop, when this thread has one
/// (P5). Returns the listener id — `Some(0)` when the bind failed and the
/// failure was queued as the server's `'error'` — or `None` when this thread
/// has no loop, in which case the caller posts the listen to the thread that
/// owns it (`turnloop_serve::post_to_owner`).
///
/// A cluster worker binds here too. turnloop 0.1.0-alpha.6 split
/// `ListenOpts::reuse_port` into `ReusePort::{No, Share, Distribute}` and
/// `perry-runtime`'s `listen_opts` maps Perry's `true` to `Share`, which is
/// the `SO_REUSEPORT` bind a SCHED_NONE worker (and a SCHED_RR worker whose
/// primary did not answer) always did. **`Distribute` is deliberately not
/// used**: it is the kernel-balanced variant, it is `Unsupported` on macOS and
/// on every BSD but FreeBSD, and Perry's cluster has never had it. A SCHED_RR
/// worker whose primary answered never reaches this: the primary owns that
/// socket and passes accepted descriptors, which `ListenPlan::start_rr_inject`
/// adopts onto the loop.
///
/// `resolved` is the port the cluster primary handed back for a shared
/// `listen(0)`; it is bound in place of the requested one.
pub(super) fn try_listen_on_turnloop(
    server_handle: i64,
    host: &str,
    port: u16,
    resolved: Option<u16>,
) -> Option<i64> {
    if !crate::server::turnloop_serve::enabled() {
        return None;
    }
    let (reuse_port, bind_port) = listen_plan(
        crate::server::cluster_bind::is_cluster_worker(),
        resolved,
        port,
    );
    let (no_delay, idle_close_ms) = {
        let server = get_handle::<HttpServer>(server_handle)?;
        (server.no_delay, idle_close_ms(server))
    };
    match crate::server::turnloop_serve::listen(
        server_handle,
        host,
        bind_port,
        511,
        None,
        reuse_port,
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
            // Node emits `'error'` on the server object, asynchronously, with
            // `code` / `errno` / `syscall` / `address` / `port`. This used to
            // `eprintln!` and return, so a failed bind was invisible to the
            // program: `server.on('error', ...)` never fired and a script that
            // waited on it hung. Reported once, to JS, where Node reports it.
            super::queue_listen_error_parts(
                server_handle,
                host,
                bind_port,
                &err.code,
                err.errno,
                &err.syscall,
            );
            // The failure is reported once and the listen ends here.
            Some(0)
        }
    }
}

/// Whether this listener shares its port, and which port it binds.
///
/// A pure function of the three things the caller knows, for the reason
/// `perry-runtime`'s `listen_opts` is one: the mapping from an input to a
/// listener option is exactly what went wrong the last time this argument moved
/// (`no_delay` landed in the `reuse_port` position and every HTTP listener
/// silently shared its port), and nothing between the caller and the kernel
/// could observe it. Here it is observable without a loop.
///
/// * **`reuse_port` is keyed on being a cluster worker, not on `resolved`.** A
///   worker whose `worker_query_listen` timed out has `resolved == None` and
///   still has to bind with `SO_REUSEPORT`, as it always has.
/// * **`resolved` wins the port** when the primary handed one back, which is
///   how N workers share one ephemeral port for `listen(0)` (#4962).
pub(super) fn listen_plan(
    is_cluster_worker: bool,
    resolved: Option<u16>,
    port: u16,
) -> (bool, u16) {
    (is_cluster_worker, resolved.unwrap_or(port))
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

#[cfg(test)]
mod tests {
    use super::listen_plan;
    use crate::server::server::HttpServer;
    use perry_ffi::{drop_handle, get_handle, register_handle};

    /// `http.Server.listen()` must reach the agent's turnloop loop on whichever
    /// thread asks, and there is no hyper accept loop to fall back to any more.
    /// The three outcomes are the three routes:
    ///
    /// * this thread owns the loop — the bind happens here, synchronously, so
    ///   the server is listening on a real port before `listen()` returns;
    /// * another thread owns it — the bind is posted to that owner, so nothing
    ///   is bound or reported on this thread yet;
    /// * no loop exists for the agent — the listen fails with Node's
    ///   `'error'`, code `ENOTSUP`, and never reports `'listening'`.
    ///
    /// **The route is observed, not assumed**: an agent's route is claimed once
    /// per thread by the first thread to ask, so which harness thread this
    /// lands on decides it. Every arm asserts something; none is a skip.
    #[test]
    fn listen_reaches_the_loop_on_every_route() {
        let owns_loop = crate::server::turnloop_serve::enabled();
        let can_post = perry_ffi::agent_post::available();
        let handle = register_handle(HttpServer::with_handler(0));
        let args = crate::server::types::ListenArgs {
            opts: 0.0,
            host: Some("127.0.0.1".to_string()),
            callback: 0,
        };
        unsafe { super::super::listen_http_server(handle, args) };

        let (listening, bound_port, listening_emit, error_code) = {
            let s = get_handle::<HttpServer>(handle).expect("server handle");
            (
                s.listening,
                s.bound_port,
                s.pending_listening_emit,
                s.pending_error_emit.as_ref().map(|e| e.code.clone()),
            )
        };
        let listener = crate::server::turnloop_serve::listener_for_server(handle);
        if let Some(id) = listener {
            crate::server::turnloop_serve::close_listener(id);
        }
        drop_handle(handle);

        if owns_loop {
            assert!(listening, "an owned loop must bind synchronously");
            assert_ne!(bound_port, 0, "port 0 must report the kernel's port");
            assert!(listener.is_some(), "the bind must be a turnloop listener");
            assert!(listening_emit, "a bound server owes a 'listening' emit");
            assert_eq!(error_code, None);
        } else if can_post {
            assert!(!listening, "a posted bind has not run on this thread");
            assert!(listener.is_none());
            assert!(!listening_emit);
            assert_eq!(error_code, None);
        } else {
            assert!(!listening);
            assert!(!listening_emit, "a failed listen never emits 'listening'");
            assert_eq!(error_code.as_deref(), Some("ENOTSUP"));
        }
    }

    /// An ordinary server must never share its port: a second `listen()` on it
    /// is Node's `EADDRINUSE`, and the last time this argument drifted every
    /// HTTP and HTTPS listener bound with `SO_REUSEPORT` and the second one
    /// quietly succeeded.
    #[test]
    fn an_ordinary_server_binds_its_own_port_exclusively() {
        assert_eq!(listen_plan(false, None, 8080), (false, 8080));
    }

    /// A cluster worker shares, and this is the half of the recorded blocker
    /// that read "SO_REUSEPORT is what the cluster worker still waits on".
    #[test]
    fn a_cluster_worker_shares_the_port_the_primary_resolved() {
        assert_eq!(listen_plan(true, Some(54321), 0), (true, 54321));
    }

    /// The decline this replaces was keyed on `resolved.is_some()`, which would
    /// send a worker whose primary did not answer down the exclusive-bind path
    /// — an `EADDRINUSE` against its sibling workers rather than a shared port.
    #[test]
    fn a_worker_whose_primary_did_not_answer_still_shares() {
        assert_eq!(listen_plan(true, None, 8080), (true, 8080));
    }

    /// A non-worker never acquires a resolved port, but if one ever reached
    /// here it must not silently turn an exclusive bind into a shared one.
    #[test]
    fn a_resolved_port_does_not_by_itself_enable_sharing() {
        assert_eq!(listen_plan(false, Some(54321), 0), (false, 54321));
    }
}
