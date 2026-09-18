//! turnloop P0: O(1) TLS keep-alive.
//!
//! `js_tls_has_active_handles` runs on every event-loop turn once the stdlib
//! pump is registered. It used to walk every TLS server and socket under their
//! locks. `KEEPALIVE` now counts exactly the records that walk would have found:
//! servers that are listening or draining connections after `close()`, and
//! server-side sockets with a live command channel. Every state change a record
//! makes is bracketed by `keeps_alive` before and after, under that record's
//! map lock, and every removal releases what the record held.

use super::{TlsServerState, TlsSocketState};
use std::sync::atomic::{AtomicUsize, Ordering};

static KEEPALIVE: AtomicUsize = AtomicUsize::new(0);

pub(super) fn server_keeps_alive(server: &TlsServerState) -> bool {
    server.listening || (server.closing && server.active_connections > 0)
}

pub(super) fn socket_keeps_alive(socket: &TlsSocketState) -> bool {
    socket.server_side && socket.cmd_tx.is_some()
}

/// Apply one record's transition from `before` to `after`.
pub(super) fn step(before: bool, after: bool) {
    match (before, after) {
        (false, true) => {
            KEEPALIVE.fetch_add(1, Ordering::AcqRel);
        }
        (true, false) => {
            let previous = KEEPALIVE.fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "TLS keep-alive count underflow");
        }
        _ => {}
    }
}

/// Mutate a server record, keeping the count in step. Call with the lock held.
pub(super) fn update_server<R>(
    server: &mut TlsServerState,
    change: impl FnOnce(&mut TlsServerState) -> R,
) -> R {
    let before = server_keeps_alive(server);
    let result = change(server);
    step(before, server_keeps_alive(server));
    result
}

pub(super) fn any() -> bool {
    KEEPALIVE.load(Ordering::Acquire) != 0
}

#[cfg(debug_assertions)]
pub(super) fn debug_verify() {
    let servers = super::servers()
        .lock()
        .unwrap()
        .values()
        .filter(|s| server_keeps_alive(s))
        .count();
    let sockets = super::sockets()
        .lock()
        .unwrap()
        .values()
        .filter(|s| socket_keeps_alive(s))
        .count();
    debug_assert_eq!(
        KEEPALIVE.load(Ordering::Acquire),
        servers + sockets,
        "TLS keep-alive count drifted from its registries"
    );
}

#[cfg(test)]
pub(super) fn count_for_test() -> usize {
    KEEPALIVE.load(Ordering::Acquire)
}
