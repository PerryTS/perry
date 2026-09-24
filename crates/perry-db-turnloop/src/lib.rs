//! The loop-driven transport shared by Perry's database bindings (turnloop P7).
//!
//! # What this replaces
//!
//! Every database binding in Perry had the same shape:
//!
//! ```text
//! perry_ffi::spawn_blocking(move || {
//!     tokio::runtime::Handle::current().block_on(async move { conn.query(..).await })
//! })
//! ```
//!
//! — an OS thread out of tokio's blocking pool, held for the whole duration of
//! the call, per in-flight operation. That is the pattern P4's report named as
//! the reason tokio's blocking pool survives its phase: turnloop's pool is
//! bounded and fixed-size, and a connection-shaped occupant cannot be hosted on
//! it.
//!
//! This module is the replacement. A connection becomes:
//!
//! * one turnloop handle (P1's `turnloop_net`), read multishot;
//! * one sans-I/O protocol core ([`DbCore`]) that never touches a socket;
//! * a table of outstanding operation tokens, each owning the promise it will
//!   settle.
//!
//! No thread is held at any point. N connections cost N descriptors and one
//! thread — the agent's own.
//!
//! # The contract, and why it is shaped this way
//!
//! Every turnloop protocol crate (`turnloop-redis`, `turnloop-postgres`,
//! `turnloop-mysql`, `turnloop-mongodb`) states the *same* host contract in its
//! README:
//!
//! 1. construct the core; the host connects the transport and says so;
//! 2. transmit `output()`, acknowledging only bytes actually written with
//!    `consume_output(n)`;
//! 3. feed received plaintext to `receive(bytes)`, then pull events until the
//!    core has none left, flushing any newly generated output;
//! 4. schedule `next_timeout()` yourself and call `handle_timeout(now)`;
//! 5. on transport failure, abort and drain the terminal events.
//!
//! [`DbCore`] is that contract with the per-crate spelling erased. The one
//! deliberate difference is [`DbCore::drain`]: the crates return a *borrowed*
//! event (`Event<'a>`, borrowing the receive buffer), which cannot cross a
//! trait method without tying the borrow to `&mut self` for the caller's whole
//! handling block. So a driver drains and handles its own events inside one
//! call and reports only whether the connection is finished. That also keeps
//! this module free of any knowledge of rows, replies or promises.
//!
//! # Threading and the GC
//!
//! [`Registry::dispatch`] runs on the agent thread, from the loop's own
//! completion dispatch, *after* a turn has returned (DESIGN D1) — the same
//! place `perry-ext-net`'s sink runs. It may therefore allocate Rust state and
//! settle promise tokens, but it must not run JS. Results cross to the main
//! thread as owned Rust data inside a `perry_ffi::JsPromise::resolve_with`
//! closure, which the resolution pump invokes on the main thread; that is the
//! same #1824 rule the `spawn_blocking` bindings already had to obey, now with
//! no worker thread involved at all.
//!
//! **No JS value and no heap pointer reaches the driver.** Read bytes are
//! copied out of turnloop's pooled lease inside the dispatch call; writes are
//! handed over as owned `Vec<u8>`. This module therefore registers no GC root
//! scanner, exactly as P1's `turnloop_net` does not.
//!
//! # Why the registry is thread-local
//!
//! A turnloop handle belongs to the loop that created it, and perry-runtime's
//! `turnloop_net` keeps its own entry table in a `thread_local!` for that
//! reason. Holding the mirror of it in a process-global `Mutex` would be a
//! claim that a connection can be driven from another thread, which is false —
//! and it would force `Send` on the protocol cores, which own promise pointers.
//! A binding therefore declares its [`Registry`] in a `thread_local!`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use perry_ffi::turnloop_net as tl;
use perry_tls_turnloop::{TlsClientTransport, TlsProgress};

pub use perry_ffi::turnloop_net::NetCompletion;
/// Re-exported so a binding can describe its TLS without naming rustls or
/// taking a dependency of its own: what a driver knows about TLS is "the user
/// asked for it, here is the trust material", and what it learns back is "this
/// is the negotiated protocol and this is the channel binding".
pub use perry_tls_turnloop::{TlsClientOptions, TlsFacts};

/// `PERRY_DB_TURNLOOP_DIAG=1` prints one line per connection open and close.
///
/// This exists for the same reason `PERRY_LOOP_STATS` does: "the gap suite is
/// green" says nothing about whether *this* transport carried the workload, and
/// a migration whose subject never ran is the failure mode CLAUDE.md's
/// "four ways a gate can be unable to fail" names as the most dangerous. A
/// `[perry-db]` line is positive proof that a real connection was opened here,
/// and the `reads=`/`writes=` on its close line prove bytes moved.
///
/// Diagnostic-only and default-off, read once per process.
fn diag() -> bool {
    use std::sync::atomic::AtomicU8;
    static STATE: AtomicU8 = AtomicU8::new(0);
    match STATE.load(Ordering::Relaxed) {
        0 => {
            let on = matches!(
                std::env::var("PERRY_DB_TURNLOOP_DIAG").as_deref(),
                Ok("1") | Ok("on") | Ok("true")
            );
            STATE.store(if on { 2 } else { 1 }, Ordering::Relaxed);
            on
        }
        2 => true,
        _ => false,
    }
}

/// Subsystem slots in the runtime's sink registry.
///
/// 0 is `perry-ext-net` (P1) and 1 is `perry-ext-http` (P5); 3 is the runtime's
/// own unit-test slot. These four are P7's, one per binding, because each
/// binding is a separately linked `staticlib` with its own sink function — they
/// cannot share one slot even though they share this module.
pub mod subsystem {
    //! The database bindings' completion-sink slots.
    //!
    //! These are a contiguous band ABOVE the slots the server and client lanes
    //! hardcode in their own crates (0 `perry-ext-net`, 1 `perry-ext-http`'s
    //! SERVER, 2 `perry-stdlib`'s turnloop HTTP client, 3 `perry-stdlib`'s
    //! SMTP, 4 `perry-ext-fastify`, 5 `perry-stdlib`'s framework server, 6
    //! `perry-ext-http`'s node:http CLIENT (`client_turnloop`), 7 and 8
    //! `perry-ext-ws`). They used to be 2/4/5/6, which collided with three of
    //! those, because the P7 database lane and the P5 server lane numbered
    //! from two different ledgers. A collision needs a program that links both
    //! bindings — a fastify app using mysql2 — and `register_sink` used to
    //! accept both and route every completion to whichever registered last.
    //!
    //! Keep this band contiguous and keep it here: it is the one authority for
    //! the database slots, and `perry-runtime`'s `turnloop_net::sink` carries
    //! the full map. The ceiling is `MAX_SUBSYSTEMS` (16).

    /// Formerly `perry-ext-pg` (removed, #10677: npm `pg` now compiles from
    /// source over `net`). Kept reserved so the database band never renumbers
    /// and a stale archive cannot collide.
    pub const PG: u8 = 9;
    /// Formerly `perry-ext-mysql2` (removed, #10680: npm `mysql2` now compiles
    /// from source over `net`). Kept reserved, as `PG`.
    pub const MYSQL: u8 = 10;
    /// Formerly `perry-ext-ioredis` (removed: npm `redis`/`ioredis`/`iovalkey`
    /// now compile from source over `net`). Kept reserved so the database
    /// band never renumbers and a stale archive cannot collide.
    pub const REDIS: u8 = 11;
    /// Formerly `perry-ext-mongodb` (removed: npm `mongodb` now compiles
    /// from source over `net`). Kept reserved so the database band never
    /// renumbers and a stale archive cannot collide.
    pub const MONGODB: u8 = 12;
}

/// Driver ids are handed out process-wide even though the tables are
/// per-thread, so an id never means two different connections in one process
/// and a stray completion can be recognised as stale rather than misrouted.
static NEXT_ID: AtomicI64 = AtomicI64::new(0);

/// Reserve this binding's id band. Bands are 2^40 apart, which is more ids than
/// a process can open connections and keeps a debugger able to tell at a glance
/// which binding an id belongs to.
pub const fn id_base(subsystem: u8) -> i64 {
    (subsystem as i64 + 1) << 40
}

/// The sans-I/O half of a database connection.
///
/// One implementation per protocol crate. Every method runs on the agent
/// thread; none may run JS, and none may block.
pub trait DbCore: 'static {
    /// The transport is up. The core emits its startup/handshake bytes here.
    fn transport_connected(&mut self) -> Result<(), String>;

    /// Plaintext arrived.
    fn receive(&mut self, bytes: &[u8]) -> Result<(), String>;

    /// Drain every protocol event the core has, settling whatever they settle.
    ///
    /// Returns `true` once the connection is finished — its terminal close
    /// event has fired — after which the driver closes the handle and drops the
    /// entry. An `Err` is a protocol error: the driver fails the core and tears
    /// the connection down.
    fn drain(&mut self) -> Result<bool, String>;

    /// Bytes the core wants on the wire.
    fn output(&self) -> &[u8];

    /// Acknowledge `n` bytes of [`Self::output`] as handed to the transport.
    fn consume_output(&mut self, n: usize);

    /// The core's next deadline, in milliseconds from now, if it has one.
    ///
    /// Relative rather than absolute because turnloop's `timer_arm` takes a
    /// delay and because the core owns the clock epoch — asking it to subtract
    /// keeps every `Instant` inside the crate that created it.
    fn next_timeout_ms(&self) -> Option<u64>;

    /// That deadline expired.
    fn handle_timeout(&mut self);

    /// The transport failed, or the core produced an error. Settle every
    /// outstanding operation with `reason` and stop.
    fn fail(&mut self, reason: &str);

    /// Whether the core has asked for the transport to be upgraded to TLS,
    /// taking the request so it is answered once.
    ///
    /// Polled after every drain. All four protocol crates spell the request the
    /// same way — an `UpgradeTls` event — but they raise it at different
    /// moments: `turnloop-redis` and `turnloop-mongodb` raise it from
    /// `transport_connected`, before a single protocol byte, while
    /// `turnloop-postgres` raises it after the one-byte `S` answer to its
    /// `SSLRequest` and `turnloop-mysql` after the server's greeting and its
    /// own `SSLRequest` packet. The driver does not need to know which: it
    /// flushes whatever plaintext the core still owes, *then* installs the
    /// session. That ordering is required by both mid-stream cores — each
    /// refuses `tls_established` while its output is unflushed.
    fn take_tls_request(&mut self) -> bool {
        false
    }

    /// The TLS handshake completed. Acknowledge it to the protocol core.
    ///
    /// `facts` carries the negotiated ALPN protocol (none, for every database
    /// here) and the RFC 5929 `tls-server-end-point` digest of the verified
    /// leaf, which is what PostgreSQL's SCRAM-SHA-256-PLUS binds to. A core
    /// that is told a binding is available and then cannot produce one fails
    /// the connection rather than authenticating with the wrong digest, so
    /// pass `facts.channel_binding.is_some()` through honestly.
    fn tls_established(&mut self, facts: &TlsFacts) -> Result<(), String> {
        let _ = facts;
        Ok(())
    }

    /// Whether the connection still owes an answer to JS.
    ///
    /// The driver mirrors this onto the turnloop handle's ref flag, so an idle
    /// pooled connection does not keep the process alive while an in-flight
    /// query does. That reproduces the pre-P7 behaviour exactly: under
    /// `spawn_blocking` the keep-alive gate counted *blocking tasks in flight*
    /// (`EXT_BLOCKING_TASKS_INFLIGHT`, #591), never idle connections. A
    /// database client that is never `.end()`ed therefore still lets the
    /// process exit, as it does today — and unlike Node, which is a
    /// pre-existing Perry divergence this change deliberately does not move.
    fn has_pending_work(&self) -> bool;
}

/// One connection: its core plus the transport bookkeeping.
struct Entry<C: DbCore> {
    core: C,
    /// The connect completion has arrived.
    connected: bool,
    /// A deadline is armed on this id.
    timer_armed: bool,
    /// The handle has been asked to close; further submissions are refused.
    closing: bool,
    /// The TLS session, once installed. Owned by the entry rather than kept in
    /// a table keyed by handle id, for the same reason this whole registry is
    /// thread-local: a turnloop handle belongs to the loop that created it.
    tls: Option<TlsClientTransport>,
    /// What to install when the core asks. `None` means the client configured
    /// no TLS, which makes an `UpgradeTls` request a connection error rather
    /// than a silent plaintext continuation.
    tls_options: Option<TlsClientOptions>,
    /// Whatever the binding wants to hang off the connection (its JS-visible
    /// handle id, a pool membership). Opaque here.
    tag: u64,
}

/// The per-binding connection table. Declared by the binding in a
/// `thread_local!` (see the module docs for why).
pub struct Registry<C: DbCore> {
    subsystem: u8,
    registered: Cell<bool>,
    connects: Cell<usize>,
    reads: Cell<usize>,
    writes: Cell<usize>,
    timers: Cell<usize>,
    tls_handshakes: Cell<usize>,
    entries: RefCell<HashMap<i64, Entry<C>>>,
}

impl<C: DbCore> Registry<C> {
    pub fn new(subsystem: u8) -> Self {
        Self {
            subsystem,
            registered: Cell::new(false),
            connects: Cell::new(0),
            reads: Cell::new(0),
            writes: Cell::new(0),
            timers: Cell::new(0),
            tls_handshakes: Cell::new(0),
            entries: RefCell::new(HashMap::new()),
        }
    }

    /// Whether a connection created *now, on this thread* can live on turnloop.
    ///
    /// Deliberately re-asked per connection rather than cached: a
    /// `worker_threads` agent has no loop of its own, and caching its "no"
    /// would strand the primary agent on the legacy transport for the rest of
    /// the run. `register_sink` is idempotent and refuses outright if the
    /// runtime's completion layout does not match this crate's, which leaves
    /// [`tl::available`] false and keeps every connection on the old path
    /// rather than submitting work nothing can deliver.
    pub fn enabled(&self, sink: tl::SinkFn) -> bool {
        self.register(sink) && tl::available(self.subsystem)
    }

    /// Install this binding's sink, once. Returns whether the runtime accepted
    /// it — false means the completion layout digests disagree, which is the
    /// drift check `perry-ffi` performs before any submission can happen.
    ///
    /// Separate from [`Self::enabled`] because the two answer different
    /// questions: this one is a property of the *build* and is deterministic;
    /// `available` is a property of the calling *agent* and is not.
    pub fn register(&self, sink: tl::SinkFn) -> bool {
        if self.registered.get() {
            return true;
        }
        let ok = tl::register_sink(self.subsystem, sink, never_accepts);
        self.registered.set(ok);
        ok
    }

    /// Insert a connection with no transport behind it, for tests of the
    /// settlement contract that must not depend on whether the running thread
    /// happens to own a loop. `cargo test` puts each test on its own thread and
    /// only some of them do, which made the driver's own tests flaky before
    /// this existed.
    #[cfg(test)]
    fn insert_detached(&self, id: i64, core: C, tag: u64) {
        self.entries.borrow_mut().insert(
            id,
            Entry {
                core,
                connected: false,
                timer_armed: false,
                closing: false,
                tls: None,
                tls_options: None,
                tag,
            },
        );
        self.connects.set(self.connects.get() + 1);
    }

    /// Open a plaintext connection. Returns its driver id; the core's handshake
    /// runs when the connect completion arrives.
    pub fn connect(&self, host: &str, port: u16, core: C, tag: u64) -> Result<i64, String> {
        self.connect_with_tls(host, port, core, tag, None)
    }

    /// Open a connection whose core may ask for a TLS upgrade.
    ///
    /// `tls` is what will be installed *if* the core asks — it does not itself
    /// start a handshake. Which is the whole point: the four protocol cores
    /// disagree about when TLS begins (see [`DbCore::take_tls_request`]) and
    /// agree about how they ask for it, so the driver holds the configuration
    /// and lets the core choose the moment.
    pub fn connect_with_tls(
        &self,
        host: &str,
        port: u16,
        core: C,
        tag: u64,
        tls: Option<TlsClientOptions>,
    ) -> Result<i64, String> {
        let id = id_base(self.subsystem) + NEXT_ID.fetch_add(1, Ordering::Relaxed);
        self.entries.borrow_mut().insert(
            id,
            Entry {
                core,
                connected: false,
                timer_armed: false,
                closing: false,
                tls: None,
                tls_options: tls,
                tag,
            },
        );
        // `nodelay` on: a database client is request/response, and Nagle adds
        // up to a round trip of latency to every small command. Both sqlx and
        // the `redis` crate set it, so this keeps the wire behaviour the same.
        if let Err(err) = tl::tcp_connect(id, self.subsystem, host, port, true) {
            self.entries.borrow_mut().remove(&id);
            return Err(err.message());
        }
        self.connects.set(self.connects.get() + 1);
        if diag() {
            eprintln!(
                "[perry-db] subsystem={} connect id={} {}:{}",
                self.subsystem, id, host, port
            );
        }
        // A connect in progress is work the process owes an answer for.
        tl::set_ref(id, true);
        Ok(id)
    }

    /// Run `f` against a live connection's core, then flush whatever it wrote
    /// and re-arm its deadline. This is how a binding submits a command.
    ///
    /// Returns `None` when the id names no live, non-closing connection.
    pub fn with_core<R>(&self, id: i64, f: impl FnOnce(&mut C) -> R) -> Option<R> {
        let out = {
            let mut map = self.entries.borrow_mut();
            let entry = map.get_mut(&id)?;
            if entry.closing {
                return None;
            }
            f(&mut entry.core)
        };
        self.flush(id);
        Some(out)
    }

    /// Read-only access, for a binding that only needs to inspect state.
    pub fn inspect<R>(&self, id: i64, f: impl FnOnce(&C) -> R) -> Option<R> {
        let map = self.entries.borrow();
        map.get(&id).map(|e| f(&e.core))
    }

    /// The binding's opaque tag for this connection.
    pub fn tag(&self, id: i64) -> Option<u64> {
        self.entries.borrow().get(&id).map(|e| e.tag)
    }

    /// Whether `id` names a live connection.
    pub fn is_live(&self, id: i64) -> bool {
        self.entries.borrow().contains_key(&id)
    }

    /// How many connections this binding has open on this thread. A test uses
    /// this so a "turnloop carried this" claim cannot pass with nothing
    /// connected.
    pub fn live_connections(&self) -> usize {
        self.entries.borrow().len()
    }

    /// `(connects, reads, writes, timer arms)` since process start, on this
    /// thread. `timers` is the one a reader is most likely to need: a protocol
    /// deadline that never armed is a command timeout that can never fire, and
    /// nothing else in the process would say so.
    pub fn counters(&self) -> (usize, usize, usize, usize) {
        (
            self.connects.get(),
            self.reads.get(),
            self.writes.get(),
            self.timers.get(),
        )
    }

    /// Hand the core's pending output to turnloop and re-arm its deadline.
    ///
    /// The bytes are acknowledged with `consume_output` as soon as `write`
    /// returns, because turnloop takes an **owned** `Vec` and orders a handle's
    /// writes — once it has accepted them, nothing encoded afterwards can
    /// overtake them. That is the same acknowledgement point P5 chose for TLS
    /// records, and it is what makes a Redis pipeline or a MULTI/EXEC block
    /// reach the wire in submission order.
    pub fn flush(&self, id: i64) {
        // The whole body deliberately releases the table borrow before every
        // FFI submission: `abort` re-enters, and a `RefCell` held across it
        // would panic rather than misbehave quietly.
        //
        // Output is written only once the transport is up — a core's handshake
        // bytes are produced by `transport_connected`, not before — but the
        // deadline and the ref flag are armed either way. A connect that never
        // completes has a deadline of its own (every core sets one), and
        // skipping the arm until after the connect would make exactly that
        // case the one nothing can time out.
        let chunk: Option<Vec<u8>> = {
            let map = self.entries.borrow();
            match map.get(&id) {
                Some(e) if e.closing => return,
                Some(e) if e.connected => {
                    let out = e.core.output();
                    (!out.is_empty()).then(|| out.to_vec())
                }
                Some(_) => None,
                None => return,
            }
        };
        if let Some(bytes) = chunk {
            // With a session installed the core's bytes are PLAINTEXT and the
            // socket takes ciphertext, so the write goes through the session
            // instead. The acknowledgement point does not move: rustls has
            // taken the bytes and turnloop orders a handle's writes, so nothing
            // encrypted afterwards can overtake them — the same reasoning the
            // plaintext arm below records, one layer up.
            let carried = if self.has_tls(id) {
                match self.run_tls(id, |tls| tls.write(&bytes)) {
                    Some(progress) => {
                        if let Some(failure) = progress.failure {
                            self.abort(id, &failure);
                            return;
                        }
                        true
                    }
                    None => return,
                }
            } else {
                match tl::write(id, &bytes, 0) {
                    Ok(_) => true,
                    Err(err) => {
                        self.abort(id, &err.message());
                        return;
                    }
                }
            };
            if carried {
                if let Some(e) = self.entries.borrow_mut().get_mut(&id) {
                    e.core.consume_output(bytes.len());
                }
                self.writes.set(self.writes.get() + 1);
            }
        }
        let (delay, referenced) = {
            let map = self.entries.borrow();
            match map.get(&id) {
                Some(e) => (e.core.next_timeout_ms(), e.core.has_pending_work()),
                None => return,
            }
        };
        self.arm(id, delay);
        tl::set_ref(id, referenced);
    }

    /// Arm (or cancel) the core's deadline as a real turnloop deadline, so a
    /// park whose only outstanding work is a database timeout ends on time
    /// rather than blocking until the server moves.
    fn arm(&self, id: i64, delay_ms: Option<u64>) {
        let armed = match self.entries.borrow().get(&id) {
            Some(e) => e.timer_armed,
            None => return,
        };
        match delay_ms {
            Some(ms) => {
                if tl::timer_arm(id, self.subsystem, ms).is_ok() {
                    self.timers.set(self.timers.get() + 1);
                    if let Some(e) = self.entries.borrow_mut().get_mut(&id) {
                        e.timer_armed = true;
                    }
                }
            }
            None if armed => {
                let _ = tl::timer_cancel(id);
                if let Some(e) = self.entries.borrow_mut().get_mut(&id) {
                    e.timer_armed = false;
                }
            }
            None => {}
        }
    }

    /// Whether a TLS session is installed on this connection.
    pub fn has_tls(&self, id: i64) -> bool {
        self.entries
            .borrow()
            .get(&id)
            .is_some_and(|e| e.tls.is_some())
    }

    /// What the handshake negotiated, once it has completed.
    ///
    /// A binding reports this to JS (`connection.ssl`, an `alpnProtocol`) and a
    /// fixture asserts on it. `channel_binding` being `Some` is the only
    /// evidence that a SCRAM-SHA-256-PLUS exchange had a real digest to bind.
    pub fn tls_facts(&self, id: i64) -> Option<TlsFacts> {
        self.entries
            .borrow()
            .get(&id)
            .and_then(|e| e.tls.as_ref())
            .and_then(|tls| tls.facts().cloned())
    }

    /// Ciphertext bytes this connection has pushed through its TLS session.
    ///
    /// The liveness counter for a TLS claim. `has_tls` says a session was
    /// installed; only this says bytes went through it, which is the difference
    /// between a gate that can fail and one that cannot.
    pub fn tls_cipher_written(&self, id: i64) -> u64 {
        self.entries
            .borrow()
            .get(&id)
            .and_then(|e| e.tls.as_ref())
            .map_or(0, TlsClientTransport::cipher_written)
    }

    /// How many TLS handshakes this binding has completed on this thread.
    pub fn tls_handshakes(&self) -> usize {
        self.tls_handshakes.get()
    }

    /// Run `f` against the connection's TLS session, then pump it and submit
    /// whatever ciphertext it produced.
    ///
    /// The session is taken **out** of the table for the duration: the pump
    /// submits through the FFI, and this module's rule is that no `RefCell`
    /// borrow is held across a submission (`abort` re-enters, and a held borrow
    /// would panic rather than misbehave quietly). Returns `None` when the id
    /// names no connection with a session, so every caller has one way to say
    /// "there was nothing to do".
    fn run_tls(&self, id: i64, f: impl FnOnce(&mut TlsClientTransport)) -> Option<TlsProgress> {
        let mut tls = {
            let mut map = self.entries.borrow_mut();
            map.get_mut(&id)?.tls.take()?
        };
        f(&mut tls);
        let progress = tls.pump(id);
        // The entry can be gone: `pump` submits, and a submission failure on a
        // handle the loop has already retired reaches us as a closed entry.
        if let Some(entry) = self.entries.borrow_mut().get_mut(&id) {
            entry.tls = Some(tls);
        }
        Some(progress)
    }

    /// Install the TLS session the core just asked for and send the
    /// ClientHello.
    ///
    /// The caller must have flushed the core's pending plaintext first — both
    /// mid-stream cores refuse `tls_established` while their output is
    /// unflushed, and MySQL's `SSLRequest` packet is *in* that output.
    fn begin_tls(&self, id: i64) {
        let options = match self
            .entries
            .borrow()
            .get(&id)
            .and_then(|e| e.tls_options.clone())
        {
            Some(options) => options,
            // Reached only if a core asks for an upgrade the binding never
            // configured. Failing is the only safe answer: continuing in
            // plaintext would mean a client that asked for TLS, was told the
            // server wanted it, and then sent its password in the clear.
            None => {
                self.abort(
                    id,
                    "the server requested a TLS upgrade but this connection has no TLS configuration",
                );
                return;
            }
        };
        let transport = match TlsClientTransport::connect(&options) {
            Ok(transport) => transport,
            Err(message) => {
                self.abort(id, &message);
                return;
            }
        };
        {
            let mut map = self.entries.borrow_mut();
            let Some(entry) = map.get_mut(&id) else {
                return;
            };
            if entry.closing {
                return;
            }
            entry.tls = Some(transport);
        }
        self.tls_handshakes.set(self.tls_handshakes.get() + 1);
        if diag() {
            eprintln!(
                "[perry-db] subsystem={} id={} tls upgrade begins servername={:?}",
                self.subsystem, id, options.servername
            );
        }
        // Sends the ClientHello.
        if let Some(progress) = self.run_tls(id, |_| {}) {
            if let Some(failure) = progress.failure {
                self.abort(id, &failure);
            }
        }
    }

    /// Acknowledge a completed handshake to the core, and report it.
    fn finish_tls(&self, id: i64) -> bool {
        let Some(facts) = self.tls_facts(id) else {
            return true;
        };
        if diag() {
            eprintln!(
                "[perry-db] subsystem={} id={} tls established alpn={:?} chain={} channel_binding={}",
                self.subsystem,
                id,
                facts.alpn_str(),
                facts.peer_certificates.len(),
                facts.channel_binding.is_some(),
            );
        }
        let acknowledged = {
            let mut map = self.entries.borrow_mut();
            match map.get_mut(&id) {
                Some(entry) => entry.core.tls_established(&facts),
                None => return false,
            }
        };
        match acknowledged {
            Ok(()) => true,
            Err(message) => {
                self.abort(id, &message);
                false
            }
        }
    }

    /// Fail the core, settle everything it owes and tear the connection down.
    pub fn abort(&self, id: i64, reason: &str) {
        {
            let mut map = self.entries.borrow_mut();
            let Some(entry) = map.get_mut(&id) else {
                return;
            };
            if entry.closing {
                return;
            }
            entry.closing = true;
            entry.core.fail(reason);
            // Drain once so the core's terminal events settle the operations it
            // still owes. A failure here is the failure we already have.
            let _ = entry.core.drain();
        }
        self.finish(id);
    }

    /// Close the connection. The core has already been told to end, or has
    /// nothing left to say.
    pub fn close(&self, id: i64) {
        {
            let mut map = self.entries.borrow_mut();
            let Some(entry) = map.get_mut(&id) else {
                return;
            };
            entry.closing = true;
        }
        self.finish(id);
    }

    fn finish(&self, id: i64) {
        let armed = match self.entries.borrow_mut().get_mut(&id) {
            Some(e) => std::mem::replace(&mut e.timer_armed, false),
            None => return,
        };
        if armed {
            let _ = tl::timer_cancel(id);
        }
        // A TLS connection that just vanishes makes the server log a truncation
        // ("could not receive data from client: Connection reset by peer" on
        // PostgreSQL). turnloop orders a handle's writes ahead of its close, so
        // queueing `close_notify` here puts it on the wire before the FIN. A
        // failed session refuses it, which is why this needs no guard of its
        // own.
        if self.has_tls(id) {
            let _ = self.run_tls(id, TlsClientTransport::close_notify);
        }
        // `close` is exactly-once in the driver and answers with `NET_CLOSED`,
        // which is where the entry is normally retired. If the handle is
        // already gone — a close that raced the peer's reset, or a second
        // close — **no completion can arrive**, so the entry has to be retired
        // here instead. Retiring it without settling is the bug this branch
        // exists to avoid: dropping the entry drops the core, and with it every
        // promise the core still owes, leaving them pending forever.
        if tl::close(id).is_err() {
            self.retire(id);
        }
    }

    /// Settle whatever the core still owes and drop the entry.
    ///
    /// The two callers are the driver's terminal `NET_CLOSED` completion and
    /// the `close`-already-gone path above. Both must settle before removing:
    /// a `JsPromise` that is dropped rather than settled is a promise that
    /// never resolves and never rejects, which is the one outcome a caller
    /// cannot recover from.
    fn retire(&self, id: i64) {
        {
            let mut map = self.entries.borrow_mut();
            let Some(entry) = map.get_mut(&id) else {
                return;
            };
            if entry.core.has_pending_work() {
                entry.core.fail("Connection closed");
                // The core's own terminal events settle what it can; `fail` is
                // required to settle the rest.
                let _ = entry.core.drain();
            }
            map.remove(&id);
        }
        if diag() {
            eprintln!(
                "[perry-db] subsystem={} closed id={} connects={} reads={} writes={} timer_arms={} live={}",
                self.subsystem,
                id,
                self.connects.get(),
                self.reads.get(),
                self.writes.get(),
                self.timers.get(),
                self.entries.borrow().len()
            );
        }
    }

    /// Route one completion. A binding's sink is a one-line forward to this.
    ///
    /// Runs on the agent thread, inside the loop's completion dispatch.
    pub fn dispatch(&self, c: &NetCompletion) {
        let id = c.id;
        match c.kind {
            tl::NET_CONNECT => {
                let started = {
                    let mut map = self.entries.borrow_mut();
                    let Some(entry) = map.get_mut(&id) else {
                        return;
                    };
                    entry.connected = true;
                    entry.core.transport_connected()
                };
                match started {
                    Ok(()) => {
                        if let Err(err) = tl::read_start(id) {
                            self.abort(id, &err.message());
                            return;
                        }
                        self.flush(id);
                        self.drive(id);
                    }
                    Err(message) => self.abort(id, &message),
                }
            }
            tl::NET_DATA => {
                self.reads.set(self.reads.get() + 1);
                // Copy out of turnloop's pooled lease before the sink returns:
                // the buffer goes back to the pool the moment it does.
                let bytes: &[u8] = if c.data.is_null() || c.len == 0 {
                    &[]
                } else {
                    // SAFETY: the driver guarantees `data`/`len` for the
                    // duration of this call.
                    unsafe { std::slice::from_raw_parts(c.data, c.len) }
                };
                // With a session installed these are TLS records, not protocol
                // bytes: decrypt first and hand the core only what came out.
                // Every core in this family refuses plaintext once it is in its
                // TLS state, so feeding the records straight through would be a
                // protocol error rather than a silent corruption — but the
                // right answer is still to never do it.
                let plaintext = if self.has_tls(id) {
                    let Some(progress) = self.run_tls(id, |tls| tls.receive(bytes)) else {
                        return;
                    };
                    if let Some(failure) = progress.failure {
                        self.abort(id, &failure);
                        return;
                    }
                    if progress.handshake_done && !self.finish_tls(id) {
                        return;
                    }
                    if progress.peer_closed && progress.plaintext.is_empty() {
                        self.abort(id, "Connection closed by the server");
                        return;
                    }
                    progress.plaintext
                } else {
                    bytes.to_vec()
                };
                if plaintext.is_empty() {
                    // A handshake flight carries no application data. Flushing
                    // is still right: acknowledging TLS can have produced the
                    // core's startup bytes.
                    self.drive(id);
                    return;
                }
                let fed = {
                    let mut map = self.entries.borrow_mut();
                    let Some(entry) = map.get_mut(&id) else {
                        return;
                    };
                    if entry.closing {
                        return;
                    }
                    entry.core.receive(&plaintext)
                };
                match fed {
                    Ok(()) => self.drive(id),
                    Err(message) => self.abort(id, &message),
                }
            }
            tl::NET_EOF => {
                // A database server closing its write side ends the session:
                // every protocol here is request/response over one stream, so
                // there is no half-open state a core could make progress in.
                self.abort(id, "Connection closed by the server");
            }
            tl::NET_TIMER => {
                {
                    let mut map = self.entries.borrow_mut();
                    let Some(entry) = map.get_mut(&id) else {
                        return;
                    };
                    entry.timer_armed = false;
                    if entry.closing {
                        return;
                    }
                    entry.core.handle_timeout();
                }
                self.drive(id);
            }
            tl::NET_ERROR => {
                let message = completion_message(c);
                self.abort(id, &message);
            }
            tl::NET_CLOSED => {
                // The driver says the handle is really gone.
                self.retire(id);
            }
            // A write completion carries no information this transport needs:
            // `flush` acknowledged the bytes when turnloop took ownership, and
            // no caller waits on a per-write callback the way a `net.Socket`
            // write does.
            _ => {}
        }
    }

    /// Drain the core's events, flush whatever they produced, and retire the
    /// connection if it finished.
    fn drive(&self, id: i64) {
        let outcome = {
            let mut map = self.entries.borrow_mut();
            let Some(entry) = map.get_mut(&id) else {
                return;
            };
            entry.core.drain()
        };
        match outcome {
            Ok(true) => {
                self.close(id);
                return;
            }
            Ok(false) => {}
            Err(message) => {
                self.abort(id, &message);
                return;
            }
        }
        let upgrade = {
            let mut map = self.entries.borrow_mut();
            match map.get_mut(&id) {
                Some(entry) if !entry.closing => entry.core.take_tls_request(),
                _ => return,
            }
        };
        // Order matters and is not interchangeable: the flush below happens
        // while the connection is still plaintext, which is what puts
        // PostgreSQL's `SSLRequest` and MySQL's `SSLRequest` packet on the wire
        // unencrypted. Installing first would encrypt the very packet that asks
        // for encryption, and both cores refuse `tls_established` while their
        // output is unflushed, so the mistake would surface as a state error
        // rather than as a hang.
        self.flush(id);
        if upgrade {
            self.begin_tls(id);
        }
    }
}

/// Node's `code`/`syscall` pair for a failing completion, rendered as the
/// message a rejected promise carries.
pub fn completion_message(c: &NetCompletion) -> String {
    let code = borrowed(c.code, c.code_len);
    let syscall = borrowed(c.syscall, c.syscall_len);
    match (code.is_empty(), syscall.is_empty()) {
        (false, false) => format!("{} {} {}", code, syscall, c.errno),
        (false, true) => code,
        (true, false) => format!("{} {}", syscall, c.errno),
        (true, true) => "Connection error".to_string(),
    }
}

fn borrowed(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: borrowed for the duration of the sink call.
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

/// A database binding never listens, so it can never be handed an accepted
/// connection. Returning zero refuses one, which is what the registry's
/// allocator contract asks for.
extern "C" fn never_accepts() -> i64 {
    0
}

#[cfg(test)]
mod tests;
