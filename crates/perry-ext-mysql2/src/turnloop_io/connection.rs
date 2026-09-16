//! One loop-driven MySQL connection: the sans-I/O core plus its command queue.
//!
//! # Why there is a queue here and not in the protocol crate
//!
//! MySQL has no pipelining. `turnloop_mysql::Connection` holds a single
//! `pending: Option<Pending>` and its `accept()` refuses any command while one
//! is outstanding — the crate's README says so outright ("MySQL permits one
//! active command. Busy calls return backpressure; the adapter queues commands
//! in JS submission order"). The adapter is this file.
//!
//! JS does not know that. `mysql2` in Node lets a program fire
//! `conn.query(a)` and `conn.query(b)` without awaiting the first, and both
//! resolve, in order. So every command JS submits is appended to [`Command`]
//! queue and issued only once the previous one has produced its `Completed`
//! event. Losing, reordering or rejecting a command queued behind another is
//! the single worst failure this file could have: it is the one the tests in
//! `super::tests` pin hardest.
//!
//! # One promise, settled exactly once
//!
//! Every queued command owns the `JsPromise` that will answer it. There are
//! exactly four exits — success, server error, abort, and connection teardown
//! — and [`MysqlCore::settle_all`] is the backstop for the last of them: a
//! `JsPromise` that is dropped rather than settled leaves `await` hanging
//! forever, which no caller can recover from.
//!
//! # No JS value is built here
//!
//! Everything in this file runs in the sink, on the agent thread, inside the
//! loop's completion dispatch. Rows are copied out of the receive buffer as
//! owned Rust data (`turnloop_mysql`'s `Row`/`Column` borrow that buffer and
//! die at the next mutable call on the core), and the JS result is built by the
//! existing `crate::outcome_to_jsvalue` inside a `JsPromise::resolve_with`
//! closure, which the resolution pump runs on the main thread. That is #1824's
//! rule, which the `spawn_blocking` path also had to obey.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use perry_db_turnloop::{DbCore, TlsFacts};
use perry_ffi::{Handle, JsPromise, JsValue};
use turnloop_mysql::{Config, Connection, Error, Event, Instant, Outcome, Statement, Token};

use super::convert::{self, OwnedColumn};
use crate::{
    MysqlPromiseError, QueryOutcome, QueryRequest, RawQueryResult, RawRowData,
    DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_QUERY_TIMEOUT_SECS,
};

/// Prepared statements kept per connection, keyed by SQL text.
///
/// Node's `mysql2` caches prepared statements per connection exactly this way
/// (its `maxPreparedStatements` option defaults to 16000). The sqlx path
/// instead used `.persistent(false)`, which prepared and closed one statement
/// per call because #8745 saw metadata from a neighbouring statement paired
/// with this request's arguments. That shape cannot recur here: a
/// `turnloop_mysql::Connection` carries one command at a time, `execute` names
/// its statement id explicitly and rejects a wrong parameter count, and each
/// execute's column metadata arrives on the wire rather than from a
/// client-side cache.
///
/// The cap exists so a program that builds SQL by interpolation cannot walk the
/// server's `max_prepared_stmt_count` (16382 by default). An evicted statement
/// is closed on the wire, not abandoned.
const STATEMENT_CACHE: usize = 32;

/// What a settled command resolves with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Answer {
    /// `query()` / `execute()` — the mysql2 `[rows, fields]` tuple.
    ResultTuple,
    /// `beginTransaction()` / `commit()` / `rollback()` — `undefined`, which is
    /// what `run_simple_command` resolved.
    Undefined,
}

/// One JS request, with the promise it owes an answer to.
pub(crate) struct Request {
    pub(crate) request: QueryRequest,
    pub(crate) promise: JsPromise,
    /// Wall-clock expiry, taken at **submission** and not at issue.
    ///
    /// The sqlx path wrapped the whole call — waiting for the connection lock
    /// included — in one `tokio::time::timeout`, so a query queued behind a
    /// slow one was already on the clock. Keeping that means a command stuck in
    /// this queue still fails on schedule instead of waiting out the command
    /// ahead of it and then starting its own 30 seconds.
    pub(crate) deadline: Instant,
    /// The rejection prefix, exactly the context string the sqlx path handed to
    /// `MysqlPromiseError::from_sqlx` — `"Query failed"`, or the SQL itself for
    /// a transaction command.
    pub(crate) context: &'static str,
    pub(crate) answer: Answer,
}

/// Something submitted to a connection, waiting its turn on the wire.
pub(crate) enum Command {
    Request(Box<Request>),
    /// A prepared statement evicted from the cache. Nothing in JS waits on it.
    CloseStatement(u32),
    /// `COM_QUIT`. The promise, when there is one, is `connection.end()`'s.
    Quit(Option<JsPromise>),
}

/// Which wire command of a request is currently outstanding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// A text-protocol `COM_QUERY`; its result answers the request.
    Query,
    /// `COM_STMT_PREPARE`; the execute follows on the same connection.
    Preparing,
    /// `COM_STMT_EXECUTE`; its result answers the request.
    Executing,
}

/// The command on the wire right now. At most one, always.
enum Active {
    Request {
        token: Token,
        stage: Stage,
        command: Box<Request>,
        acc: ResultAcc,
        /// Filled by the `Prepared` event, consumed by its `Completed`.
        prepared: Option<Statement>,
    },
    CloseStatement {
        token: Token,
    },
}

/// A server error, copied out of the receive buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerFailure {
    errno: u16,
    message: String,
}

/// Everything one request has collected so far.
#[derive(Default)]
struct ResultAcc {
    /// Columns of the result set currently streaming, used to decode its rows.
    columns: Vec<OwnedColumn>,
    /// Columns of the **first** result set, which is what `fields` reports.
    ///
    /// sqlx's `fetch_all` concatenated the rows of every result set and
    /// `raws_from_mysql_rows` then described them with `rows[0]`'s columns. A
    /// multi-statement `query()` therefore reported the first statement's
    /// fields, and this reproduces that rather than reporting the last.
    reported: Vec<OwnedColumn>,
    rows: Vec<RawRowData>,
    saw_result_set: bool,
    /// A second result set has begun, so `reported` is final.
    first_set_done: bool,
    affected_rows: u64,
    last_insert_id: u64,
    error: Option<ServerFailure>,
}

/// An event that needs `&mut self`, lifted out of the borrow of the receive
/// buffer that produced it.
///
/// `Connection::next_event` hands back an `Event<'_>` borrowing the core, so
/// nothing inside the drain loop may call a `&mut self` method while that
/// borrow is live. Events that only touch the accumulator are handled in place
/// against disjoint fields; the rest become one of these, which own their data.
enum Action {
    Connected,
    RsaSeed,
    Prepared(Token, Statement),
    Completed(Token, Outcome),
    Closed(Error),
    /// The core asked for something this host cannot do. Terminal.
    Unsupported(String),
}

/// A `createConnection()` / `getConnection()` caller waiting for the handshake.
struct ReadyWaiter {
    promise: JsPromise,
    /// The JS handle to resolve with. Registered before the socket is opened so
    /// the handshake has something to answer with.
    handle: Handle,
}

/// How a connection died, rendered into the two message shapes its outstanding
/// promises need.
struct Failure {
    /// The bare reason, without any context prefix.
    reason: String,
    /// A deadline expired, which the sqlx path reported with its own fixed
    /// strings rather than the underlying error.
    timeout: bool,
    code: Option<&'static str>,
    errno: Option<u16>,
}

impl Failure {
    /// The connection died on its own terms — a protocol error, a deadline, the
    /// peer closing.
    fn from_core(reason: Error, ready: bool, server: Option<ServerFailure>) -> Self {
        // A server error seen just before the connection went away is the real
        // cause; "Connection closed" on its own hides an `ER_ACCESS_DENIED` or
        // a wrong database behind a transport message.
        if let Some(server) = server {
            return Self {
                reason: server.message,
                timeout: false,
                code: crate::mysql2_error_code(server.errno),
                errno: Some(server.errno),
            };
        }
        Self {
            reason: reason.to_string(),
            timeout: reason == Error::Timeout && ready,
            code: None,
            errno: None,
        }
    }

    /// The transport failed underneath the core; the driver named the reason.
    fn from_host(reason: String) -> Self {
        Self {
            reason,
            timeout: false,
            code: None,
            errno: None,
        }
    }

    /// The rejection a command carries.
    fn command_error(&self, context: &str) -> MysqlPromiseError {
        if self.timeout {
            return MysqlPromiseError::message("Query timed out");
        }
        MysqlPromiseError {
            message: format!("{context}: {}", self.reason),
            code: self.code,
            errno: self.errno,
        }
    }

    /// The rejection a `createConnection()` / `getConnection()` caller carries.
    fn connect_message(&self) -> String {
        if self.timeout {
            return "MySQL connection timed out".to_string();
        }
        format!("Failed to connect: {}", self.reason)
    }
}

/// The sans-I/O half of one MySQL connection.
pub(crate) struct MysqlCore {
    conn: Connection,
    /// Submitted, not yet on the wire. Issued strictly in submission order.
    queue: VecDeque<Command>,
    active: Option<Active>,
    /// The handshake finished.
    ready: bool,
    /// The terminal `Closed` event has fired.
    finished: bool,
    /// No further command may be issued: `end()` is under way, or the transport
    /// failed. Distinct from `finished`, which means the core is done talking.
    closing: bool,
    waiting_ready: Vec<ReadyWaiter>,
    /// SQL → server statement id.
    statements: HashMap<String, u32>,
    /// Least-recently-executed first; the eviction order for `statements`.
    statement_order: VecDeque<String>,
    next_token: Token,
    /// A deadline the **binding** wants a turn at, independent of the
    /// protocol's own.
    ///
    /// Two commands finish without the server saying anything: `COM_STMT_CLOSE`
    /// gets no reply at all, and `COM_QUIT`'s terminal `Closed` only fires from
    /// the *next* `next_event` once its bytes are acknowledged. Neither will
    /// produce a read completion, so without a deadline of our own the
    /// connection would sit there with a command that can never complete and a
    /// queue that can never advance. Arming zero milliseconds asks the driver
    /// for one more turn, which is all either needs.
    kick_at: Option<Instant>,
    /// `connection.end()`'s promise, settled by the terminal `Closed`.
    quit: Option<JsPromise>,
    /// The last server error, kept as the real reason when the connection dies.
    last_server_error: Option<ServerFailure>,
    /// The driver's word for a transport failure, preferred over the core's
    /// generic `Connection lost`.
    host_failure: Option<String>,
    /// The core has put its `SSLRequest` packet in `output()` and wants the
    /// transport upgraded. Taken by the driver, which flushes that packet in
    /// the clear and only then installs the session.
    tls_requested: bool,
}

impl MysqlCore {
    pub(crate) fn new(config: &crate::MySqlConfig) -> Result<Self, String> {
        let conn = Connection::new(protocol_config(config))
            .map_err(|err| format!("Failed to connect: {err}"))?;
        Ok(Self {
            conn,
            queue: VecDeque::new(),
            active: None,
            ready: false,
            finished: false,
            closing: false,
            waiting_ready: Vec::new(),
            statements: HashMap::new(),
            statement_order: VecDeque::new(),
            next_token: 1,
            kick_at: None,
            quit: None,
            last_server_error: None,
            host_failure: None,
            tls_requested: false,
        })
    }

    /// Whether this connection is free to take on a pool request.
    ///
    /// A connection with anything queued, anything on the wire, or a teardown
    /// under way is not free, and the pool must not hand it to a second caller.
    pub(crate) fn is_idle(&self) -> bool {
        !self.closing && !self.finished && self.active.is_none() && self.queue.is_empty()
    }

    /// The handshake has finished and commands go straight out.
    ///
    /// Only the tests read this: the pool deliberately treats a connection that
    /// is still shaking hands as assignable, because a command submitted on it
    /// simply queues until `Connected` fires.
    #[cfg(test)]
    pub(crate) fn is_ready(&self) -> bool {
        self.ready
    }

    /// Park a `createConnection()` / `getConnection()` promise until the
    /// handshake finishes. Resolves with `handle`, which is already registered.
    pub(crate) fn park_ready(&mut self, promise: JsPromise, handle: Handle) {
        if self.ready {
            promise.resolve_with(move || JsValue::from_object_ptr(handle as *mut ()));
            return;
        }
        self.waiting_ready.push(ReadyWaiter { promise, handle });
    }

    /// Append one command and issue it if the wire is free.
    pub(crate) fn enqueue(&mut self, command: Command) {
        self.queue.push_back(command);
        self.pump();
    }

    /// Ask the driver for one more turn no later than `at`.
    ///
    /// Used by the pool for its acquire deadline: a pool has no turnloop handle
    /// of its own, so a waiter's timeout has to ride on one of its
    /// connections'. Never moves an existing kick later.
    pub(crate) fn request_kick(&mut self, at: Instant) {
        self.kick_at = Some(match self.kick_at {
            Some(existing) => existing.min(at),
            None => at,
        });
    }

    fn take_token(&mut self) -> Token {
        let token = self.next_token;
        self.next_token += 1;
        token
    }

    /// Issue the next queued command, if the wire is free.
    ///
    /// The guards are exactly `Connection::accept`'s preconditions, checked
    /// before a command is popped so a refusal here always means a real
    /// rejection (a statement past the buffer bound) and never backpressure —
    /// which would otherwise reject a command that merely had to wait.
    fn pump(&mut self) {
        while self.active.is_none() && !self.queue.is_empty() {
            if self.closing || self.finished || !self.conn.is_ready() {
                return;
            }
            if !self.conn.output().is_empty() {
                return;
            }
            let command = self.queue.pop_front().expect("the queue is not empty");
            match command {
                Command::CloseStatement(id) => {
                    let token = self.take_token();
                    if self.conn.close_statement(token, id).is_ok() {
                        self.active = Some(Active::CloseStatement { token });
                        self.kick_at = Some(Instant::now());
                    }
                    // A refused close means the server statement is already
                    // gone. Nothing in JS waits on it, so take the next
                    // command rather than stalling the queue.
                }
                Command::Quit(promise) => match self.conn.quit() {
                    Ok(()) => {
                        self.closing = true;
                        self.quit = promise;
                        self.kick_at = Some(Instant::now());
                    }
                    Err(_) => {
                        // Already closing. `end()` on a connection that is
                        // going away has got what it asked for, which is also
                        // what the sqlx path did with a connection whose slot
                        // had been taken by a concurrent `end()`.
                        //
                        // `finished` too, not just `closing`: the guards above
                        // mean the core can only refuse here if it is past
                        // `Ready`, so no `Closed` event is coming and a
                        // connection left merely `closing` would never be
                        // retired by the driver.
                        if let Some(promise) = promise {
                            promise.resolve_undefined();
                        }
                        self.closing = true;
                        self.finished = true;
                    }
                },
                Command::Request(command) => self.issue(command),
            }
        }
    }

    /// Put one request on the wire, choosing text protocol or prepare+execute
    /// exactly as the sqlx path did.
    fn issue(&mut self, command: Box<Request>) {
        let token = self.take_token();
        let deadline = Some(command.deadline);
        let started = if !command.request.uses_prepared_statement() {
            // `query()` with no bind values is MySQL's text protocol, which is
            // what mysql2 does and what keeps DDL out of the statement cache.
            self.conn
                .query(token, &command.request.sql, deadline)
                .map(|()| Stage::Query)
        } else if let Some(id) = self.statements.get(&command.request.sql).copied() {
            let params = convert::bind_values(&command.request.params);
            match self.conn.execute(token, id, &params, deadline) {
                Ok(()) => {
                    self.touch_statement(&command.request.sql);
                    Ok(Stage::Executing)
                }
                // The cached id is no longer registered with the core. Forget
                // it and prepare again: failing a valid query because our own
                // cache went stale would be a defect of this file's making.
                Err(_) => {
                    self.forget_statement(&command.request.sql);
                    self.conn
                        .prepare(token, &command.request.sql, deadline)
                        .map(|()| Stage::Preparing)
                }
            }
        } else {
            self.conn
                .prepare(token, &command.request.sql, deadline)
                .map(|()| Stage::Preparing)
        };
        match started {
            Ok(stage) => {
                self.active = Some(Active::Request {
                    token,
                    stage,
                    command,
                    acc: ResultAcc::default(),
                    prepared: None,
                })
            }
            Err(err) => {
                let Request {
                    promise, context, ..
                } = *command;
                MysqlPromiseError::message(format!("{context}: {err}")).reject(promise);
            }
        }
    }

    /// Run the execute half of a prepared request, on the same connection the
    /// prepare ran on.
    fn issue_execute(&mut self, command: Box<Request>, id: u32) {
        let token = self.take_token();
        let params = convert::bind_values(&command.request.params);
        match self
            .conn
            .execute(token, id, &params, Some(command.deadline))
        {
            Ok(()) => {
                self.active = Some(Active::Request {
                    token,
                    stage: Stage::Executing,
                    command,
                    acc: ResultAcc::default(),
                    prepared: None,
                })
            }
            Err(err) => {
                let Request {
                    promise, context, ..
                } = *command;
                MysqlPromiseError::message(format!("{context}: {err}")).reject(promise);
            }
        }
    }

    fn remember_statement(&mut self, sql: &str, id: u32) {
        if self.statements.contains_key(sql) {
            self.touch_statement(sql);
            return;
        }
        while self.statement_order.len() >= STATEMENT_CACHE {
            let Some(evicted) = self.statement_order.pop_front() else {
                break;
            };
            if let Some(id) = self.statements.remove(&evicted) {
                self.queue.push_back(Command::CloseStatement(id));
            }
        }
        self.statements.insert(sql.to_string(), id);
        self.statement_order.push_back(sql.to_string());
    }

    fn touch_statement(&mut self, sql: &str) {
        if let Some(at) = self.statement_order.iter().position(|s| s == sql) {
            if let Some(entry) = self.statement_order.remove(at) {
                self.statement_order.push_back(entry);
            }
        }
    }

    fn forget_statement(&mut self, sql: &str) {
        self.statements.remove(sql);
        if let Some(at) = self.statement_order.iter().position(|s| s == sql) {
            self.statement_order.remove(at);
        }
    }

    /// Pull every event the core has, handling each one.
    fn drain_events(&mut self) -> Result<bool, String> {
        loop {
            let event = match self.conn.next_event() {
                Ok(Some(event)) => event,
                Ok(None) => break,
                Err(err) => return Err(format!("MySQL protocol error: {err}")),
            };
            // Arms that only touch the accumulator run in place: `event`
            // borrows `self.conn` and the accumulator lives in `self.active`,
            // which is a disjoint field. Anything needing `&mut self` becomes
            // an owned `Action` instead.
            let action = match event {
                // `Progress` is a consumed control packet; the auth events are
                // informational. The host contract's only obligation for all
                // three is to keep polling, which this loop does.
                Event::Progress | Event::AuthFastSuccess | Event::AuthFull => None,
                Event::Connected { .. } => Some(Action::Connected),
                Event::RsaSeedNeeded => Some(Action::RsaSeed),
                Event::UpgradeTls => {
                    // The hard boundary: the `SSLRequest` packet is already in
                    // `output()` and everything after it is encrypted. The
                    // driver flushes that packet in the clear and then installs
                    // the session — acknowledging here would be too early,
                    // since the core refuses `tls_established` while it still
                    // has unsent output.
                    self.tls_requested = true;
                    None
                }
                Event::LocalInfile { .. } => Some(Action::Unsupported(
                    "LOAD DATA LOCAL INFILE is disabled".to_string(),
                )),
                Event::ColumnCount { token, count } => {
                    if let Some(Active::Request {
                        token: active,
                        stage,
                        acc,
                        ..
                    }) = self.active.as_mut()
                    {
                        if *active == token && *stage != Stage::Preparing {
                            // A second `ColumnCount` for the same command is a
                            // second result set (a multi-statement `query()`).
                            // Its columns decode its own rows, but `fields`
                            // keeps describing the first set, which is what
                            // sqlx's `rows[0]` did.
                            acc.first_set_done |= acc.saw_result_set;
                            acc.saw_result_set = true;
                            acc.columns.clear();
                            acc.columns.reserve(count);
                        }
                    }
                    None
                }
                Event::Column {
                    token,
                    column,
                    parameter,
                } => {
                    if let Some(Active::Request {
                        token: active,
                        stage,
                        acc,
                        ..
                    }) = self.active.as_mut()
                    {
                        // A prepare answers with the statement's parameter and
                        // result metadata. Only the execute's own column list
                        // describes the rows that follow, so the prepare's is
                        // dropped rather than accumulated.
                        if *active == token && !parameter && *stage != Stage::Preparing {
                            let column = OwnedColumn {
                                name: String::from_utf8_lossy(column.name).into_owned(),
                                info: column.type_info,
                            };
                            if !acc.first_set_done {
                                acc.reported.push(column.clone());
                            }
                            acc.columns.push(column);
                        }
                    }
                    None
                }
                Event::Row { token, row } => {
                    if let Some(Active::Request {
                        token: active,
                        stage,
                        acc,
                        ..
                    }) = self.active.as_mut()
                    {
                        if *active == token && *stage != Stage::Preparing {
                            // Copied out of the receive buffer now: `row`
                            // borrows the packet bytes and dies at the next
                            // mutable call on the core.
                            let decoded = convert::decode_row(row, &acc.columns);
                            acc.rows.push(decoded);
                        }
                    }
                    None
                }
                Event::Ok { token, packet } => {
                    if let Some(Active::Request {
                        token: active, acc, ..
                    }) = self.active.as_mut()
                    {
                        // An `Ok` that terminates a result set is an EOF packet
                        // whose affected-rows field is not a row count; only
                        // the one that *replaces* a result set carries the
                        // numbers a `ResultSetHeader` reports.
                        if *active == token && !acc.saw_result_set {
                            acc.affected_rows = packet.affected_rows();
                            acc.last_insert_id = packet.last_insert_id().unwrap_or(0);
                        }
                    }
                    None
                }
                Event::Error { token, error } => {
                    let failure = ServerFailure {
                        errno: error.errno,
                        message: error.sql_message.to_string(),
                    };
                    match token {
                        Some(token) => {
                            if let Some(Active::Request {
                                token: active, acc, ..
                            }) = self.active.as_mut()
                            {
                                if *active == token {
                                    acc.error = Some(failure.clone());
                                }
                            }
                            self.last_server_error = Some(failure);
                        }
                        // An error with no command outstanding is fatal to the
                        // session — the core has already moved to closing — so
                        // keep it as the reason everything else will report.
                        None => self.last_server_error = Some(failure),
                    }
                    None
                }
                Event::Prepared { token, statement } => Some(Action::Prepared(token, statement)),
                Event::Completed { token, outcome } => Some(Action::Completed(token, outcome)),
                Event::Closed { reason } => Some(Action::Closed(reason)),
            };
            if let Some(action) = action {
                self.apply(action)?;
            }
        }
        self.pump();
        Ok(self.finished)
    }

    fn apply(&mut self, action: Action) -> Result<(), String> {
        match action {
            Action::Connected => {
                self.ready = true;
                for waiter in std::mem::take(&mut self.waiting_ready) {
                    let handle = waiter.handle;
                    waiter
                        .promise
                        .resolve_with(move || JsValue::from_object_ptr(handle as *mut ()));
                }
            }
            Action::RsaSeed => {
                let seed = random_seed()?;
                self.conn
                    .rsa_seed(seed)
                    .map_err(|err| format!("MySQL authentication failed: {err}"))?;
            }
            Action::Prepared(token, statement) => {
                if let Some(Active::Request {
                    token: active,
                    prepared,
                    ..
                }) = self.active.as_mut()
                {
                    if *active == token {
                        *prepared = Some(statement);
                    }
                }
            }
            Action::Completed(token, outcome) => self.completed(token, outcome),
            Action::Closed(reason) => {
                self.finished = true;
                self.closing = true;
                // Our own `COM_QUIT` closed this. `end()` asked for exactly
                // that, so it resolves before the teardown rejects anything
                // else.
                if reason == Error::Cancelled {
                    if let Some(promise) = self.quit.take() {
                        promise.resolve_undefined();
                    }
                }
                let failure = match self.host_failure.take() {
                    Some(reason) => Failure::from_host(reason),
                    None => Failure::from_core(reason, self.ready, self.last_server_error.take()),
                };
                self.settle_all(&failure);
            }
            Action::Unsupported(message) => return Err(message),
        }
        Ok(())
    }

    fn completed(&mut self, token: Token, outcome: Outcome) {
        let Some(active) = self.active.take() else {
            return;
        };
        match active {
            Active::CloseStatement { token: active } => {
                if active != token {
                    self.active = Some(Active::CloseStatement { token: active });
                }
                // Nothing in JS waits on a statement close. A failed one leaks
                // a server-side statement, never a promise.
            }
            Active::Request {
                token: active,
                stage,
                command,
                acc,
                prepared,
            } => {
                if active != token {
                    self.active = Some(Active::Request {
                        token: active,
                        stage,
                        command,
                        acc,
                        prepared,
                    });
                    return;
                }
                match outcome {
                    Outcome::Success if stage == Stage::Preparing => match prepared {
                        Some(statement) => {
                            self.remember_statement(&command.request.sql, statement.id);
                            self.issue_execute(command, statement.id);
                        }
                        None => {
                            let Request {
                                promise, context, ..
                            } = *command;
                            MysqlPromiseError::message(format!(
                                "{context}: the server prepared no statement"
                            ))
                            .reject(promise);
                        }
                    },
                    Outcome::Success => self.settle_success(*command, acc),
                    Outcome::ServerError => {
                        let failure = acc.error.unwrap_or_else(|| ServerFailure {
                            errno: 0,
                            message: "the server rejected the command".to_string(),
                        });
                        let Request {
                            promise, context, ..
                        } = *command;
                        MysqlPromiseError {
                            message: format!("{context}: {}", failure.message),
                            code: crate::mysql2_error_code(failure.errno),
                            errno: (failure.errno != 0).then_some(failure.errno),
                        }
                        .reject(promise);
                    }
                    Outcome::Aborted(err) => {
                        let failure = match self.host_failure.clone() {
                            Some(reason) => Failure::from_host(reason),
                            None => Failure::from_core(err, self.ready, acc.error),
                        };
                        let Request {
                            promise, context, ..
                        } = *command;
                        failure.command_error(context).reject(promise);
                    }
                }
            }
        }
    }

    fn settle_success(&mut self, command: Request, acc: ResultAcc) {
        let Request {
            request,
            promise,
            answer,
            ..
        } = command;
        if answer == Answer::Undefined {
            promise.resolve_undefined();
            return;
        }
        // Row-returning-ness is decided from the SQL, not from what the server
        // sent back — the sqlx path chose `fetch_all` vs `execute` the same
        // way, so a `CALL` that returns rows still answers a ResultSetHeader
        // today and keeps doing so here.
        let outcome = if request.is_row_returning() {
            QueryOutcome::Rows(RawQueryResult {
                // The sqlx path derived the field list from `rows[0]`, so an
                // empty result set reported **no** fields at all. Reproduced:
                // a program reading `fields.length` after an empty SELECT sees
                // 0 today, and this change must not move that.
                columns: if acc.rows.is_empty() {
                    Vec::new()
                } else {
                    acc.reported.iter().map(OwnedColumn::describe).collect()
                },
                rows: acc.rows,
            })
        } else {
            QueryOutcome::Executed {
                affected_rows: acc.affected_rows,
                last_insert_id: acc.last_insert_id,
            }
        };
        let rows_as_array = request.rows_as_array;
        // Built on the MAIN thread, by the same function the sqlx path used, so
        // the tuple's shape and key order cannot drift between transports.
        promise.resolve_with(move || crate::outcome_to_jsvalue(&outcome, rows_as_array));
    }

    /// Settle everything this connection still owes. The backstop for a
    /// promise that no other exit reached.
    fn settle_all(&mut self, failure: &Failure) {
        if let Some(Active::Request { command, .. }) = self.active.take() {
            let Request {
                promise, context, ..
            } = *command;
            failure.command_error(context).reject(promise);
        }
        for command in std::mem::take(&mut self.queue) {
            match command {
                Command::Request(command) => {
                    let Request {
                        promise, context, ..
                    } = *command;
                    failure.command_error(context).reject(promise);
                }
                Command::CloseStatement(_) => {}
                // A queued `end()` on a connection that died first got what it
                // asked for: the connection is closed.
                Command::Quit(Some(promise)) => promise.resolve_undefined(),
                Command::Quit(None) => {}
            }
        }
        let connect_message = failure.connect_message();
        for waiter in std::mem::take(&mut self.waiting_ready) {
            waiter.promise.reject_string(&connect_message);
            // The JS handle was registered before the socket was opened so the
            // handshake would have something to resolve with. A handshake that
            // failed leaves it naming a connection that will never exist, and
            // nothing in JS ever received it, so nothing will ever release it —
            // retire it here or every refused connection leaks a registry slot.
            perry_ffi::drop_handle(waiter.handle);
        }
        if let Some(promise) = self.quit.take() {
            promise.reject_string(&format!("Failed to close: {}", failure.reason));
        }
    }

    fn fail_with(&mut self, reason: &str) {
        if self.finished {
            return;
        }
        self.host_failure = Some(reason.to_string());
        self.closing = true;
        self.conn.abort(Error::Transport);
        // Let the core's own terminal events run first: an in-flight command
        // gets `Completed { Aborted }` and then `Closed`, which is where most
        // of the settling happens. Whatever those did not answer for is caught
        // by `settle_all` below.
        let _ = self.drain_events();
        let failure = Failure::from_host(reason.to_string());
        self.settle_all(&failure);
        self.finished = true;
    }
}

impl DbCore for MysqlCore {
    fn transport_connected(&mut self) -> Result<(), String> {
        // MySQL's server speaks first: the core sits in its handshake state and
        // emits nothing until the greeting arrives. That is also why the first
        // command can always be accepted once the handshake finishes —
        // `Connection::accept` requires an empty output buffer, and nothing has
        // been written into it yet.
        Ok(())
    }

    fn receive(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.conn
            .receive(bytes)
            .map_err(|err| format!("MySQL protocol error: {err}"))
    }

    fn take_tls_request(&mut self) -> bool {
        std::mem::take(&mut self.tls_requested)
    }

    fn tls_established(&mut self, facts: &TlsFacts) -> Result<(), String> {
        // `facts` is deliberately unread. MySQL's authentication plugins define
        // no channel binding, and its TLS carries the MySQL protocol alone, so
        // there is no negotiated ALPN protocol either — nothing in the
        // handshake is an input to what the core does next. The acknowledgement
        // itself is what matters: it releases the handshake response, which
        // carries the credentials and is now written encrypted.
        let _ = facts;
        self.conn
            .tls_established()
            .map_err(|err| format!("MySQL TLS upgrade failed: {err}"))
    }

    fn drain(&mut self) -> Result<bool, String> {
        self.drain_events()
    }

    fn output(&self) -> &[u8] {
        self.conn.output()
    }

    fn consume_output(&mut self, n: usize) {
        // The driver only ever acknowledges bytes it took from `output()`, so
        // the core's range check cannot fail here; if it ever did, dropping the
        // acknowledgement would wedge the connection rather than corrupt it.
        let _ = self.conn.consume_output(n);
    }

    fn next_timeout_ms(&self) -> Option<u64> {
        let mut earliest = self.conn.next_timeout();
        let mut consider = |at: Instant| {
            earliest = Some(match earliest {
                Some(existing) => existing.min(at),
                None => at,
            });
        };
        if let Some(at) = self.kick_at {
            consider(at);
        }
        // A queued command has no `pending` entry in the core, so the core's
        // deadline does not cover it. Watching it here is what makes a query
        // stuck behind a slow one time out on its own schedule.
        for command in &self.queue {
            if let Command::Request(request) = command {
                consider(request.deadline);
            }
        }
        let now = Instant::now();
        earliest.map(|at| {
            if at <= now {
                0
            } else {
                at.duration_since(now).as_millis().min(u128::from(u64::MAX)) as u64
            }
        })
    }

    fn handle_timeout(&mut self) {
        let now = Instant::now();
        if self.kick_at.is_some_and(|at| at <= now) {
            self.kick_at = None;
        }
        let mut kept = VecDeque::with_capacity(self.queue.len());
        while let Some(command) = self.queue.pop_front() {
            match command {
                Command::Request(request) if request.deadline <= now => {
                    let Request { promise, .. } = *request;
                    MysqlPromiseError::message("Query timed out").reject(promise);
                }
                other => kept.push_back(other),
            }
        }
        self.queue = kept;
        self.conn.handle_timeout(now);
    }

    fn fail(&mut self, reason: &str) {
        self.fail_with(reason);
    }

    fn has_pending_work(&self) -> bool {
        self.active.is_some()
            || !self.queue.is_empty()
            || !self.waiting_ready.is_empty()
            || self.quit.is_some()
    }
}

/// The protocol config for one connection.
///
/// Separated from [`MysqlCore::new`] so the choices below are assertable
/// without a server: whether the handshake negotiates `CLIENT_SSL`, and the
/// capabilities sqlx negotiated that programs already depend on.
///
/// `tls` follows the `ssl` option alone, and turning it on also changes
/// **authentication**: `caching_sha2_password`'s full-auth path sends the
/// password as cleartext over the encrypted channel instead of RSA-OAEP
/// encrypting it, so a TLS connection never reaches `random_seed` and never
/// needs `/dev/urandom`. Compression stays off for the reason the sqlx path
/// never enabled it: a wire change with no caller asking for it.
pub(crate) fn protocol_config(config: &crate::MySqlConfig) -> Config {
    Config {
        user: config.user.clone(),
        password: config.password.clone().into_bytes(),
        database: config.database.clone(),
        // A server that does not offer `CLIENT_SSL` fails the connection here
        // rather than continuing in the clear: there is no "prefer TLS" mode,
        // because a client that asked for TLS and silently got none would send
        // its password in plaintext.
        tls: config.ssl.is_some(),
        compression: false,
        // sqlx negotiates `CLIENT_MULTI_STATEMENTS` (sqlx-mysql's
        // `stream.rs`), so `query("A; B")` works on the legacy transport today.
        // Keeping the capability keeps those programs working; the extra result
        // sets are concatenated in `ResultAcc`, which is what sqlx's
        // `fetch_all` did with them.
        multiple_statements: true,
        // sqlx does not request `CLIENT_LOCAL_FILES`, and the core aborts an
        // unsolicited `LOCAL INFILE` request without reading a file.
        local_infile: false,
        connect_deadline: Instant::now()
            .checked_add(Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS)),
        ..Config::default()
    }
}

/// The wall-clock expiry one request gets, taken at submission.
pub(crate) fn query_deadline() -> Instant {
    Instant::now()
        .checked_add(Duration::from_secs(DEFAULT_QUERY_TIMEOUT_SECS))
        .unwrap_or_else(Instant::now)
}

/// 20 fresh cryptographically random bytes for `caching_sha2_password`'s full
/// RSA exchange over a plaintext transport.
///
/// `turnloop_mysql` reads no entropy of its own — `RsaSeedNeeded` asks the host
/// for it — and this crate's production dependencies are `perry-ffi`,
/// `perry-db-turnloop` and the protocol crates, none of which exports a
/// random-bytes call. The OS device is therefore read directly rather than
/// pulling in a new dependency.
///
/// **A seed is used exactly once.** This reads fresh bytes on every call and
/// caches nothing: reusing an OAEP seed across two encryptions of the same
/// password is what makes the ciphertext distinguishable.
#[cfg(unix)]
fn random_seed() -> Result<[u8; 20], String> {
    use std::io::Read;
    let mut seed = [0u8; 20];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut device| device.read_exact(&mut seed))
        .map_err(|err| {
            format!(
                "MySQL caching_sha2_password authentication needs 20 random bytes \
                 and /dev/urandom could not be read ({err})"
            )
        })?;
    Ok(seed)
}

/// Fails rather than seeding deterministically.
///
/// A fixed seed would authenticate — the server cannot tell — while making the
/// encrypted password reproducible, so this refuses the connection and says
/// why. `mysql_native_password` and the fast `caching_sha2` path do not come
/// through here and keep working.
#[cfg(not(unix))]
fn random_seed() -> Result<[u8; 20], String> {
    Err(
        "MySQL caching_sha2_password authentication over a plaintext connection needs \
         cryptographic entropy, which this platform does not offer this binding"
            .to_string(),
    )
}
