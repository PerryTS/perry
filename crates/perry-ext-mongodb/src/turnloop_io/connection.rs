//! The sans-I/O half of one MongoDB connection: the operation queue, the
//! receive staging buffer, and the [`DbCore`] implementation that joins them to
//! `perry-db-turnloop`'s transport.
//!
//! # One operation at a time, and why this file owns the queue
//!
//! `turnloop_mongodb::Connection` is a *turn-taking* state machine: its
//! `command` refuses with "Connection busy" unless the connection is `Ready`
//! and its transmit buffer is empty, and exactly one `Reply`, `Failed` or
//! `Unacknowledged` event follows an accepted token. That is MongoDB's wire
//! protocol, not a limitation of the crate — an OP_MSG reply is matched to its
//! request by `responseTo`, and this core keeps one request outstanding.
//!
//! JavaScript has no such rule. `Promise.all([find(a), find(b)])` submits both
//! before either resolves, and a second `insertOne` inside a `for await` can
//! land while the first is still on the wire. So **this file queues**: a
//! submission that cannot be issued waits in [`MongoCore::queue`] in submission
//! order and is issued when the previous operation completes. Losing,
//! reordering or rejecting that second submission would each be a real bug that
//! only shows up under concurrency, which is why `the_queue_issues_in_
//! submission_order_and_loses_nothing` pins it.
//!
//! # Receive staging
//!
//! `Connection::receive` returns the *consumed prefix* and deliberately stops
//! at a frame boundary: a complete reply must be consumed and released before
//! the next frame may be fed, and feeding while a reply is outstanding is an
//! error rather than a no-op. A socket read does not respect frame boundaries,
//! so bytes are buffered here and fed back in during [`MongoCore::drain`],
//! interleaved with releasing replies. Dropping the remainder would hang the
//! next operation on bytes that already arrived.

use std::collections::VecDeque;
use std::time::Instant;

use perry_db_turnloop::{DbCore, TlsFacts};
use perry_ffi::JsPromise;
use turnloop_mongodb::uri::Options;
use turnloop_mongodb::{Connection, ConnectionEvent, Error, ErrorKind};

use super::ops::{Operation, Request, Step};

/// An upper bound on bytes staged for a core that is not currently expecting a
/// reply. A well-behaved server never sends unsolicited data, so this only
/// bounds the damage a broken or hostile one can do; 64 MiB is above the
/// protocol's 48 MB maximum message size, so it can never reject a legitimate
/// reply.
const MAX_STAGED_BYTES: usize = 64 * 1024 * 1024;

/// A submitted operation: what it will send next, and the promise it owes.
struct PendingOp {
    op: Operation,
    request: Request,
    promise: JsPromise,
    /// The wire token, assigned when the operation is issued rather than when
    /// it is queued, so tokens run in issue order and a `Reply` can be matched
    /// against the operation that is actually outstanding.
    token: u64,
}

pub(crate) struct MongoCore {
    conn: Connection,
    /// Bytes received but not yet accepted by `conn` — see the module docs.
    staged: Vec<u8>,
    /// Submitted, not yet issued. Front is next.
    queue: VecDeque<PendingOp>,
    /// Issued, awaiting its reply. At most one, by the protocol.
    inflight: Option<PendingOp>,
    /// The connection is mid-exchange and `conn.receive` will accept bytes.
    ///
    /// Tracked here because `Connection` does not expose its state, and feeding
    /// it while it holds an unreleased reply is an error that would tear down a
    /// perfectly healthy connection.
    expecting_reply: bool,
    ready: bool,
    finished: bool,
    next_token: u64,
    /// The SCRAM client nonce, held only until the handshake consumes it.
    nonce: String,
    /// The deadline for the TCP connect plus handshake.
    ///
    /// `Connection` arms its own deadline from `connected()`, which cannot run
    /// before the transport is up — so a TCP connect to a host that blackholes
    /// packets would have no deadline at all. This one starts when the core is
    /// constructed and is dropped once the connection is `Ready`.
    connect_deadline: Option<Instant>,
    /// The last protocol-level error the core reported. Used as the reason for
    /// anything still outstanding when the connection goes away: without it, a
    /// connection that died during authentication settles its queue with
    /// "Connection closed", which hides the actual cause.
    last_error: Option<String>,
    /// The URI asked for TLS, so `connected()` raises `UpgradeTls` instead of
    /// sending the handshake. Kept here because `Connection` does not expose
    /// its state and `transport_connected` has to know whether a reply is due:
    /// in the upgrade state `receive` does not merely refuse a byte, it fails
    /// the connection.
    tls: bool,
    /// The core has asked for the upgrade. Taken by the driver, which flushes,
    /// installs the session and acknowledges it.
    tls_requested: bool,
}

impl MongoCore {
    pub(crate) fn new(options: Options, nonce: String) -> Self {
        let connect_deadline = if options.connect_timeout.is_zero() {
            None
        } else {
            Instant::now().checked_add(options.connect_timeout)
        };
        let tls = options.tls;
        Self {
            conn: Connection::new(options),
            staged: Vec::new(),
            queue: VecDeque::new(),
            inflight: None,
            expecting_reply: false,
            ready: false,
            finished: false,
            next_token: 1,
            nonce,
            connect_deadline,
            last_error: None,
            tls,
            tls_requested: false,
        }
    }

    /// Queue one operation and issue it if the connection is idle.
    ///
    /// Takes the promise by value: from here on this core owes the answer, on
    /// every path including its own teardown.
    pub(crate) fn submit(&mut self, op: Operation, request: Request, promise: JsPromise) {
        self.queue.push_back(PendingOp {
            op,
            request,
            promise,
            token: 0,
        });
        self.issue_next();
    }

    /// How many operations are submitted but not yet issued. A test uses this
    /// to prove the queue actually holds the second submission rather than
    /// dropping it.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Issue the front of the queue, if anything can be issued.
    ///
    /// Returns whether the connection made progress. An operation the core
    /// refuses outright — a command over the negotiated BSON size, say — is
    /// rejected here and the next one tried, because a refusal is that
    /// operation's failure and must not stall everything behind it.
    fn issue_next(&mut self) -> bool {
        if self.finished || !self.ready || self.inflight.is_some() {
            return false;
        }
        while let Some(mut pending) = self.queue.pop_front() {
            let token = self.next_token;
            self.next_token += 1;
            let now = Instant::now();
            let issued = match &pending.request {
                Request::Body(body) => self.conn.command(token, body, &[], now),
                Request::WithSequence { body, name, docs } => {
                    let refs: Vec<&bson::raw::RawDocument> =
                        docs.iter().map(|d| d.as_ref()).collect();
                    self.conn.command(token, body, &[(name, &refs)], now)
                }
            };
            match issued {
                Ok(()) => {
                    pending.token = token;
                    self.inflight = Some(pending);
                    self.expecting_reply = true;
                    return true;
                }
                Err(error) => {
                    let message = error.to_string();
                    pending.op.reject(pending.promise, &message);
                }
            }
        }
        false
    }

    /// Hand staged bytes to the core, as many as it will take right now.
    fn feed(&mut self) -> Result<bool, String> {
        if self.staged.is_empty() || !self.expecting_reply || self.finished {
            return Ok(false);
        }
        let consumed = self
            .conn
            .receive(&self.staged)
            .map_err(|e| format!("MongoDB protocol error: {}", e))?;
        if consumed == 0 {
            // The core is holding a complete frame it has not been allowed to
            // release yet. Stop rather than spin; the next drain iteration runs
            // after the reply is handled.
            return Ok(false);
        }
        self.staged.drain(..consumed);
        Ok(true)
    }

    fn on_event(&mut self, event: ConnectionEvent) -> Result<(), String> {
        match event {
            ConnectionEvent::UpgradeTls => {
                // MongoDB puts TLS underneath the whole protocol, so this
                // arrives from `connected()` before the `hello` — and therefore
                // before the speculative SCRAM the handshake carries when the
                // URI has credentials. Answering here would be too early: the
                // driver installs the session first and acknowledges it through
                // `tls_established`.
                self.tls_requested = true;
            }
            ConnectionEvent::Ready => {
                self.ready = true;
                self.expecting_reply = false;
                self.connect_deadline = None;
            }
            ConnectionEvent::Reply { token } => self.on_reply(token),
            ConnectionEvent::Unacknowledged { token } => {
                // This binding never sends `w: 0`, so no operation is waiting on
                // an unacknowledged write. Settle defensively rather than leave
                // a promise pending if that ever changes.
                self.settle_token_with_error(token, "MongoDB write was unacknowledged");
            }
            ConnectionEvent::Failed { token, error } => {
                let message = error.to_string();
                self.last_error = Some(message.clone());
                self.ready = false;
                self.expecting_reply = false;
                // A handshake or authentication failure carries no token, and
                // everything queued behind it is settled by the `Closed` event
                // that follows, through `fail`.
                if let Some(token) = token {
                    self.settle_token_with_error(token, &message);
                }
            }
            ConnectionEvent::Closed => {
                self.ready = false;
                self.expecting_reply = false;
                self.finished = true;
            }
        }
        Ok(())
    }

    /// Materialise one reply and settle (or continue) the operation that owns
    /// it.
    ///
    /// The reply borrows the core's receive buffer, so the conversion to owned
    /// data happens inside the `match` below and `release_reply` runs before
    /// anything else touches the core. Until it does, the connection refuses
    /// both a new command and another frame.
    fn on_reply(&mut self, token: u64) {
        let Some(mut pending) = self.inflight.take() else {
            // No operation owns this reply. Release the buffer anyway: leaving
            // it held would wedge the connection in a state that accepts
            // neither a command nor another frame.
            let _ = self.conn.release_reply();
            return;
        };
        if pending.token != token {
            // Equally impossible with one operation outstanding, and equally
            // not worth wedging the connection over.
            let _ = self.conn.release_reply();
            self.inflight = Some(pending);
            return;
        }
        let outcome = match self.conn.reply() {
            Ok(reply) => pending.op.interpret(reply),
            Err(error) => Err(format!("MongoDB protocol error: {}", error)),
        };
        let _ = self.conn.release_reply();
        self.expecting_reply = false;
        match outcome {
            Ok(Step::Settle(settlement)) => pending.op.settle(pending.promise, settlement),
            Ok(Step::More(request)) => {
                // A cursor continuation. It goes to the *front* so it keeps its
                // place ahead of operations submitted while it was in flight —
                // a `getMore` that queued behind a later `insertOne` would
                // interleave two round trips of the same logical read.
                pending.request = request;
                self.queue.push_front(pending);
            }
            Err(message) => {
                // A server-reported failure (a duplicate key, a bad filter) is
                // this operation's failure, not the connection's: the socket is
                // healthy and the next queued operation runs normally.
                pending.op.reject(pending.promise, &message);
            }
        }
    }

    fn settle_token_with_error(&mut self, token: u64, message: &str) {
        if let Some(pending) = self.inflight.take() {
            if pending.token == token {
                pending.op.reject(pending.promise, message);
            } else {
                self.inflight = Some(pending);
            }
        }
    }

    /// Settle everything outstanding with `reason`.
    ///
    /// Leaving a promise pending is the one outcome a caller cannot recover
    /// from — no rejection handler runs, no timeout fires, the `await` never
    /// returns — so every teardown path funnels through here.
    fn settle_all_with_error(&mut self, reason: &str) {
        let message = match &self.last_error {
            Some(recorded) => format!("{} ({})", recorded, reason),
            None => reason.to_string(),
        };
        if let Some(pending) = self.inflight.take() {
            pending.op.reject(pending.promise, &message);
        }
        while let Some(pending) = self.queue.pop_front() {
            pending.op.reject(pending.promise, &message);
        }
    }
}

/// The last line of defence for an unsettled promise.
///
/// `Registry::finish` drops an entry outright when `tl::close` reports the
/// handle is already gone (a close that raced the peer's reset), in which case
/// no `NET_CLOSED` completion will ever arrive to settle what this core owes.
/// Every other path settles first and finds nothing to do here.
impl Drop for MongoCore {
    fn drop(&mut self) {
        if self.inflight.is_some() || !self.queue.is_empty() {
            self.settle_all_with_error("MongoDB connection closed");
        }
    }
}

impl DbCore for MongoCore {
    fn transport_connected(&mut self) -> Result<(), String> {
        let now = Instant::now();
        self.conn
            .connected(now, &self.nonce)
            .map_err(|e| format!("MongoDB connection error: {}", e))?;
        // The nonce is consumed by the handshake; the connection keeps its own
        // copy for the SCRAM exchange, so drop this one rather than keep a
        // credential-adjacent secret alive for the life of the connection.
        self.nonce.clear();
        // Only when the handshake actually went out. On a TLS connection
        // `connected()` sent nothing and the core is in its upgrade state,
        // where feeding it a byte fails the connection outright; the reply
        // becomes due at `tls_established` instead.
        self.expecting_reply = !self.tls;
        Ok(())
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        if self.staged.len() + bytes.len() > MAX_STAGED_BYTES {
            return Err("MongoDB server sent more data than a reply can contain".to_string());
        }
        self.staged.extend_from_slice(bytes);
        Ok(())
    }

    fn take_tls_request(&mut self) -> bool {
        std::mem::take(&mut self.tls_requested)
    }

    fn tls_established(&mut self, facts: &TlsFacts) -> Result<(), String> {
        // `facts` is deliberately unread. Nothing in this protocol consumes
        // any of it: no ALPN is offered (see `tls_options`), and MongoDB's
        // SCRAM-SHA-256 has no channel-binding variant to feed the
        // `tls-server-end-point` digest to the way PostgreSQL's
        // SCRAM-SHA-256-PLUS does. The core's own acknowledgement takes no
        // argument for the same reason.
        let _ = facts;
        self.conn
            .tls_established()
            .map_err(|e| format!("MongoDB connection error: {}", e))?;
        // The `hello` is on the wire now, so a reply is due — the half of
        // `transport_connected` a TLS connection skipped.
        self.expecting_reply = true;
        Ok(())
    }

    fn drain(&mut self) -> Result<bool, String> {
        loop {
            let mut progress = false;
            while let Some(event) = self.conn.poll_event() {
                progress = true;
                self.on_event(event)?;
            }
            if self.finished {
                return Ok(true);
            }
            if self.issue_next() {
                progress = true;
            }
            if self.feed()? {
                progress = true;
            }
            if !progress {
                break;
            }
        }
        Ok(self.finished)
    }

    fn output(&self) -> &[u8] {
        self.conn.transmit()
    }

    fn consume_output(&mut self, n: usize) {
        // The only error is acknowledging more bytes than were offered, which
        // would be a driver bug rather than a connection failure; the core
        // leaves its buffer untouched in that case, so the bytes are re-sent
        // rather than lost. The call is made *outside* the assertion on
        // purpose: a side effect inside `debug_assert!` disappears in an
        // assertions-off build, and this one must always happen.
        let acknowledged = self.conn.consume_transmit(n);
        debug_assert!(
            acknowledged.is_ok(),
            "acknowledged more transmit bytes than the core offered"
        );
        drop(acknowledged);
    }

    fn next_timeout_ms(&self) -> Option<u64> {
        let at = self.conn.next_timeout().or(self.connect_deadline)?;
        let now = Instant::now();
        Some(if at <= now {
            0
        } else {
            at.duration_since(now).as_millis().min(u128::from(u64::MAX)) as u64
        })
    }

    fn handle_timeout(&mut self) {
        let now = Instant::now();
        if self.conn.next_timeout().is_none() {
            if self.connect_deadline.is_some_and(|at| now >= at) {
                self.connect_deadline = None;
                self.conn.fail(Error::new(
                    ErrorKind::Timeout,
                    "MongoDB connection timed out",
                ));
            }
            return;
        }
        self.conn.handle_timeout(now);
    }

    fn fail(&mut self, reason: &str) {
        self.conn
            .fail(Error::new(ErrorKind::Network, reason.to_string()));
        // Drain first so the core's own terminal events settle what they can,
        // and this only has to answer what they could not.
        let _ = self.drain();
        self.settle_all_with_error(reason);
        self.ready = false;
        self.finished = true;
    }

    fn has_pending_work(&self) -> bool {
        self.inflight.is_some() || !self.queue.is_empty()
    }
}
