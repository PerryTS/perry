use crate::{BufLease, Error, Handle, OpId, Token};
use std::{any::Any, net::SocketAddr, time::Duration};

/// Common small posts need no box/allocation. Larger host results can own bytes or
/// a Send Rust value; the driver never invokes a function from this payload.
#[derive(Debug)]
pub enum Payload {
    /// An inline integer payload with no heap allocation.
    U64(u64),
    /// An owned byte payload transferred to the destination loop.
    Bytes(Vec<u8>),
    /// An owned, type-erased Send host value.
    Boxed(Box<dyn Any + Send>),
}
/// Result returned by a host blocking job.
pub type BlockingResult = crate::Result<Payload>;

#[derive(Debug)]
/// One host-visible operation result, carrying its original routing token.
pub struct Completion {
    /// Opaque host routing token preserved from submission.
    pub token: Token,
    /// None for unsolicited posts and the final Closed notification.
    pub op: Option<OpId>,
    /// Resource identity belonging to the submitting or receiving loop.
    pub handle: Option<Handle>,
    /// True once this operation will produce no further results or native buffer accesses.
    pub terminal: bool,
    /// The operation outcome or its original error.
    pub result: OpResult,
}
#[derive(Debug)]
/// Host-visible I/O, timer, process, service and lifecycle results.
pub enum OpResult {
    /// A typed filesystem result.
    Fs(crate::FsResult),
    /// Nonterminal batch of filesystem watch records; parse with `WatchEvents`.
    Watch {
        /// Initialized record bytes, in native event order.
        events: BufLease,
        /// Events were lost before this batch; rescan the watched scope.
        overflow: bool,
    },
    /// Completion of a registered host wait condition.
    ExternalWait(crate::WaitResult),
    /// Child exit, after reaping; produced exactly once per accepted spawn.
    Exited(crate::ExitStatus),
    /// Process-wide signal delivered to this subscribed loop.
    Signal(crate::Signal),
    /// Accepted local stream, already attached to this loop.
    PipeAccepted {
        /// Accepted connection handle, already registered on this loop.
        conn: Handle,
    },
    /// Received transport, already attached to this loop.
    HandleReceived {
        /// Resource identity belonging to the submitting or receiving loop.
        handle: Handle,
    },
    /// One transport was passed to the receiving process.
    HandleSent,
    /// The connection is established and ready for stream operations.
    Connected,
    /// An incoming TCP connection with its peer address.
    Accepted {
        /// Accepted connection handle, already registered on this loop.
        conn: Handle,
        /// Remote TCP peer endpoint.
        peer: SocketAddr,
    },
    /// Read bytes, with a byte count and optional pooled lease in the completion.
    Read {
        /// Number of bytes transferred by this completion.
        n: usize,
        /// Owned pooled read bytes, present only when a pooled buffer was used.
        lease: Option<BufLease>,
    },
    /// The peer or file reached the end of its readable stream.
    Eof,
    /// Number of bytes successfully written.
    Wrote(usize),
    /// Receive one UDP datagram and report its source address.
    RecvFrom {
        /// Number of bytes transferred by this completion.
        n: usize,
        /// Source endpoint of the received datagram.
        from: SocketAddr,
        /// Owned pooled read bytes, present only when a pooled buffer was used.
        lease: Option<BufLease>,
    },
    /// Complete a stream write-side shutdown after preceding queued writes.
    Shutdown,
    /// A timer reached its deadline; repeating timers produce nonterminal ticks.
    Timer,
    /// A payload posted by another thread to this loop.
    Posted(Payload),
    /// A successful host blocking-job result.
    Blocking(Payload),
    /// Resolved IP endpoints returned by the native name resolver.
    Resolved(Vec<SocketAddr>),
    /// The operation was cancelled and its native buffer access has ended.
    Cancelled,
    /// Final handle completion, following all of its cancelled operations.
    Closed,
    /// Terminal completion of an explicitly stopped multishot operation.
    Stopped,
    /// An accepted operation failed with this error.
    Err(Error),
}
/// Fixed-capacity output. Each turn clears old completions and fills at most this
/// capacity. Retained completions remain in the driver, providing backpressure.
#[derive(Debug)]
pub struct Completions {
    pub(crate) entries: Vec<Completion>,
}
impl Completions {
    /// Allocate fixed completion storage; capacity must be nonzero.
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0);
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }
    /// Return the number of current completions.
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Whether no completions are currently stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Return the fixed maximum number of completions held by this buffer.
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }
    /// Iterate over the completions from the most recent turn.
    pub fn iter(&self) -> std::slice::Iter<'_, Completion> {
        self.entries.iter()
    }
    /// Move all completions out while retaining their allocated storage.
    pub fn drain(&mut self) -> std::vec::Drain<'_, Completion> {
        self.entries.drain(..)
    }
    /// Drop current completions and their leases while retaining output capacity.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}
impl Default for Completions {
    fn default() -> Self {
        Self::with_capacity(256)
    }
}
impl std::ops::Deref for Completions {
    type Target = [Completion];
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
#[derive(Clone, Copy, Debug)]
/// Completed turn statistics and current reference-counted liveness.
pub struct TurnInfo {
    /// Number of completions appended to the host output buffer.
    pub completions: usize,
    /// Total monotonic elapsed time inside this turn.
    pub waited: Duration,
    /// Whether any referenced handle, operation or queued terminal result remains.
    pub alive: bool,
    /// Blocking native waits during this turn (positive timeout or infinite).
    pub os_waits: u32,
    /// Zero-time native discovery polls; `os_waits + discovery_polls <= 1`.
    /// Callback and IOCP Event-helper queue draining count as neither.
    pub discovery_polls: u32,
    /// Empty native wait/discovery calls, including interrupted calls.
    /// Retains raw accounting across both counters for the no-spin gate.
    /// Private timeout events (such as timerfd expiry) count as zero-event waits.
    pub zero_event_waits: u32,
}
