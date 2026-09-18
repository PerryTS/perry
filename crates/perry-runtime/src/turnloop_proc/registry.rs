//! Who a completion belongs to, and what it looks like when it gets there.
//!
//! P1 needed a registry of `extern "C"` sink pointers because its subsystem
//! was a separately linked `staticlib`. Every P2 subsystem is compiled into
//! this crate, so the equivalent is an enum and a `match`: no function
//! pointers, no process-global slots, no ABI layout digest, and the compiler
//! checks that every owner handles every event it can receive.
//!
//! [`Owner`] is `Copy` and holds only the subsystem's own key — never a JS
//! value and never a pointer — so the entry that carries it stays outside the
//! GC's concern, exactly as `turnloop_net::Entry` does.

use std::net::SocketAddr;

use super::NodeError;

/// Which subsystem submitted an operation, and the key it knows it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    /// A `node:dgram` socket, keyed by `dgram_reactor`'s own socket id.
    #[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
    Dgram { socket: u64 },
    /// A process-wide OS signal subscription, keyed by signal number.
    ProcessSignal { signum: i32 },
    /// A child's readable pipe: stdout (`fd == 1`), stderr (`fd == 2`), or an
    /// extra `stdio` descriptor.
    ChildStream { child: u64, fd: usize },
    /// The acceptance tests' own owner. It exists so the tests exercise the
    /// real registry, the real token space and the real dispatch path rather
    /// than a parallel mock of them.
    #[cfg(test)]
    Test,
}

/// One completion, in the shape the owning subsystem consumes.
///
/// `Data` and `Datagram` own their bytes: they were copied out of turnloop's
/// pooled lease inside `dispatch`, before the lease went back to the pool, so
/// nothing here borrows driver memory and the owner may hold it across a
/// collection.
#[derive(Debug)]
pub(crate) enum StreamEvent {
    /// Bytes arrived on a stream.
    Data(Vec<u8>),
    /// One datagram arrived, with its source endpoint.
    #[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
    Datagram { bytes: Vec<u8>, from: SocketAddr },
    /// The peer closed its write side.
    Eof,
    /// A queued datagram reached the OS. `user` echoes the caller's token.
    #[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
    Wrote { user: u64, len: usize },
    /// A subscribed signal was delivered to this agent. Which signal it was is
    /// already in the [`Owner`], so the payload would only be a second copy.
    Signal,
    /// The handle's final completion; no further event can name this id.
    Closed,
    /// An operation failed. `terminal` distinguishes a transient failure of a
    /// multishot operation from one that ended it; `user` echoes the write or
    /// send token whose failure this is, and is zero for a read-side failure —
    /// which is what lets a caller tell "this datagram could not be sent" from
    /// "this socket errored", two different Node reporting shapes.
    #[cfg_attr(not(feature = "mod-dgram"), allow(dead_code))]
    Error {
        user: u64,
        error: NodeError,
        terminal: bool,
    },
}

/// Route one translated completion to its subsystem.
///
/// Runs on the loop-owning thread, inside `agent_loop::dispatch_staged`, with
/// no borrow held on the driver or on this module's entry table — so a handler
/// may run JS, allocate, collect and submit new work.
pub(crate) fn deliver(owner: Owner, id: u64, event: StreamEvent) {
    match owner {
        #[cfg(feature = "mod-dgram")]
        Owner::Dgram { socket } => crate::dgram_reactor::on_completion(socket, event),
        #[cfg(not(feature = "mod-dgram"))]
        Owner::Dgram { .. } => {
            let _ = event;
        }
        #[cfg(unix)]
        Owner::ProcessSignal { signum } => crate::os::signal::on_signal_completion(signum, event),
        #[cfg(not(unix))]
        Owner::ProcessSignal { .. } => {
            let _ = event;
        }
        Owner::ChildStream { child, fd } => {
            crate::child_process::reactor::on_stream_completion(child, fd, event)
        }
        #[cfg(test)]
        Owner::Test => super::tests::record(id, event),
    }
    let _ = id;
}
