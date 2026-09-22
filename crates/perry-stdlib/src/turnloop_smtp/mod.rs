//! turnloop P6: SMTP on turnloop handles.
//!
//! `lettre`'s `AsyncSmtpTransport<Tokio1Executor>` owns a tokio `TcpStream`, a
//! `tokio_rustls` session and its own task. `turnloop_smtp::Connection` is the
//! same protocol with none of that: a pull-driven state machine the host feeds
//! bytes and takes bytes from. This module is the transport underneath it —
//! P1's socket layer, P6's shared client TLS session, and the completion sink
//! that joins them.
//!
//! ```text
//!   send(config, job)  ──► turnloop_net::tcp_connect_host
//!                                  │ NET_CONNECT
//!                                  ▼
//!                        Connection::connected  ──► 220 greeting ─► EHLO
//!                                  │ Event::UpgradeTls  (STARTTLS / implicit)
//!                                  ▼
//!                        TlsClientSession  ──► Connection::tls_established
//!                                  │ EHLO ─► AUTH ─► Event::Ready
//!                                  ▼
//!                        Connection::send(envelope, message)
//!                                  │ Event::Sent { info } / Event::Failed
//!                                  ▼
//!                        Sink::on_done → queue_deferred_resolution
//! ```
//!
//! The MIME half does not move: `turnloop_smtp::message` re-exports the same
//! `lettre` 0.11 `Message` builder Perry's nodemailer surface already used, so
//! the bytes on the wire are produced by the same code as before and only the
//! transport changed.
//!
//! # Ids and the subsystem slot
//!
//! Slot 3, with its own id band (`ID_BASE = 1 << 45`), disjoint from the HTTP
//! client engine's (`1 << 40`) and from both handle bands that end at
//! `0x40000`. See `turnloop_client`'s module note for why that matters:
//! `turnloop_net` keys every handle on a thread in ONE map.
//!
//! # GC
//!
//! No JS value reaches the engine. A job carries owned `String`s and the
//! rendered message bytes; `ctx` is the pinned promise address from
//! `js_promise_new_cross_thread` (#9552). Nothing is rooted here, so this
//! module registers no root scanner.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use perry_runtime::turnloop_net as tl;
use turnloop_smtp::{Config, Connection, Envelope, Event, SendInfo, State, Tls};

mod ffi;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use ffi::*;

/// `turnloop_net` subsystem slot. 0 is `perry-ext-net`, 1 is reserved for the
/// bundled stdlib `net`, 2 is P6's HTTP client.
const SUBSYSTEM: u8 = 3;

/// Private id band, disjoint from every other subsystem's. See the module note.
const ID_BASE: i64 = 1 << 45;

/// The band start, so the HTTP engine's disjointness test can name it.
#[cfg(test)]
pub(crate) fn id_base_for_test() -> i64 {
    ID_BASE
}
const ID_CEILING: i64 = 1 << 50;

static SUBMITTED: AtomicU64 = AtomicU64::new(0);
static DECLINED: AtomicU64 = AtomicU64::new(0);
static SENT: AtomicU64 = AtomicU64::new(0);
static FAILED: AtomicU64 = AtomicU64::new(0);
static UPGRADED: AtomicU64 = AtomicU64::new(0);
static PUMP_EXHAUSTED: AtomicU64 = AtomicU64::new(0);

/// Events one `pump` will drain before deciding the state machine is not
/// converging. One exchange produces a handful.
const PUMP_EVENT_BUDGET: usize = 64;

/// How many exchanges were abandoned because [`pump`] ran out of budget. A
/// nonzero value is a bug in this engine or in `turnloop-smtp`, never a
/// workload property — which is why it is printed rather than logged.
pub fn pump_exhausted() -> u64 {
    PUMP_EXHAUSTED.load(Ordering::Relaxed)
}

/// `PERRY_LOOP_STATS`'s P6 SMTP line.
pub fn stats_line() -> String {
    format!(
        "[perry-loop] p6 smtp_submitted={} declined={} sent={} failed={} tls_upgrades={} \
         pump_exhausted={}",
        SUBMITTED.load(Ordering::Relaxed),
        DECLINED.load(Ordering::Relaxed),
        SENT.load(Ordering::Relaxed),
        FAILED.load(Ordering::Relaxed),
        UPGRADED.load(Ordering::Relaxed),
        PUMP_EXHAUSTED.load(Ordering::Relaxed),
    )
}

/// Whether this phase carried any exchange at all.
pub fn submitted_total() -> u64 {
    SUBMITTED.load(Ordering::Relaxed)
}

/// Exchanges still outstanding on this thread, read by the keep-alive gate.
pub fn has_pending() -> bool {
    STATE.with(|s| !s.borrow().conns.is_empty())
}

/// Why a submission could not be served here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Declined {
    /// This agent has no loop (a worker agent).
    NoLoop,
    /// TLS is required but the client configuration could not be built.
    NoTls,
    /// The host or the client name is not something SMTP can carry.
    Invalid,
}

fn note_declined() {
    DECLINED.fetch_add(1, Ordering::Relaxed);
}

// ── What a caller hands in ─────────────────────────────────────────────────

/// One transporter's configuration, materialized before submission.
#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    /// `true` is nodemailer's `secure: true` — TLS from the first byte.
    /// `false` uses STARTTLS when the server offers it, which is what
    /// `lettre`'s `starttls_relay` did.
    pub implicit_tls: bool,
    /// Refuse to continue in the clear when the server does not offer
    /// STARTTLS. False keeps `lettre`'s opportunistic behaviour.
    pub require_tls: bool,
    pub user: Option<String>,
    pub pass: Option<String>,
    /// The EHLO name. `[127.0.0.1]` is turnloop-smtp's own default and what
    /// nodemailer sends when it cannot determine a hostname.
    pub client_name: String,
}

/// One message to deliver.
pub struct MailJob {
    pub from: String,
    pub to: Vec<String>,
    pub message_id: String,
    /// The rendered RFC 5322 message. Produced by the same `lettre` builder as
    /// before; `Connection::send` applies dot-stuffing itself.
    pub message: Vec<u8>,
}

/// The result of an exchange.
pub enum Outcome {
    /// A message was accepted. Carries turnloop-smtp's own `SendInfo`, which
    /// is richer than the two-field object the lettre path produced.
    Sent(Box<SendInfo>),
    /// `verify()` reached `Ready` — the connection, TLS and credentials are
    /// all good.
    Verified,
    Err(SmtpError),
}

/// A failure in the shape nodemailer reports one.
#[derive(Clone, Debug)]
pub struct SmtpError {
    /// nodemailer's `err.code` — `ECONNECTION`, `EAUTH`, `EENVELOPE`,
    /// `EMESSAGE`, `ESOCKET`, `EPROTOCOL`.
    pub code: &'static str,
    pub message: String,
    /// The SMTP reply code, when the failure came from one.
    pub response_code: Option<u16>,
    pub response: String,
    /// The command that failed (`MAIL FROM`, `AUTH`, …).
    pub command: &'static str,
}

impl SmtpError {
    fn from_protocol(error: turnloop_smtp::Error) -> Self {
        Self {
            code: intern_code(error.code),
            message: error.message,
            response_code: error.response_code,
            response: error.response,
            command: error.command,
        }
    }

    fn transport(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            response_code: None,
            response: String::new(),
            command: "CONN",
        }
    }
}

/// turnloop-smtp's codes are already `&'static str`s from one table; this
/// re-interns the borrowed one so it can reach the runtime's error-diagnostics
/// registry, which takes a `&'static str`.
fn intern_code(code: &str) -> &'static str {
    const CODES: &[&str] = &[
        "EAUTH",
        "ECONNECTION",
        "EENVELOPE",
        "EINVAL",
        "EMESSAGE",
        "EPROTOCOL",
        "ESOCKET",
        "ESTATE",
    ];
    CODES
        .iter()
        .find(|known| **known == code)
        .copied()
        .unwrap_or("ESOCKET")
}

/// Where an outcome goes. A plain `fn` for the same reason the HTTP engine's
/// sink is one: nothing in a thread-local table may be something a moving
/// collector could invalidate.
#[derive(Clone, Copy)]
pub struct Sink {
    pub ctx: usize,
    pub on_done: fn(usize, Outcome),
}

/// What the exchange is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Send,
    Verify,
}

// ── Engine state ───────────────────────────────────────────────────────────

struct Exchange {
    conn: Connection,
    tls: Option<Box<crate::turnloop_tls_client::TlsClientSession>>,
    host: String,
    require_tls: bool,
    purpose: Purpose,
    job: Option<MailJob>,
    sink: Sink,
    /// The message has been handed to `Connection::send`; a `Ready` after this
    /// is the post-send reset, not the moment to send again.
    dispatched: bool,
    delivered: bool,
    closing: bool,
}

#[derive(Default)]
struct EngineState {
    registered: bool,
    conns: HashMap<i64, Exchange>,
    next_id: i64,
    pending: Vec<(Sink, Outcome)>,
    draining: bool,
}

thread_local! {
    static STATE: RefCell<EngineState> = RefCell::new(EngineState::default());
}

impl EngineState {
    fn alloc_id(&mut self) -> i64 {
        loop {
            if self.next_id < ID_BASE || self.next_id >= ID_CEILING {
                self.next_id = ID_BASE;
            }
            let id = self.next_id;
            self.next_id += 1;
            if !self.conns.contains_key(&id) {
                return id;
            }
        }
    }
}

fn ensure_registered(state: &mut EngineState) -> bool {
    if state.registered {
        return true;
    }
    extern "C" fn no_accept() -> i64 {
        0
    }
    state.registered = tl::register_sink(SUBSYSTEM, sink, no_accept);
    if state.registered {
        perry_runtime::event_pump::register_stats_reporter(print_stats);
        // The keep-alive contributor — see the HTTP engine's note. An exchange
        // in flight is work the process owes an answer for.
        //
        // SAFETY: a plain registration with a `'static` function pointer.
        unsafe { js_register_aux_has_active(aux_has_active) };
    }
    state.registered
}

unsafe extern "C" {
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
}

extern "C" fn aux_has_active() -> i32 {
    i32::from(has_pending())
}

extern "C" fn print_stats() {
    eprintln!("{}", stats_line());
}

extern "C" fn sink(completion: *const tl::NetCompletion) {
    // SAFETY: `turnloop_net::dispatch` borrows a live completion for this call.
    let c = unsafe { &*completion };
    let bytes = unsafe { c.bytes() };
    let code = unsafe { c.code_str() };
    STATE.with(|s| {
        let mut state = s.borrow_mut();
        match c.kind {
            tl::NET_CONNECT => on_connect(&mut state, c.id),
            tl::NET_DATA => on_data(&mut state, c.id, bytes),
            tl::NET_EOF => on_eof(&mut state, c.id),
            tl::NET_ERROR => {
                let message = code.unwrap_or("socket error").to_string();
                fail(&mut state, c.id, SmtpError::transport("ESOCKET", message));
            }
            tl::NET_CLOSED => on_closed(&mut state, c.id),
            _ => {}
        }
    });
    drain_pending();
}

fn drain_pending() {
    let already = STATE.with(|s| {
        let mut state = s.borrow_mut();
        if state.draining {
            return true;
        }
        state.draining = !state.pending.is_empty();
        !state.draining
    });
    if already {
        return;
    }
    loop {
        let next = STATE.with(|s| {
            let mut state = s.borrow_mut();
            let next = state.pending.pop();
            if next.is_none() {
                state.draining = false;
            }
            next
        });
        let Some((sink, outcome)) = next else { break };
        (sink.on_done)(sink.ctx, outcome);
    }
}

// ── Submission ─────────────────────────────────────────────────────────────

/// Deliver one message. `Err` means the caller keeps its `lettre` transport.
pub fn send(config: &SmtpConfig, job: MailJob, sink: Sink) -> Result<(), Declined> {
    start(config, Purpose::Send, Some(job), sink)
}

/// `transporter.verify()`: connect, negotiate TLS, authenticate, and report.
pub fn verify(config: &SmtpConfig, sink: Sink) -> Result<(), Declined> {
    start(config, Purpose::Verify, None, sink)
}

fn start(
    config: &SmtpConfig,
    purpose: Purpose,
    job: Option<MailJob>,
    sink: Sink,
) -> Result<(), Declined> {
    if !tl::available() {
        note_declined();
        return Err(Declined::NoLoop);
    }
    if config.host.is_empty() {
        note_declined();
        return Err(Declined::Invalid);
    }
    let tls_mode = if config.implicit_tls {
        Tls::Implicit
    } else if config.require_tls {
        Tls::Required
    } else {
        Tls::Opportunistic
    };
    let needs_tls = tls_mode != Tls::None;
    if needs_tls && crate::turnloop_tls_client::client_config().is_none() {
        note_declined();
        return Err(Declined::NoTls);
    }
    let auth = match (config.user.as_ref(), config.pass.as_ref()) {
        (Some(user), Some(password)) => Some(turnloop_smtp::Auth::Plain {
            user: user.clone(),
            password: password.clone(),
        }),
        _ => None,
    };
    let conn = Connection::new(Config {
        name: config.client_name.clone(),
        tls: tls_mode,
        auth,
        ..Config::default()
    })
    .map_err(|_| {
        note_declined();
        Declined::Invalid
    })?;

    let id = STATE.with(|s| {
        let mut state = s.borrow_mut();
        if !ensure_registered(&mut state) {
            return None;
        }
        let id = state.alloc_id();
        state.conns.insert(
            id,
            Exchange {
                conn,
                tls: None,
                host: config.host.clone(),
                require_tls: config.require_tls,
                purpose,
                job,
                sink,
                dispatched: false,
                delivered: false,
                closing: false,
            },
        );
        Some(id)
    });
    let Some(id) = id else {
        note_declined();
        return Err(Declined::NoLoop);
    };
    SUBMITTED.fetch_add(1, Ordering::Relaxed);
    if let Err(err) = tl::tcp_connect_host(id, SUBSYSTEM, &config.host, config.port, true) {
        STATE.with(|s| {
            let mut state = s.borrow_mut();
            let message = format!("{} {}", err.syscall, err.code);
            fail(&mut state, id, SmtpError::transport("ECONNECTION", message));
        });
        drain_pending();
        return Ok(());
    }
    drain_pending();
    Ok(())
}

// ── The exchange ───────────────────────────────────────────────────────────

fn on_connect(state: &mut EngineState, id: i64) {
    if let Err(err) = tl::read_start(id) {
        let message = format!("{} {}", err.syscall, err.code);
        fail(state, id, SmtpError::transport("ECONNECTION", message));
        return;
    }
    let now = Instant::now();
    let result = state
        .conns
        .get_mut(&id)
        .map(|exchange| exchange.conn.connected(now));
    match result {
        Some(Err(e)) => fail(state, id, SmtpError::from_protocol(e)),
        Some(Ok(())) => pump(state, id),
        None => {}
    }
}

fn on_data(state: &mut EngineState, id: i64, bytes: &[u8]) {
    let Some(exchange) = state.conns.get_mut(&id) else {
        return;
    };
    if exchange.closing {
        return;
    }
    let plaintext = match exchange.tls.as_mut() {
        Some(session) => {
            session.receive(bytes);
            session.pump();
            if let Some((code, text)) = session.failure() {
                let error = SmtpError {
                    code: intern_code(code),
                    message: text.to_string(),
                    response_code: None,
                    response: String::new(),
                    command: "STARTTLS",
                };
                fail(state, id, error);
                return;
            }
            let handshaking = session.is_handshaking();
            let plaintext = session.take_plaintext();
            let out = session.take_output();
            if !out.is_empty() && tl::write(id, out, 0).is_err() {
                fail(
                    state,
                    id,
                    SmtpError::transport("ESOCKET", "write failed".to_string()),
                );
                return;
            }
            if !handshaking {
                let now = Instant::now();
                let established = state.conns.get_mut(&id).is_some_and(|exchange| {
                    exchange.conn.state() == State::Tls
                        && exchange.conn.tls_established(now).is_ok()
                });
                if established {
                    UPGRADED.fetch_add(1, Ordering::Relaxed);
                    pump(state, id);
                }
            }
            plaintext
        }
        None => bytes.to_vec(),
    };
    if plaintext.is_empty() {
        return;
    }
    let now = Instant::now();
    let result = state
        .conns
        .get_mut(&id)
        .map(|exchange| exchange.conn.receive(&plaintext, now));
    match result {
        Some(Err(e)) => {
            fail(state, id, SmtpError::from_protocol(e));
            return;
        }
        Some(Ok(())) => {}
        None => return,
    }
    pump(state, id);
}

/// Drain the connection's events and its output, repeatedly, until neither
/// produces anything.
///
/// The iteration bound is a guard against a protocol state this code did not
/// anticipate, not an expectation: sixty-four is far more events than one
/// exchange produces. But *falling out of the loop* with events still queued
/// would be a silent stall — nothing else calls `pump` until the next
/// `NET_DATA`, and a state machine that has more to say without more bytes
/// would never get another one, leaving the exchange undelivered and its
/// `Exchange` in `conns` forever. So exhaustion is reported as a failure
/// instead. `pump_exhausted()` is a live counter so a run can say this never
/// happened rather than assume it.
fn pump(state: &mut EngineState, id: i64) {
    for _ in 0..PUMP_EVENT_BUDGET {
        let event = state
            .conns
            .get_mut(&id)
            .and_then(|exchange| exchange.conn.poll_event());
        match event {
            Some(Event::UpgradeTls) => {
                if !upgrade_tls(state, id) {
                    return;
                }
            }
            Some(Event::Ready) => {
                if !on_ready(state, id) {
                    return;
                }
            }
            Some(Event::Sent { info, .. }) => {
                SENT.fetch_add(1, Ordering::Relaxed);
                deliver(state, id, Outcome::Sent(Box::new(info)));
                quit(state, id);
            }
            Some(Event::Failed { error, .. }) => {
                deliver(state, id, Outcome::Err(SmtpError::from_protocol(error)));
                quit(state, id);
            }
            Some(Event::CloseTransport | Event::Closed) => {
                close(state, id);
                return;
            }
            Some(Event::Reset) => {}
            None => {
                flush(state, id);
                return;
            }
        }
    }
    // The budget ran out with events still queued. Reporting it is the whole
    // point: the alternative is an exchange nothing will ever settle.
    PUMP_EXHAUSTED.fetch_add(1, Ordering::Relaxed);
    fail(
        state,
        id,
        SmtpError::transport(
            "EPROTOCOL",
            "SMTP event budget exhausted; the exchange was abandoned rather \
             than left unsettled",
        ),
    );
}

fn on_ready(state: &mut EngineState, id: i64) -> bool {
    let Some(exchange) = state.conns.get_mut(&id) else {
        return false;
    };
    // `Required` is enforced here rather than by the protocol crate: it reaches
    // `Ready` in the clear when the server offered no STARTTLS, and a caller
    // that asked for TLS must see a failure rather than a plaintext delivery.
    if exchange.require_tls && exchange.tls.is_none() {
        let error = SmtpError {
            code: "ESOCKET",
            message: "STARTTLS is required but the server does not offer it".to_string(),
            response_code: None,
            response: String::new(),
            command: "STARTTLS",
        };
        fail(state, id, error);
        return false;
    }
    if exchange.purpose == Purpose::Verify {
        deliver(state, id, Outcome::Verified);
        quit(state, id);
        return false;
    }
    if exchange.dispatched {
        // The post-send `Ready`: the exchange is finished and `quit` has
        // already been submitted.
        return true;
    }
    let Some(job) = exchange.job.take() else {
        return true;
    };
    exchange.dispatched = true;
    let now = Instant::now();
    let envelope = Envelope {
        from: job.from,
        to: job.to,
    };
    let result = exchange
        .conn
        .send(1, envelope, job.message_id, &job.message, now);
    if let Err(e) = result {
        fail(state, id, SmtpError::from_protocol(e));
        return false;
    }
    true
}

fn upgrade_tls(state: &mut EngineState, id: i64) -> bool {
    // Flush whatever the protocol queued (the `STARTTLS` command itself) before
    // the session is installed: those bytes are still cleartext.
    flush(state, id);
    let Some(exchange) = state.conns.get_mut(&id) else {
        return false;
    };
    let Some(config) = crate::turnloop_tls_client::client_config() else {
        fail(
            state,
            id,
            SmtpError::transport("ESOCKET", "TLS client configuration unavailable"),
        );
        return false;
    };
    let name = match crate::turnloop_tls_client::server_name(&exchange.host) {
        Ok(name) => name,
        Err(message) => {
            fail(state, id, SmtpError::transport("ESOCKET", message));
            return false;
        }
    };
    match crate::turnloop_tls_client::TlsClientSession::new(config, name) {
        Ok(mut session) => {
            session.pump();
            let out = session.take_output();
            exchange.tls = Some(Box::new(session));
            if !out.is_empty() && tl::write(id, out, 0).is_err() {
                fail(state, id, SmtpError::transport("ESOCKET", "write failed"));
                return false;
            }
            true
        }
        Err(message) => {
            fail(state, id, SmtpError::transport("ESOCKET", message));
            false
        }
    }
}

/// Move whatever the protocol has produced onto the socket, through the TLS
/// session when one is installed.
fn flush(state: &mut EngineState, id: i64) {
    let Some(exchange) = state.conns.get_mut(&id) else {
        return;
    };
    if exchange.closing {
        return;
    }
    let plain = exchange.conn.output().to_vec();
    if !plain.is_empty() {
        exchange.conn.consume_output(plain.len());
    }
    let out = match exchange.tls.as_mut() {
        Some(session) => {
            if !plain.is_empty() {
                session.write(&plain);
            }
            session.pump();
            session.take_output()
        }
        None => plain,
    };
    if out.is_empty() {
        return;
    }
    if tl::write(id, out, 0).is_err() {
        fail(state, id, SmtpError::transport("ESOCKET", "write failed"));
    }
}

fn quit(state: &mut EngineState, id: i64) {
    let now = Instant::now();
    if let Some(exchange) = state.conns.get_mut(&id) {
        let _ = exchange.conn.quit(now);
    }
    flush(state, id);
    close(state, id);
}

fn on_eof(state: &mut EngineState, id: i64) {
    let delivered = state
        .conns
        .get(&id)
        .is_some_and(|exchange| exchange.delivered);
    if let Some(exchange) = state.conns.get_mut(&id) {
        exchange.conn.transport_lost();
    }
    if !delivered {
        fail(
            state,
            id,
            SmtpError::transport("ECONNECTION", "Connection closed unexpectedly"),
        );
    } else {
        close(state, id);
    }
}

fn close(state: &mut EngineState, id: i64) {
    let Some(exchange) = state.conns.get_mut(&id) else {
        return;
    };
    if exchange.closing {
        return;
    }
    exchange.closing = true;
    exchange.conn.close();
    if let Some(session) = exchange.tls.as_mut() {
        session.close_notify();
        session.pump();
        let out = session.take_output();
        if !out.is_empty() {
            let _ = tl::write(id, out, 0);
        }
    }
    if tl::close(id).is_err() {
        on_closed(state, id);
    }
}

fn on_closed(state: &mut EngineState, id: i64) {
    let Some(exchange) = state.conns.remove(&id) else {
        return;
    };
    if !exchange.delivered {
        let sink = exchange.sink;
        FAILED.fetch_add(1, Ordering::Relaxed);
        state.pending.push((
            sink,
            Outcome::Err(SmtpError::transport(
                "ECONNECTION",
                "Connection closed unexpectedly",
            )),
        ));
    }
}

fn fail(state: &mut EngineState, id: i64, error: SmtpError) {
    deliver(state, id, Outcome::Err(error));
    close(state, id);
}

/// Queue the outcome for `drain_pending`, exactly once per exchange
/// (DESIGN D4).
fn deliver(state: &mut EngineState, id: i64, outcome: Outcome) {
    let Some(exchange) = state.conns.get_mut(&id) else {
        return;
    };
    if exchange.delivered {
        return;
    }
    exchange.delivered = true;
    if matches!(outcome, Outcome::Err(_)) {
        FAILED.fetch_add(1, Ordering::Relaxed);
    }
    let sink = exchange.sink;
    state.pending.push((sink, outcome));
}
