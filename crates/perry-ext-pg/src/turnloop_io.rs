//! `pg` on a turnloop socket (P7).
//!
//! What this replaces, one for one:
//!
//! | before | after |
//! |---|---|
//! | `spawn_blocking` + `Handle::current().block_on` per call — one tokio blocking-pool thread held for the whole round trip | one `execute()` on a sans-I/O core, submitted where the FFI call happens |
//! | `sqlx::PgConnection`, which owns the socket and drives it on tokio | `turnloop_postgres::Connection` driven over P1's `turnloop_net` |
//! | `sqlx::PgPool`, ten tokio-owned connections checked out per call | one loop-driven connection, pipelined (see "The pool" below) |
//!
//! The JS-visible surface does not move: the same ten `js_pg_*` symbols, the
//! same `{ rows, fields, rowCount, command }` result object, the same rejection
//! prefixes (`Failed to connect: `, `Failed to create pool: `, `Query failed: `,
//! `Invalid client handle`, `Connection already closed`).
//!
//! # Which connections come here
//!
//! [`enabled`] is false on a `worker_threads` agent (no loop of its own) and in
//! the `tokio-wait-driver` A/B arm. [`supports`] additionally declines a
//! **Unix-domain-socket host** (one whose `host` is a path): `Registry::connect`
//! submits a TCP connect, and reaching a socket file needs `pipe_connect`.
//! Those clients keep the sqlx path, where they work exactly as they do today.
//!
//! # TLS, and SCRAM-SHA-256-PLUS
//!
//! A config with an `ssl` option connects with [`SslMode::Require`]: the core
//! sends an `SSLRequest`, the driver installs a client session on the same
//! turnloop handle when the server answers `S`, and a server that answers `N`
//! fails the connection rather than continuing in the clear. There is no
//! `prefer` mode — a client that asked for TLS and silently got none would send
//! its password in plaintext, which is the one outcome worse than a refused
//! connection.
//!
//! Channel binding comes with it. `turnloop-postgres` offers
//! **SCRAM-SHA-256-PLUS** only when the host says it can supply RFC 5929
//! `tls-server-end-point` data, and then cross-checks that the `ScramSha256`
//! the host builds really carries a `p=tls-server-end-point,` GS2 header. So
//! [`PgCore::tls_established`] passes `facts.channel_binding.is_some()` through
//! honestly and keeps the digest for [`Step::ScramNeeded`]; a leaf whose
//! signature algorithm has no defined binding (Ed25519) reports `false` and
//! authenticates with plain SCRAM-SHA-256, which is what the server offers in
//! that case anyway.
//!
//! The **legacy** sqlx path still has no TLS — this crate's sqlx dependency is
//! built without a backend — so a client that declines to it (a Unix-socket
//! host, or a thread with no loop) fails exactly as it does today.
//!
//! # Authentication
//!
//! `AuthenticationSASL` arrives as [`Event::ScramNeeded`], and the core cannot
//! answer it itself: `ScramSha256::new` reads entropy, which a sans-I/O crate
//! must not do. The host constructs it here and hands it back through
//! `start_scram`. Channel binding is [`ChannelBinding::unsupported`] because
//! there is no TLS to bind to; a `plus` request (which the core only raises when
//! TLS *is* established) is refused rather than answered with a bogus binding.
//! MD5 and cleartext password auth are handled inside the core.
//!
//! # The pool
//!
//! `Pool` here is **one connection, opened lazily and reused**, with commands
//! pipelined onto it — not a pool of ten. That is a deliberate, stated
//! limitation, not an oversight:
//!
//! * the sqlx pool checked a connection out *per call*, so `pool.query('BEGIN')`
//!   was already unreliable — a transaction opened on one checkout and used from
//!   the next is a different session. One connection does not make that worse.
//! * `turnloop_postgres` gives every operation its own Sync, so N in-flight
//!   `pool.query()` calls pipeline on one socket and one statement's error does
//!   not discard the others.
//!
//! What it costs is server-side parallelism: ten concurrent slow queries now
//! serialize behind each other rather than running on ten backends.
//! `turnloop_postgres::pool` exists and would close that gap; wiring it needs
//! host-executed Connect/Close events and is its own change.
//!
//! A `Client`, by contrast, pins one connection for life, exactly as before, so
//! `client.query('BEGIN')` keeps working.
//!
//! # Threading and the GC
//!
//! The sink runs on the agent thread from the loop's own completion dispatch, so
//! it may touch the connection table directly. It builds **no JS value**: every
//! row is copied out of the core's receive buffer into the owned `Cell`s of
//! [`mod@result`], and the result object is built inside a
//! `JsPromise::resolve_with` closure that the resolution pump runs on the main
//! thread. That is the #1824 rule — and it is what this module fixes for `pg`,
//! whose sqlx path builds `rows`, `fields` and every row object *on the
//! blocking-pool thread* before resolving.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};

use perry_db_turnloop::{subsystem, DbCore, NetCompletion, Registry, TlsClientOptions, TlsFacts};
use perry_ffi::{register_handle, Handle, JsPromise, JsValue};
use turnloop_postgres::{
    ChannelBinding, Config, Connection, Error, Event, ExtendedQuery, Instant, Outcome, Parameter,
    ScramSha256, SslMode, Token,
};

use crate::{ParamValue, PgConfig, PgConnectionHandle, PgPoolHandle};

mod result;
use result::{cells_from_row, columns_from_fields, Cell, ColumnMeta, QueryResult};

/// This binding's slot in the runtime's sink registry.
pub(crate) const SUBSYSTEM: u8 = subsystem::PG;

/// Rejection prefixes. These are the strings the sqlx path already produces and
/// the reason they are constants is that a caller may be matching on them: the
/// *detail* after the colon necessarily changes with the driver, the prefix
/// must not.
pub(crate) const CONNECT_FAILURE: &str = "Failed to connect";
pub(crate) const POOL_FAILURE: &str = "Failed to create pool";
const QUERY_FAILURE: &str = "Query failed";
const CLOSED: &str = "Connection already closed";
const INVALID_CLIENT: &str = "Invalid client handle";
const INVALID_POOL: &str = "Invalid pool handle";

/// PostgreSQL's text wire format, for both parameters and results.
///
/// Requesting text results keeps decoding stable and format-independent: the
/// same `types::decode` call handles every OID, and a value's spelling is the
/// one `psql` would print. Binary would need a per-type encoder and buys nothing
/// for the five scalar families this binding converts.
const TEXT_FORMAT: i16 = 0;

thread_local! {
    /// The connection table. Thread-local because a turnloop handle belongs to
    /// the loop that created it — see `perry_db_turnloop`'s module docs.
    static REGISTRY: Registry<PgCore> = Registry::new(SUBSYSTEM);
    /// JS-visible handle (a `Client` or a `Pool`) → the driver id of its
    /// connection. Absent until something opens one: for a `Client` that is
    /// `connect()`, for a `Pool` the first `query()`.
    static OPEN: RefCell<HashMap<Handle, i64>> = RefCell::new(HashMap::new());
}

/// Which of pg's two result shapes a statement produces.
///
/// The distinction is not cosmetic and is not the server's: the sqlx path chose
/// between `fetch_all` and `execute` from the *SQL text*, and the two fill
/// `rowCount` from different sources. Reproducing the choice is what keeps
/// `INSERT` reporting affected rows and `SELECT` reporting returned rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ResultKind {
    /// `rowCount` is the number of rows collected. What `fetch_all` produced —
    /// note this is why a non-parameterised `client.query('INSERT …')` reports
    /// `rowCount: 0` today rather than the affected count.
    Rows,
    /// `rows: []`, `fields: []`, `rowCount` = the server's affected-row count.
    /// What `execute()` produced.
    RowsAffected,
}

impl ResultKind {
    /// The choice `js_pg_client_query_params` makes, from the same classifier.
    pub(crate) fn for_sql(sql: &str) -> Self {
        if crate::is_row_returning_query(sql) {
            Self::Rows
        } else {
            Self::RowsAffected
        }
    }
}

/// One statement, owned, ready to submit or to queue.
pub(crate) struct Statement {
    pub(crate) sql: String,
    pub(crate) params: Vec<ParamValue>,
    pub(crate) kind: ResultKind,
    /// The first word of the SQL, uppercased — see [`QueryResult::command`].
    pub(crate) command: String,
}

/// A statement plus the promise that will answer it.
struct QueuedQuery {
    statement: Statement,
    promise: JsPromise,
}

/// What a parked connect-time promise resolves with.
enum ReadyValue {
    /// `client.connect()` resolves `undefined`.
    Undefined,
    /// `pg.connect(config)` resolves the handle it pre-registered.
    ClientHandle(Handle),
    /// `pg.createPool(config)` resolves the handle it pre-registered.
    PoolHandle(Handle),
}

/// One outstanding statement, accumulating its rows.
struct PendingOp {
    promise: JsPromise,
    command: String,
    kind: ResultKind,
    columns: Vec<ColumnMeta>,
    rows: Vec<Vec<Cell>>,
    rows_affected: u64,
    /// The server's `ErrorResponse` message for this statement. It arrives
    /// *before* the `Completed { ServerError }` that settles the promise, so it
    /// has to be held here — without it the rejection would carry no reason.
    server_error: Option<String>,
}

/// The sans-I/O half of one `pg` connection.
pub(crate) struct PgCore {
    conn: Connection,
    /// Kept because `ScramSha256` is built in the host, on demand, and the core
    /// does not expose the `Config` it owns.
    password: Vec<u8>,
    /// The prefix a failure carries while the handshake is still running —
    /// `Failed to connect` for a `Client`, `Failed to create pool` for a `Pool`.
    connect_failure: &'static str,
    /// Statements submitted to the core, oldest first. A `VecDeque` rather than
    /// a map so that a mass rejection settles promises in submission order,
    /// which is the order a caller's `.catch` handlers run in.
    pending: VecDeque<(Token, PendingOp)>,
    /// Statements that arrived before the handshake finished. Only a `Pool`
    /// fills this (see [`PgCore::queue_offline`]).
    offline: VecDeque<QueuedQuery>,
    /// `connect()` callers waiting for the handshake.
    waiting_ready: Vec<(JsPromise, ReadyValue)>,
    /// Whether a statement submitted before `Connected` waits or is refused.
    /// A `Pool`'s only entry point is `query()`, so it must wait; a `Client`
    /// holds no connection until `connect()` resolves, so it must be refused
    /// with the message the sqlx path gives for exactly that state.
    queue_offline: bool,
    next_token: Token,
    ready: bool,
    finished: bool,
    /// The first failure observed, kept as the reason for everything this
    /// connection still owes. First rather than last on purpose: the driver's
    /// later "Connection closed" is a consequence, and reporting it would hide
    /// the `28P01` or `ECONNREFUSED` that actually explains the failure.
    transport_failure: Option<String>,
    /// The core answered `S` to its `SSLRequest` and wants the transport
    /// upgraded. Taken by the driver, which flushes the pending plaintext and
    /// installs the session.
    tls_requested: bool,
    /// RFC 5929 `tls-server-end-point` over the verified leaf, kept from the
    /// handshake because `ScramSha256` is built here, later, and by then the
    /// facts are no longer in hand.
    channel_binding: Option<Vec<u8>>,
}

/// The protocol config for one connection.
///
/// Separated from [`PgCore::new`] so the choices below are assertable without a
/// server: the SSL mode the JS `ssl` option asks for, and PostgreSQL's own
/// default of "the database is named after the user" when the JS config omits
/// `database` — which is what a sqlx URL with no path component did.
fn turnloop_config(config: &PgConfig) -> Config {
    Config {
        user: config.user.clone(),
        password: config.password.clone().into_bytes(),
        database: config
            .database
            .clone()
            .unwrap_or_else(|| config.user.clone()),
        // The core always sends `application_name`; sqlx sent it only when the
        // URL carried one, and Perry's URL never does. Empty is the closest
        // equivalent — it leaves `pg_stat_activity.application_name` blank, as
        // today. Not JS-visible either way.
        application_name: String::new(),
        // `Require`, never `Prefer`: `Prefer` lets the server answer `N` and
        // the core continue in plaintext, which would turn "I asked for TLS"
        // into "I sent my password in the clear" without telling anyone.
        ssl: if config.ssl.is_some() {
            SslMode::Require
        } else {
            SslMode::Disable
        },
        // Not required: a server that offers only SCRAM-SHA-256 (or a leaf
        // whose algorithm has no RFC 5929 binding) must still authenticate.
        // The core still refuses to *downgrade* — it asks for PLUS only when
        // the host said a binding is available.
        channel_binding_required: false,
        ..Config::default()
    }
}

/// The TLS options the driver installs when the core asks for the upgrade.
///
/// `None` for a plaintext config, which is what makes an `UpgradeTls` request
/// on such a connection a driver error rather than a silent plaintext
/// continuation.
fn tls_options(config: &PgConfig) -> Option<TlsClientOptions> {
    let ssl = config.ssl.as_ref()?;
    let servername = ssl
        .servername
        .clone()
        .unwrap_or_else(|| config.host.clone());
    let mut options = TlsClientOptions::from_node_environment(servername);
    options.reject_unauthorized = ssl.reject_unauthorized;
    options.ca_pem = ssl.ca.clone();
    // No ALPN: PostgreSQL's TLS carries the PostgreSQL protocol and nothing
    // else, and offering a protocol list a server has no opinion about is how
    // a middlebox learns to have one.
    Some(options)
}

/// Which channel binding a SCRAM exchange carries.
///
/// Split out of [`PgCore::apply`] so the downgrade guard is assertable without
/// a server. The rule it encodes: `plus` is the CORE's decision, taken from
/// what the server offered and from what [`PgCore::tls_established`] said was
/// available, so a `plus` request with no digest in hand is a bug in this file
/// — and answering it with `ChannelBinding::unsupported()` would be a silent
/// channel-binding downgrade, which is the attack RFC 5802's `p=` header
/// exists to prevent.
fn scram_channel(plus: bool, binding: Option<&[u8]>) -> Result<ChannelBinding, String> {
    if !plus {
        return Ok(ChannelBinding::unsupported());
    }
    match binding {
        Some(binding) => Ok(ChannelBinding::tls_server_end_point(binding.to_vec())),
        None => Err(
            "PostgreSQL SCRAM-SHA-256-PLUS needs tls-server-end-point channel binding, which this connection has none of"
                .to_string(),
        ),
    }
}

/// `PERRY_DB_TURNLOOP_DIAG=1` also prints the SCRAM mechanism.
///
/// Same knob as the driver's, deliberately: a reader debugging a database
/// connection should not have to discover a second one, and the mechanism is
/// only interesting next to the `tls established … channel_binding=` line the
/// driver prints from the same variable.
fn scram_diag() -> bool {
    matches!(
        std::env::var("PERRY_DB_TURNLOOP_DIAG").as_deref(),
        Ok("1") | Ok("on") | Ok("true")
    )
}

impl PgCore {
    fn new(
        config: &PgConfig,
        connect_failure: &'static str,
        queue_offline: bool,
    ) -> Result<Self, String> {
        let wire = turnloop_config(config);
        let password = wire.password.clone();
        // `Connection::new` already queues the StartupMessage, because this
        // config disables SSL and so has no SSLRequest round trip to make
        // first. The driver flushes it when the connect completion arrives.
        let conn = Connection::new(wire).map_err(|e| e.to_string())?;
        Ok(Self {
            conn,
            password,
            connect_failure,
            pending: VecDeque::new(),
            offline: VecDeque::new(),
            waiting_ready: Vec::new(),
            queue_offline,
            next_token: 1,
            ready: false,
            finished: false,
            transport_failure: None,
            tls_requested: false,
            channel_binding: None,
        })
    }

    /// Submit one statement, taking ownership of the promise that answers it.
    fn submit(&mut self, query: QueuedQuery) -> Result<(), (JsPromise, String)> {
        let QueuedQuery { statement, promise } = query;
        let token = self.next_token;
        self.next_token += 1;
        // Bound to locals so every borrow outlives the `execute` call: the core
        // copies the bytes into its output buffer and keeps nothing.
        let oids: Vec<u32> = statement.params.iter().map(param_oid).collect();
        let encoded: Vec<Option<Vec<u8>>> = statement.params.iter().map(encode_param).collect();
        let params: Vec<Parameter<'_>> = encoded
            .iter()
            .map(|value| Parameter {
                value: value.as_deref(),
                format: TEXT_FORMAT,
            })
            .collect();
        // The extended protocol, always — including for the no-parameter
        // entry points. `sqlx::query(..).fetch_all()` prepares too, so this
        // keeps the one JS-visible consequence: a statement string containing
        // several `;`-separated statements is refused by the server here as it
        // is today, rather than quietly running all of them.
        //
        // The statement name is empty (uncached). sqlx cached prepared
        // statements per connection; not caching costs a Parse per call and
        // avoids having to invalidate the cache when a table changes shape.
        let outcome = self.conn.execute(
            token,
            ExtendedQuery {
                name: "",
                sql: &statement.sql,
                oids: &oids,
                params: &params,
                result_formats: &[TEXT_FORMAT],
            },
            // No deadline: the sqlx path had no statement timeout either (its
            // only timeout was the pool's 30s acquire, which has no analogue
            // here), and inventing one would start rejecting queries that work
            // today.
            None,
        );
        match outcome {
            Ok(()) => {
                self.pending.push_back((
                    token,
                    PendingOp {
                        promise,
                        command: statement.command,
                        kind: statement.kind,
                        columns: Vec::new(),
                        rows: Vec::new(),
                        rows_affected: 0,
                        server_error: None,
                    },
                ));
                Ok(())
            }
            Err(err) => Err((promise, format!("{}: {}", QUERY_FAILURE, err))),
        }
    }

    /// Submit now if the handshake is done, otherwise queue or refuse.
    fn submit_or_queue(&mut self, query: QueuedQuery) -> Result<(), (JsPromise, String)> {
        if self.ready {
            return self.submit(query);
        }
        if self.queue_offline {
            self.offline.push_back(query);
            return Ok(());
        }
        Err((query.promise, CLOSED.to_string()))
    }

    fn pending_mut(&mut self, token: Token) -> Option<&mut PendingOp> {
        self.pending
            .iter_mut()
            .find(|(t, _)| *t == token)
            .map(|(_, op)| op)
    }

    fn take_pending(&mut self, token: Token) -> Option<PendingOp> {
        let index = self.pending.iter().position(|(t, _)| *t == token)?;
        self.pending.remove(index).map(|(_, op)| op)
    }

    /// The prefix a failure carries right now: connect-phase failures are the
    /// caller's `connect()`/`createPool()` failing, everything later is a query
    /// failing.
    fn failure_message(&self, detail: &str) -> String {
        let prefix = if self.ready {
            QUERY_FAILURE
        } else {
            self.connect_failure
        };
        format!("{}: {}", prefix, detail)
    }

    fn record_failure(&mut self, detail: &str) {
        if self.transport_failure.is_none() {
            self.transport_failure = Some(detail.to_string());
        }
    }

    /// Pull one event and copy everything it borrows into owned data.
    ///
    /// Materialising here rather than handling the event in place is what the
    /// crate's contract demands — every event borrows the receive buffer until
    /// the next mutable call — and it is also what lets the handling code take
    /// `&mut self`.
    fn next_step(&mut self) -> Result<Option<Step>, Error> {
        let Some(event) = self.conn.next_event()? else {
            return Ok(None);
        };
        Ok(Some(match event {
            Event::Connected => Step::Connected,
            Event::ScramNeeded { plus } => Step::ScramNeeded { plus },
            Event::UpgradeTls => Step::UpgradeTls,
            Event::Fields { token, fields } => Step::Fields {
                token,
                columns: columns_from_fields(fields)?,
            },
            Event::Row { token, row } => {
                // Reading `self.pending` while `self.conn` is mutably borrowed
                // is a disjoint-field borrow; the OID list has to come from the
                // RowDescription this statement already reported.
                let columns = self
                    .pending
                    .iter()
                    .find(|(t, _)| *t == token)
                    .map(|(_, op)| op.columns.as_slice())
                    .unwrap_or(&[]);
                Step::Row {
                    token,
                    cells: cells_from_row(columns, row)?,
                }
            }
            Event::CommandComplete {
                token, row_count, ..
            } => Step::CommandComplete { token, row_count },
            Event::Error { token, error } => Step::ServerError {
                token,
                // `.message()` is what node-pg surfaces as `err.message`; the
                // remaining ErrorResponse fields (`code`, `detail`, `hint`,
                // `position`) are available and not exposed, because the sqlx
                // path exposed none of them either.
                message: error.message().to_string(),
            },
            Event::Completed { token, outcome, .. } => Step::Completed { token, outcome },
            Event::Closed { .. } => Step::Closed,
            Event::CopyIn { .. } => Step::CopyIn,
            // ParameterStatus, Notice, Notification and the COPY OUT stream are
            // not part of this binding's surface (`pg.Client` here has no
            // `.on('notice')` and no LISTEN). Dropping them is what the sqlx
            // path did; a COPY OUT still completes, with its rows discarded.
            Event::ParameterStatus { .. }
            | Event::Notice(_)
            | Event::Notification { .. }
            | Event::CopyOut { .. }
            | Event::CopyData { .. }
            | Event::CopyDone { .. } => Step::Ignored,
        }))
    }

    /// Handle one materialized event. `Some(message)` is a fatal protocol
    /// error: the driver aborts the connection with it.
    fn apply(&mut self, step: Step) -> Option<String> {
        match step {
            Step::Connected => {
                self.ready = true;
                for (promise, value) in std::mem::take(&mut self.waiting_ready) {
                    resolve_ready(promise, value);
                }
                // Anything a `Pool` queued during the handshake goes out now, in
                // arrival order.
                while let Some(query) = self.offline.pop_front() {
                    if let Err((promise, message)) = self.submit(query) {
                        promise.reject_string(&message);
                    }
                }
            }
            Step::ScramNeeded { plus } => {
                // `plus` is the core's own decision, taken from what the server
                // offered AND from what `tls_established` said was available.
                // Answering a `plus` request with `unsupported()` would be a
                // channel-binding downgrade, so a missing digest here is a
                // failure rather than a fallback — and the core cross-checks
                // the GS2 header anyway, so a mismatch cannot get past it.
                let channel = match scram_channel(plus, self.channel_binding.as_deref()) {
                    Ok(channel) => channel,
                    Err(message) => return Some(message),
                };
                let scram = ScramSha256::new(&self.password, channel);
                if scram_diag() {
                    // The one place a run can say WHICH mechanism it used.
                    // Nothing else does: the server accepts both, the core
                    // decides silently, and a PLUS exchange that silently
                    // became plain SCRAM would look identical from JS. The
                    // `p=` prefix is read off the SCRAM client-first message
                    // itself rather than off `plus`, so this reports what went
                    // on the wire rather than what was intended.
                    eprintln!(
                        "[perry-pg] scram mechanism={} gs2={}",
                        if plus {
                            "SCRAM-SHA-256-PLUS"
                        } else {
                            "SCRAM-SHA-256"
                        },
                        String::from_utf8_lossy(&scram.message()[..scram.message().len().min(24)]),
                    );
                }
                if let Err(err) = self.conn.start_scram(scram) {
                    return Some(err.to_string());
                }
            }
            Step::UpgradeTls => {
                // The hard boundary: no more plaintext is parsed, and the
                // driver installs the session once it has flushed whatever the
                // core still owes. Answering here would be too early — the
                // core refuses `tls_established` while its output is unsent.
                self.tls_requested = true;
            }
            Step::Fields { token, columns } => {
                if let Some(op) = self.pending_mut(token) {
                    op.columns = columns;
                }
            }
            Step::Row { token, cells } => {
                if let Some(op) = self.pending_mut(token) {
                    op.rows.push(cells);
                }
            }
            Step::CommandComplete { token, row_count } => {
                if let Some(op) = self.pending_mut(token) {
                    op.rows_affected = row_count.unwrap_or(0);
                }
            }
            Step::ServerError { token, message } => match token.and_then(|t| self.pending_mut(t)) {
                Some(op) => op.server_error = Some(message),
                // An error with no statement to attach it to is terminal; the
                // core has already moved to closing and will complete every
                // queued token. Keep the diagnostic so those completions carry
                // it instead of a bare "Connection closed".
                None => self.record_failure(&message),
            },
            Step::CopyIn => {
                // `COPY … FROM STDIN` would need the host to stream chunks, and
                // this binding has no surface for that. Failing the copy ends
                // the statement with a server error the caller can read, rather
                // than leaving the connection stuck in the copy-in phase
                // waiting for data that will never arrive.
                if let Err(err) = self
                    .conn
                    .copy_finish(Some("COPY FROM STDIN is not supported by this client"))
                {
                    return Some(err.to_string());
                }
            }
            Step::Completed { token, outcome } => self.settle(token, outcome),
            Step::Closed => {
                self.finished = true;
                // The core accounts for the statements it accepted — by here it
                // has already completed every one of them. The offline queue
                // and the handshake waiters are this module's, so they are
                // settled here rather than left to a later `fail` from the
                // driver: `Registry::close` retires the entry, and if the
                // turnloop handle has already gone no `NET_CLOSED` follows to
                // trigger one. On the ordinary `end()` path both are empty and
                // this does nothing.
                self.settle_all_with_error("Connection closed");
            }
            Step::Ignored => {}
        }
        None
    }

    fn settle(&mut self, token: Token, outcome: Outcome) {
        let Some(op) = self.take_pending(token) else {
            return;
        };
        match outcome {
            Outcome::Success => {
                let row_count = match op.kind {
                    ResultKind::Rows => op.rows.len() as f64,
                    ResultKind::RowsAffected => op.rows_affected as f64,
                };
                let result = QueryResult {
                    columns: op.columns,
                    rows: op.rows,
                    command: op.command,
                    row_count,
                };
                // Owned data only; the closure runs on the main thread.
                op.promise.resolve_with(move || result.into_js());
            }
            Outcome::ServerError => {
                let detail = op
                    .server_error
                    .unwrap_or_else(|| "the server rejected the statement".to_string());
                op.promise
                    .reject_string(&format!("{}: {}", QUERY_FAILURE, detail));
            }
            Outcome::Aborted(err) => {
                let detail = self
                    .transport_failure
                    .clone()
                    .or(op.server_error)
                    .unwrap_or_else(|| err.to_string());
                let message = self.failure_message(&detail);
                op.promise.reject_string(&message);
            }
        }
    }

    /// Settle everything this connection still owes. Leaving a promise pending
    /// is the one outcome a caller cannot recover from.
    fn settle_all_with_error(&mut self, reason: &str) {
        let detail = self
            .transport_failure
            .clone()
            .unwrap_or_else(|| reason.to_string());
        let message = self.failure_message(&detail);
        while let Some((_, op)) = self.pending.pop_front() {
            op.promise.reject_string(&message);
        }
        while let Some(query) = self.offline.pop_front() {
            query.promise.reject_string(&message);
        }
        for (promise, value) in std::mem::take(&mut self.waiting_ready) {
            forget_pre_registered(&value);
            promise.reject_string(&message);
        }
    }
}

/// One protocol event, with everything it borrowed copied out.
enum Step {
    Connected,
    ScramNeeded {
        plus: bool,
    },
    UpgradeTls,
    Fields {
        token: Token,
        columns: Vec<ColumnMeta>,
    },
    Row {
        token: Token,
        cells: Vec<Cell>,
    },
    CommandComplete {
        token: Token,
        row_count: Option<u64>,
    },
    ServerError {
        token: Option<Token>,
        message: String,
    },
    CopyIn,
    Completed {
        token: Token,
        outcome: Outcome,
    },
    Closed,
    Ignored,
}

impl DbCore for PgCore {
    fn transport_connected(&mut self) -> Result<(), String> {
        // Nothing to do: the StartupMessage was queued at construction and the
        // driver flushes it as soon as this returns. A core that negotiated SSL
        // would send its SSLRequest here instead.
        Ok(())
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        // Bare detail, no prefix: the driver hands this back to `fail`, which
        // is where the `Query failed: ` / `Failed to connect: ` choice is made.
        self.conn.receive(bytes).map_err(|e| e.to_string())
    }

    fn take_tls_request(&mut self) -> bool {
        std::mem::take(&mut self.tls_requested)
    }

    fn tls_established(&mut self, facts: &TlsFacts) -> Result<(), String> {
        // Kept for `Step::ScramNeeded`, which happens several round trips
        // later and has no way back to the handshake.
        self.channel_binding = facts.channel_binding.clone();
        // The bool the core believes. `tls_established_with_channel_binding`
        // is what decides whether SCRAM-SHA-256-PLUS is offered at all, so
        // passing `true` here without a digest to back it would make the core
        // ask for PLUS and then fail — which is exactly the failure mode this
        // pair of calls exists to prevent.
        self.conn
            .tls_established_with_channel_binding(facts.channel_binding.is_some())
            .map_err(|e| e.to_string())
    }

    fn drain(&mut self) -> Result<bool, String> {
        loop {
            match self.next_step() {
                Ok(None) => break,
                Ok(Some(step)) => {
                    if let Some(message) = self.apply(step) {
                        return Err(message);
                    }
                }
                Err(err) => {
                    // "Do not resume parsing after a protocol error." `abort`
                    // is safe to call unconditionally: a terminal server error
                    // has already put the core in closing (where `abort` is a
                    // no-op, so the server's diagnostic survives), and any
                    // other error has not. Either way the next pulls drain one
                    // `Completed { Aborted }` per queued token and then
                    // `Closed`, which is why this continues the loop rather
                    // than returning — the loop then terminates because a
                    // closing core cannot produce another `Err`.
                    self.record_failure(&err.to_string());
                    self.conn.abort(err);
                }
            }
        }
        Ok(self.finished)
    }

    fn output(&self) -> &[u8] {
        self.conn.output()
    }

    fn consume_output(&mut self, n: usize) {
        // The only error is acknowledging more than `output()` offered, and the
        // driver acknowledges exactly the slice it copied.
        let _ = self.conn.consume_output(n);
    }

    fn next_timeout_ms(&self) -> Option<u64> {
        let at = self.conn.next_timeout()?;
        let now = Instant::now();
        Some(if at <= now {
            0
        } else {
            at.duration_since(now).as_millis().min(u128::from(u64::MAX)) as u64
        })
    }

    fn handle_timeout(&mut self) {
        self.conn.handle_timeout(Instant::now());
    }

    fn fail(&mut self, reason: &str) {
        self.record_failure(reason);
        self.conn.abort(Error::Transport);
        // Drain first so the core's own terminal events settle the statements
        // it is accounting for; this then only has to answer what the core does
        // not know about — the offline queue and anyone waiting on the
        // handshake.
        let _ = <Self as DbCore>::drain(self);
        self.settle_all_with_error(reason);
        self.finished = true;
    }

    fn has_pending_work(&self) -> bool {
        !self.pending.is_empty() || !self.offline.is_empty() || !self.waiting_ready.is_empty()
    }
}

/// Resolve a parked connect-time promise.
///
/// `resolve_with` even for `undefined`, which needs no allocation, because
/// **every** settlement this module makes has to go through the same deferred
/// queue that `reject_string` uses. Two reasons. It is called from inside the
/// sink, where the driver holds its connection table borrowed and a settlement
/// that ran JS re-entrantly would deadlock rather than misbehave quietly. And
/// mixing the immediate and deferred paths would reorder settlements against
/// each other — a `client.end()` resolving before the queries it just
/// terminated rejected, say — which is observable as the order `.then`/`.catch`
/// handlers run in.
fn resolve_ready(promise: JsPromise, value: ReadyValue) {
    match value {
        ReadyValue::Undefined => promise.resolve_with(|| JsValue::UNDEFINED),
        ReadyValue::ClientHandle(handle) | ReadyValue::PoolHandle(handle) => {
            promise.resolve_with(move || JsValue::from_number(handle as f64))
        }
    }
}

/// Take back a handle that was registered only so a connect could be keyed on
/// it.
///
/// The sqlx path registers a handle **after** a successful connect, so a failed
/// `pg.connect()` leaves nothing behind. Doing the same here is what keeps a
/// program that retries in a loop from leaking one registry slot per attempt.
fn forget_pre_registered(value: &ReadyValue) {
    match value {
        ReadyValue::Undefined => {}
        ReadyValue::ClientHandle(handle) => {
            crate::forget_client(*handle);
        }
        ReadyValue::PoolHandle(handle) => {
            crate::forget_pool(*handle);
        }
    }
}

/// The OID the sqlx path bound each JS value as.
///
/// Reproduced rather than left unspecified (`0`, "let the server infer"):
/// PostgreSQL resolves an unspecified parameter from its context, and sqlx's
/// explicit typing is what today's queries were written against. `SELECT $1`
/// with a JS number resolves to `float8` here exactly as it did before, instead
/// of silently becoming `text`.
fn param_oid(param: &ParamValue) -> u32 {
    match param {
        // sqlx bound `Option::<String>::None` for a JS null/undefined.
        ParamValue::Null | ParamValue::String(_) => 25, // TEXT
        ParamValue::Number(_) => 701,                   // FLOAT8
        ParamValue::Int(_) => 20,                       // INT8
        ParamValue::Bool(_) => 16,                      // BOOL
    }
}

/// Encode one parameter in the text format. `None` is a SQL NULL, which is not
/// the same wire value as an empty string.
fn encode_param(param: &ParamValue) -> Option<Vec<u8>> {
    match param {
        ParamValue::Null => None,
        ParamValue::String(s) => Some(s.as_bytes().to_vec()),
        // Rust's shortest round-tripping `f64` rendering, so the value the
        // server parses is bit-identical to the one JS held. `inf`, `-inf` and
        // `NaN` are all accepted by PostgreSQL's float8 input.
        ParamValue::Number(n) => Some(n.to_string().into_bytes()),
        ParamValue::Int(i) => Some(i.to_string().into_bytes()),
        ParamValue::Bool(b) => Some(if *b { b"t".to_vec() } else { b"f".to_vec() }),
    }
}

extern "C" fn sink(completion: *const NetCompletion) {
    if completion.is_null() {
        return;
    }
    // SAFETY: the runtime borrows one completion for the duration of this call.
    let completion = unsafe { &*completion };
    let id = completion.id;
    let retired = REGISTRY.with(|reg| {
        reg.dispatch(completion);
        !reg.is_live(id)
    });
    if retired {
        OPEN.with(|open| open.borrow_mut().retain(|_, v| *v != id));
    }
}

/// Whether a connection created *now, on this thread* can live on turnloop.
pub(crate) fn enabled() -> bool {
    REGISTRY.with(|reg| reg.enabled(sink))
}

/// Install the sink and report whether the runtime accepted it.
///
/// Separate from [`enabled`] so a test can assert the part that is a property
/// of the build — the completion-layout digest check — without also asserting
/// that the thread it happens to run on owns a loop. `cargo test` puts each
/// test on its own thread and only some of them do.
#[cfg(test)]
fn register_only() -> bool {
    REGISTRY.with(|reg| reg.register(sink))
}

/// Whether this config can live on turnloop.
///
/// A host that names a Unix-domain socket declines: the driver submits a TCP
/// connect, and a socket file needs `pipe_connect`. sqlx reaches one today, so
/// declining keeps those clients working instead of trading a slow connection
/// for no connection.
pub(crate) fn supports(config: &PgConfig) -> bool {
    // `enabled()` first so the sink is registered even for a config that
    // declines — registration is idempotent and its result is what every other
    // connection on this thread consults.
    enabled() && !is_unix_socket_host(&config.host)
}

/// libpq's own rule: a host starting with `/` is a socket directory.
fn is_unix_socket_host(host: &str) -> bool {
    host.starts_with('/')
}

fn driver_id(handle: Handle) -> Option<i64> {
    let id = OPEN.with(|open| open.borrow().get(&handle).copied())?;
    REGISTRY.with(|reg| reg.is_live(id)).then_some(id)
}

/// Open `handle`'s connection if it has none, and return its driver id.
fn open(
    handle: Handle,
    config: &PgConfig,
    connect_failure: &'static str,
    queue_offline: bool,
) -> Result<i64, String> {
    if let Some(id) = driver_id(handle) {
        return Ok(id);
    }
    OPEN.with(|open| {
        open.borrow_mut().remove(&handle);
    });
    let core = PgCore::new(config, connect_failure, queue_offline)?;
    let tls = tls_options(config);
    let id = REGISTRY.with(|reg| {
        reg.connect_with_tls(
            &config.host,
            config.port,
            core,
            handle.try_into().unwrap_or(0),
            tls,
        )
    })?;
    OPEN.with(|open| {
        open.borrow_mut().insert(handle, id);
    });
    Ok(id)
}

/// Park `promise` until the handshake finishes, or settle it now if it already
/// has.
fn park_ready(id: i64, promise: JsPromise, value: ReadyValue, connect_failure: &'static str) {
    // Checked before the promise moves: a connection that is already through
    // its handshake settles immediately, which is what a second `connect()`
    // did on the sqlx path.
    if REGISTRY
        .with(|reg| reg.inspect(id, |core| core.ready))
        .unwrap_or(false)
    {
        resolve_ready(promise, value);
        return;
    }
    // The promise travels through an `Option` so that a `with_core` which never
    // runs its closure — the entry went away between `open` and here — hands it
    // back instead of dropping it. A dropped `JsPromise` never settles.
    let mut slot = Some((promise, value));
    let parked = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            core.waiting_ready
                .push(slot.take().expect("the closure runs at most once"));
        })
    });
    if parked.is_none() {
        if let Some((promise, value)) = slot {
            forget_pre_registered(&value);
            promise.reject_string(&format!("{}: {}", connect_failure, "Connection closed"));
        }
    }
}

/// `client.connect()` on a client built by `js_pg_client_new`.
pub(crate) fn client_connect(handle: Handle, config: &PgConfig, promise: JsPromise) {
    let id = match open(handle, config, CONNECT_FAILURE, false) {
        Ok(id) => id,
        Err(message) => {
            promise.reject_string(&format!("{}: {}", CONNECT_FAILURE, message));
            return;
        }
    };
    park_ready(id, promise, ReadyValue::Undefined, CONNECT_FAILURE);
}

/// `pg.connect(config)` — register the handle, connect, resolve the handle.
pub(crate) fn connect_new_client(config: PgConfig, promise: JsPromise) {
    let handle = register_handle(PgConnectionHandle::turnloop(config.clone(), true));
    match open(handle, &config, CONNECT_FAILURE, false) {
        Ok(id) => park_ready(
            id,
            promise,
            ReadyValue::ClientHandle(handle),
            CONNECT_FAILURE,
        ),
        Err(message) => {
            crate::forget_client(handle);
            promise.reject_string(&format!("{}: {}", CONNECT_FAILURE, message));
        }
    }
}

/// `pg.createPool(config)` — the eager pool factory.
pub(crate) fn create_pool(config: PgConfig, promise: JsPromise) {
    let handle = register_handle(PgPoolHandle::turnloop(config.clone()));
    match open(handle, &config, POOL_FAILURE, true) {
        Ok(id) => park_ready(id, promise, ReadyValue::PoolHandle(handle), POOL_FAILURE),
        Err(message) => {
            crate::forget_pool(handle);
            promise.reject_string(&format!("{}: {}", POOL_FAILURE, message));
        }
    }
}

/// `client.query(...)` — the connection must already exist.
pub(crate) fn client_query(handle: Handle, promise: JsPromise, statement: Statement) {
    let Some(id) = driver_id(handle) else {
        // `connect()` was never called, or its connection has gone. The sqlx
        // path finds `connection: None` in exactly these cases and says this.
        promise.reject_string(CLOSED);
        return;
    };
    submit(id, promise, statement);
}

/// `pool.query(...)` — opens the connection on first use.
pub(crate) fn pool_query(
    handle: Handle,
    config: &PgConfig,
    promise: JsPromise,
    statement: Statement,
) {
    let id = match open(handle, config, POOL_FAILURE, true) {
        Ok(id) => id,
        Err(message) => {
            promise.reject_string(&format!("{}: {}", POOL_FAILURE, message));
            return;
        }
    };
    submit(id, promise, statement);
}

fn submit(id: i64, promise: JsPromise, statement: Statement) {
    let mut slot = Some(promise);
    let submitted = REGISTRY.with(|reg| {
        reg.with_core(id, |core| {
            let promise = slot.take().expect("the closure runs at most once");
            core.submit_or_queue(QueuedQuery { statement, promise })
        })
    });
    match submitted {
        Some(Ok(())) => {}
        Some(Err((promise, message))) => promise.reject_string(&message),
        None => {
            if let Some(promise) = slot {
                promise.reject_string(CLOSED);
            }
        }
    }
}

/// `client.end()`.
pub(crate) fn client_end(handle: Handle, promise: JsPromise) {
    let live = driver_id(handle);
    // The sqlx path takes the handle out of the registry here, so a query after
    // `end()` says "Invalid client handle" rather than "Connection already
    // closed". Same bookkeeping, same messages.
    let existed = crate::forget_client(handle);
    let Some(id) = live else {
        promise.reject_string(if existed { CLOSED } else { INVALID_CLIENT });
        return;
    };
    close_connection(handle, id);
    promise.resolve_with(|| JsValue::UNDEFINED);
}

/// `pool.end()`.
pub(crate) fn pool_end(handle: Handle, promise: JsPromise) {
    let live = driver_id(handle);
    if !crate::forget_pool(handle) {
        promise.reject_string(INVALID_POOL);
        return;
    }
    if let Some(id) = live {
        close_connection(handle, id);
    }
    // A pool that was never queried has nothing to close and still resolves —
    // as it does today, where `pool` is `None` and the close is skipped.
    promise.resolve_with(|| JsValue::UNDEFINED);
}

/// Send Terminate, drain the core's terminal events, then close the socket.
///
/// Two `with_core` calls because the core withholds its `Closed` event until
/// its output has been acknowledged, and the acknowledgement happens in the
/// flush the driver runs when the first closure returns.
fn close_connection(handle: Handle, id: i64) {
    OPEN.with(|open| {
        open.borrow_mut().remove(&handle);
    });
    let ended = REGISTRY.with(|reg| reg.with_core(id, |core| core.conn.end()));
    match ended {
        Some(Ok(())) => {
            REGISTRY.with(|reg| {
                reg.with_core(id, |core| {
                    let _ = <PgCore as DbCore>::drain(core);
                });
                reg.close(id);
            });
        }
        // `end()` refuses while statements are still in flight. Dropping the
        // socket is what `PgConnection::close` did to the same state; aborting
        // rejects those statements rather than leaving them pending forever.
        Some(Err(_)) | None => REGISTRY.with(|reg| reg.abort(id, "Connection terminated")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_subsystem_slot_is_the_one_reserved_for_this_binding() {
        // Two bindings sharing a slot would route each other's completions into
        // the wrong connection table — a cross-protocol misdelivery that no
        // protocol-level test could catch.
        assert_eq!(SUBSYSTEM, subsystem::PG);
        assert_ne!(SUBSYSTEM, subsystem::MYSQL);
        assert_ne!(SUBSYSTEM, subsystem::REDIS);
        assert_ne!(SUBSYSTEM, subsystem::MONGODB);
    }

    #[test]
    fn registration_passes_the_abi_layout_check() {
        // The dev-dependency links the runtime, so this exercises the real
        // `register_sink`: a mismatch between perry-ffi's `NetCompletion`
        // layout digest and the runtime's refuses registration, leaves
        // `available` false, and would silently put every client back on the
        // sqlx transport. On an agent with no loop this is legitimately false
        // and that fallback is correct — which is why the assertion belongs
        // here, where the runtime *is* linked, rather than in a live run.
        assert!(
            register_only(),
            "a false here is an ABI layout mismatch between perry-ffi and perry-runtime"
        );
        assert!(perry_ffi::turnloop_net::sink_installed(SUBSYSTEM));
    }

    #[test]
    fn the_wire_config_is_plaintext_with_no_channel_binding() {
        // Perry's `pg` has never had TLS on either path. If this ever flips to
        // Prefer, the core starts an SSLRequest round trip and emits
        // `UpgradeTls`, which this transport cannot perform — so the config is
        // the guard, and the `UpgradeTls` arm is only the backstop.
        let wire = turnloop_config(&PgConfig::default());
        assert_eq!(wire.ssl, SslMode::Disable);
        assert!(!wire.channel_binding_required);
    }

    #[test]
    fn an_absent_database_falls_back_to_the_user_name() {
        // `PgConfig::to_url` omits the path component entirely when `database`
        // is None, and PostgreSQL then defaults the database to the user name.
        // Sending the user name explicitly reproduces that; sending "postgres"
        // (the core's own default) would connect a `pg.Client({user:'app'})` to
        // a different database than it reaches today.
        let config = PgConfig {
            user: "app".to_string(),
            database: None,
            ..PgConfig::default()
        };
        assert_eq!(turnloop_config(&config).database, "app");
        let named = PgConfig {
            database: Some("shop".to_string()),
            ..config
        };
        assert_eq!(turnloop_config(&named).database, "shop");
    }

    #[test]
    fn the_result_kind_follows_the_sql_classifier() {
        // `rowCount` comes from a different place for each kind, so a
        // misclassified statement reports the wrong number rather than failing.
        assert_eq!(ResultKind::for_sql("SELECT * FROM x"), ResultKind::Rows);
        assert_eq!(ResultKind::for_sql("  select 1"), ResultKind::Rows);
        assert_eq!(ResultKind::for_sql("WITH cte AS (…)"), ResultKind::Rows);
        assert_eq!(
            ResultKind::for_sql("INSERT INTO x VALUES (1)"),
            ResultKind::RowsAffected
        );
        assert_eq!(
            ResultKind::for_sql("UPDATE x SET y = 1"),
            ResultKind::RowsAffected
        );
    }

    #[test]
    fn a_unix_socket_host_declines_this_transport() {
        // Declining keeps those clients on sqlx, where they work. Accepting
        // them would submit a TCP connect to a path and fail every connection
        // that works today.
        assert!(is_unix_socket_host("/var/run/postgresql"));
        assert!(!is_unix_socket_host("localhost"));
        assert!(!is_unix_socket_host("db.internal"));
        assert!(!is_unix_socket_host("127.0.0.1"));
    }

    #[test]
    fn parameters_carry_the_same_type_oids_sqlx_bound() {
        // These OIDs are what makes `WHERE id = $1` resolve the same way it
        // does today. Leaving them unspecified would let PostgreSQL infer a
        // different type from context for the same JS value.
        assert_eq!(param_oid(&ParamValue::Null), 25);
        assert_eq!(param_oid(&ParamValue::String(String::new())), 25);
        assert_eq!(param_oid(&ParamValue::Int(1)), 20);
        assert_eq!(param_oid(&ParamValue::Number(1.5)), 701);
        assert_eq!(param_oid(&ParamValue::Bool(true)), 16);
    }

    #[test]
    fn a_null_parameter_is_an_absent_value_not_an_empty_string() {
        // The Bind message distinguishes them by length (-1 vs 0), and so does
        // every `IS NULL` in user SQL.
        assert_eq!(encode_param(&ParamValue::Null), None);
        assert_eq!(
            encode_param(&ParamValue::String(String::new())),
            Some(Vec::new())
        );
    }

    #[test]
    fn scram_plus_without_a_binding_is_refused_rather_than_downgraded() {
        // The failure this guards is silent: `ChannelBinding::unsupported()`
        // authenticates successfully against a server that also offers plain
        // SCRAM, so a downgrade here would look like a working connection.
        let Err(refused) = scram_channel(true, None) else {
            panic!("PLUS with no digest must be refused");
        };
        assert!(
            refused.contains("tls-server-end-point"),
            "the message names what is missing, got {refused:?}"
        );
    }

    #[test]
    fn scram_plus_binds_the_digest_into_the_gs2_header() {
        // `start_scram` cross-checks the mechanism against this prefix, and the
        // SERVER recomputes the digest from its own certificate — so a wrong
        // one fails authentication rather than weakening it. Asserting the
        // prefix here is asserting that the digest reached the message at all.
        let digest = vec![0xABu8; 32];
        let channel = scram_channel(true, Some(&digest)).expect("a digest is enough");
        let scram = ScramSha256::new(b"pw", channel);
        assert!(
            scram.message().starts_with(b"p=tls-server-end-point,"),
            "got {:?}",
            String::from_utf8_lossy(&scram.message()[..24.min(scram.message().len())])
        );
    }

    #[test]
    fn plain_scram_announces_that_it_did_not_bind() {
        let channel = scram_channel(false, None).expect("plain SCRAM needs nothing");
        let scram = ScramSha256::new(b"pw", channel);
        // `n,,` is RFC 5802's "client does not support channel binding". The
        // third spelling, `y,,`, would claim the server hid PLUS from us, and
        // claiming that when TLS is off is how a downgrade goes unnoticed.
        assert!(
            scram.message().starts_with(b"n,,"),
            "got {:?}",
            String::from_utf8_lossy(&scram.message()[..8.min(scram.message().len())])
        );
    }

    #[test]
    fn a_binding_is_ignored_when_the_core_did_not_ask_for_plus() {
        // Having a digest does not license offering PLUS: the core asks for it
        // only when the SERVER offered the PLUS mechanism, and answering a
        // plain request with a `p=` header makes `start_scram` refuse.
        let scram = ScramSha256::new(b"pw", scram_channel(false, Some(&[0u8; 32])).unwrap());
        assert!(scram.message().starts_with(b"n,,"));
    }

    #[test]
    fn a_config_without_ssl_disables_the_sslrequest_entirely() {
        let plain = turnloop_config(&PgConfig::default());
        assert_eq!(plain.ssl, SslMode::Disable);
        assert!(!plain.channel_binding_required);
        assert!(tls_options(&PgConfig::default()).is_none());
    }

    #[test]
    fn an_ssl_config_requires_tls_rather_than_preferring_it() {
        // `Prefer` would let a server answer `N` and the core continue in
        // plaintext — a client that asked for TLS sending its password in the
        // clear. That is the whole reason this is not configurable.
        let config = PgConfig {
            host: "db.example.com".to_string(),
            ssl: Some(crate::PgSslConfig {
                reject_unauthorized: true,
                ca: b"-----BEGIN CERTIFICATE-----".to_vec(),
                servername: None,
            }),
            ..PgConfig::default()
        };
        assert_eq!(turnloop_config(&config).ssl, SslMode::Require);
        let options = tls_options(&config).expect("an ssl config produces TLS options");
        assert_eq!(options.servername, "db.example.com");
        assert!(options.reject_unauthorized);
        assert_eq!(options.ca_pem, b"-----BEGIN CERTIFICATE-----".to_vec());
        assert!(
            options.alpn.is_empty(),
            "PostgreSQL's TLS carries PostgreSQL and nothing else"
        );
    }

    #[test]
    fn an_explicit_servername_overrides_the_host() {
        // What a client connecting through a pooler or an IP literal needs:
        // the certificate names the logical host, not the address dialled.
        let config = PgConfig {
            host: "10.0.0.7".to_string(),
            ssl: Some(crate::PgSslConfig {
                reject_unauthorized: false,
                ca: Vec::new(),
                servername: Some("db.internal".to_string()),
            }),
            ..PgConfig::default()
        };
        let options = tls_options(&config).expect("TLS options");
        assert_eq!(options.servername, "db.internal");
        assert!(!options.reject_unauthorized);
    }

    #[test]
    fn numeric_parameters_round_trip_through_their_text_spelling() {
        // The text format is only safe if the rendering round-trips exactly;
        // Rust's shortest-representation `Display` does, a fixed-precision
        // format would not.
        let encoded = encode_param(&ParamValue::Number(0.1 + 0.2)).unwrap();
        let text = String::from_utf8(encoded).unwrap();
        assert_eq!(text.parse::<f64>().unwrap(), 0.1 + 0.2);
        assert_eq!(
            encode_param(&ParamValue::Int(-9007199254740993)),
            Some(b"-9007199254740993".to_vec())
        );
        assert_eq!(encode_param(&ParamValue::Bool(false)), Some(b"f".to_vec()));
    }
}
