//! Unit tests for the loop-driven mysql2 transport.
//!
//! Each test is named after the property it pins, and says why that property
//! matters. Several drive a real [`MysqlCore`] through a synthetic server
//! ([`Wire`]) rather than asserting on internals: a queue that is merely
//! *shaped* right proves nothing, and the commands this file must never lose
//! are only observable once they have actually been put on the wire.

use super::*;
use connection::{Answer, Command, Request};
use perry_db_turnloop::DbCore;
use perry_ffi::{js_array_get, js_array_length, ArrayHeader, JsValue};
use turnloop_mysql::{ColumnFlags, ColumnType, ColumnTypeInfo, RawValue as WireValue, Value};

use crate::{ParamValue, QueryRequest, RawValue};

// ── The registry slot ─────────────────────────────────────────────

#[test]
fn the_subsystem_slot_is_the_one_reserved_for_this_binding() {
    // Two bindings sharing a slot would route each other's completions: each is
    // a separately linked staticlib with its own sink, so the slot is the only
    // thing that tells them apart.
    assert_eq!(SUBSYSTEM, subsystem::MYSQL);
    assert_ne!(SUBSYSTEM, subsystem::PG);
    assert_ne!(SUBSYSTEM, subsystem::REDIS);
    assert_ne!(SUBSYSTEM, subsystem::MONGODB);
}

#[test]
fn registration_passes_the_abi_layout_check() {
    // The dev-dependency links the runtime, so this exercises the real
    // `register_sink`: a mismatch between perry-ffi's `NetCompletion` layout
    // digest and the runtime's refuses registration, leaves `available` false,
    // and would silently put every connection back on the sqlx transport. On an
    // agent with no loop — a `worker_threads` Worker, or the
    // `tokio-wait-driver` arm — this is false and that fallback is correct.
    assert!(
        super::register_only(),
        "a false here is an ABI layout mismatch between perry-ffi and perry-runtime"
    );
    assert!(perry_ffi::turnloop_net::sink_installed(SUBSYSTEM));
}

#[test]
fn the_pool_is_bounded_where_the_sqlx_pool_was() {
    // `MySqlPoolOptions::new().max_connections(10)`. A pool that quietly opened
    // more would move a limit a DBA has sized `max_connections` against.
    assert_eq!(pool::POOL_MAX, 10);
}

// ── A synthetic MySQL server ──────────────────────────────────────

/// Frames packet bodies the way a server would, tracking the sequence id the
/// core's codec expects. A wrong sequence is a protocol error in the core, so
/// this is not bookkeeping that can drift unnoticed.
struct Wire {
    seq: u8,
}

impl Wire {
    fn frame(&mut self, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 4);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes()[..3]);
        out.push(self.seq);
        self.seq = self.seq.wrapping_add(1);
        out.extend_from_slice(body);
        out
    }

    fn frames(&mut self, bodies: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for body in bodies {
            out.extend_from_slice(&self.frame(body));
        }
        out
    }
}

fn lenenc_int(out: &mut Vec<u8>, n: u64) {
    match n {
        0..=250 => out.push(n as u8),
        251..=0xffff => {
            out.push(0xfc);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        _ => {
            out.push(0xfd);
            out.extend_from_slice(&(n as u32).to_le_bytes()[..3]);
        }
    }
}

fn lenenc_str(out: &mut Vec<u8>, bytes: &[u8]) {
    lenenc_int(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// A protocol-41 greeting offering `mysql_native_password`, which the core
/// answers without asking the host for entropy.
fn greeting() -> Vec<u8> {
    greeting_with(0)
}

/// The same greeting with `CLIENT_SSL` offered, which is what lets a `tls`
/// config get past the core's "Server does not support secure connection".
fn greeting_offering_ssl() -> Vec<u8> {
    greeting_with(1 << 11)
}

fn greeting_with(extra: u32) -> Vec<u8> {
    const CAPS: u32 = 1          // LONG_PASSWORD
        | 1 << 2                 // LONG_FLAG
        | 1 << 9                 // PROTOCOL_41
        | 1 << 13                // TRANSACTIONS
        | 1 << 15                // SECURE_CONNECTION
        | 1 << 16                // MULTI_STATEMENTS
        | 1 << 17                // MULTI_RESULTS
        | 1 << 18                // PS_MULTI_RESULTS
        | 1 << 19                // PLUGIN_AUTH
        | 1 << 21; // PLUGIN_AUTH_LENENC_CLIENT_DATA
    let caps = CAPS | extra;
    let mut p = Vec::new();
    p.push(10);
    p.extend_from_slice(b"8.0.46\0");
    p.extend_from_slice(&7u32.to_le_bytes());
    p.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    p.push(0);
    p.extend_from_slice(&(caps as u16).to_le_bytes());
    p.push(45);
    p.extend_from_slice(&2u16.to_le_bytes());
    p.extend_from_slice(&((caps >> 16) as u16).to_le_bytes());
    p.push(21);
    p.extend_from_slice(&[0u8; 10]);
    p.extend_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0]);
    p.extend_from_slice(b"mysql_native_password\0");
    p
}

fn ok_packet(affected: u64, insert_id: u64) -> Vec<u8> {
    let mut p = vec![0x00];
    lenenc_int(&mut p, affected);
    lenenc_int(&mut p, insert_id);
    p.extend_from_slice(&2u16.to_le_bytes());
    p.extend_from_slice(&0u16.to_le_bytes());
    p
}

/// The legacy EOF the core negotiates on purpose (see the crate README).
fn eof_packet() -> Vec<u8> {
    vec![0xfe, 0, 0, 0, 0]
}

fn err_packet(errno: u16, message: &str) -> Vec<u8> {
    let mut p = vec![0xff];
    p.extend_from_slice(&errno.to_le_bytes());
    p.push(b'#');
    p.extend_from_slice(b"23000");
    p.extend_from_slice(message.as_bytes());
    p
}

fn column_def(name: &str, column_type: ColumnType, charset: u16) -> Vec<u8> {
    let mut p = Vec::new();
    lenenc_str(&mut p, b"def");
    lenenc_str(&mut p, b"");
    lenenc_str(&mut p, b"");
    lenenc_str(&mut p, b"");
    lenenc_str(&mut p, name.as_bytes());
    lenenc_str(&mut p, b"");
    p.push(0x0c);
    p.extend_from_slice(&charset.to_le_bytes());
    p.extend_from_slice(&64u32.to_le_bytes());
    p.push(column_type as u8);
    p.extend_from_slice(&0u16.to_le_bytes());
    p.push(0);
    p.extend_from_slice(&[0, 0]);
    p
}

fn text_row(cells: &[Option<&[u8]>]) -> Vec<u8> {
    let mut p = Vec::new();
    for cell in cells {
        match cell {
            None => p.push(0xfb),
            Some(bytes) => lenenc_str(&mut p, bytes),
        }
    }
    p
}

/// One text-protocol result set with a single VARCHAR column.
fn one_column_result(column: &str, values: &[&str]) -> Vec<Vec<u8>> {
    let mut packets = vec![
        vec![0x01],
        column_def(column, ColumnType::MYSQL_TYPE_VAR_STRING, 45),
    ];
    packets.push(eof_packet());
    for value in values {
        packets.push(text_row(&[Some(value.as_bytes())]));
    }
    packets.push(eof_packet());
    packets
}

struct Harness {
    core: MysqlCore,
    wire: Wire,
}

impl Harness {
    /// A core that has been through a full `mysql_native_password` handshake,
    /// so a queued command goes straight out.
    fn connected() -> Self {
        let mut core = MysqlCore::new(&crate::MySqlConfig::default()).expect("a default config");
        core.transport_connected()
            .expect("MySQL's server speaks first, so there is nothing to send yet");
        let mut wire = Wire { seq: 0 };
        let bytes = wire.frame(&greeting());
        core.receive(&bytes).expect("the greeting parses");
        core.drain().expect("the handshake response is produced");
        let response = take_output(&mut core);
        assert!(
            !response.is_empty(),
            "the core must answer the greeting, or nothing is being tested"
        );
        // The client's response occupies sequence 1.
        wire.seq = 2;
        let bytes = wire.frame(&ok_packet(0, 0));
        core.receive(&bytes).expect("the auth OK parses");
        core.drain().expect("authentication completes");
        assert!(core.is_ready(), "the handshake must have finished");
        Self { core, wire }
    }

    /// Bytes the core wants on the wire, acknowledged exactly as
    /// `Registry::flush` does the moment turnloop takes ownership of them.
    fn written(&mut self) -> Vec<u8> {
        take_output(&mut self.core)
    }

    /// Answer the command now on the wire, then drain.
    fn serve(&mut self, bodies: &[Vec<u8>]) {
        // A command resets the sequence id, so a reply always starts at 1.
        self.wire.seq = 1;
        let bytes = self.wire.frames(bodies);
        self.core.receive(&bytes).expect("the reply parses");
        self.core.drain().expect("the reply is consumed");
    }

    fn enqueue(&mut self, sql: &str, answer: Answer) -> perry_ffi::JsPromise {
        let promise = perry_ffi::JsPromise::new();
        let raw = promise.as_raw();
        self.core.enqueue(Command::Request(Box::new(Request {
            request: QueryRequest::new(sql.to_string(), Vec::new(), false, false),
            promise,
            deadline: connection::query_deadline(),
            context: "Query failed",
            answer,
        })));
        // SAFETY: the promise is not resolved yet, and this clone is only read
        // back through `js_promise_state` / `js_promise_value`.
        unsafe { perry_ffi::JsPromise::from_raw(raw) }
    }
}

fn take_output(core: &mut MysqlCore) -> Vec<u8> {
    let bytes = core.output().to_vec();
    core.consume_output(bytes.len());
    bytes
}

fn state(promise: &perry_ffi::JsPromise) -> i32 {
    perry_runtime::promise::js_promise_state(promise.as_raw().cast())
}

fn value(promise: &perry_ffi::JsPromise) -> JsValue {
    JsValue::from_bits(perry_runtime::promise::js_promise_value(promise.as_raw().cast()).to_bits())
}

fn rejection_message(promise: &perry_ffi::JsPromise) -> String {
    let reason = perry_runtime::promise::js_promise_reason(promise.as_raw().cast());
    let reason = JsValue::from_bits(reason.to_bits());
    // SAFETY: a rejected promise's reason is a live runtime Error object.
    unsafe {
        crate::jsvalue_to_string(crate::object_field_by_name(reason, "message")).unwrap_or_default()
    }
}

/// `[rows, fields]` → the `rows` array's length.
fn row_count(promise: &perry_ffi::JsPromise) -> u32 {
    let tuple = value(promise).as_pointer::<ArrayHeader>();
    assert!(!tuple.is_null(), "a fulfilled query resolves an array");
    // SAFETY: the value under test is the tuple the result builder produced.
    unsafe {
        let rows = js_array_get(tuple, 0).as_pointer::<ArrayHeader>();
        js_array_length(rows)
    }
}

/// The COM_QUERY payloads in `bytes`, in wire order.
fn text_commands(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        let size =
            bytes[at] as usize | (bytes[at + 1] as usize) << 8 | (bytes[at + 2] as usize) << 16;
        let body = &bytes[at + 4..at + 4 + size];
        if body.first() == Some(&0x03) {
            out.push(String::from_utf8_lossy(&body[1..]).into_owned());
        }
        at += 4 + size;
    }
    out
}

// ── The command queue ─────────────────────────────────────────────

#[test]
fn a_connection_issues_one_command_at_a_time_in_submission_order() {
    // THE property of this file. MySQL has no pipelining: `Connection::accept`
    // refuses a second command while one is outstanding, so a binding that
    // forwards every `conn.query()` straight to the core would drop, reorder or
    // spuriously reject everything a program fires without awaiting. Node's
    // mysql2 accepts exactly that and answers all of them, in order.
    let mut harness = Harness::connected();
    let first = harness.enqueue("SELECT 1", Answer::ResultTuple);
    let second = harness.enqueue("SELECT 2", Answer::ResultTuple);
    let third = harness.enqueue("SELECT 3", Answer::ResultTuple);

    // Only the first is on the wire; the other two are still owed an answer.
    assert_eq!(text_commands(&harness.written()), vec!["SELECT 1"]);
    assert_eq!(state(&first), 0);
    assert_eq!(state(&second), 0);
    assert_eq!(state(&third), 0);

    harness.serve(&one_column_result("a", &["1"]));
    assert_eq!(state(&first), 1, "the first query must resolve");
    assert_eq!(row_count(&first), 1);
    // …and only then does the second reach the wire.
    assert_eq!(text_commands(&harness.written()), vec!["SELECT 2"]);
    assert_eq!(state(&second), 0);

    harness.serve(&one_column_result("a", &["2", "3"]));
    assert_eq!(state(&second), 1);
    assert_eq!(row_count(&second), 2);
    assert_eq!(text_commands(&harness.written()), vec!["SELECT 3"]);

    harness.serve(&one_column_result("a", &[]));
    assert_eq!(state(&third), 1);
    assert_eq!(row_count(&third), 0);
}

#[test]
fn a_failed_command_does_not_strand_the_ones_queued_behind_it() {
    // A server error settles one command and must leave the connection usable:
    // the sqlx path got this free by holding a connection per call, and a queue
    // that stalled on the first `ER_DUP_ENTRY` would hang every later `await`.
    let mut harness = Harness::connected();
    let failing = harness.enqueue("INSERT INTO t VALUES (1)", Answer::ResultTuple);
    let following = harness.enqueue("SELECT 1", Answer::ResultTuple);
    let _ = harness.written();

    harness.serve(&[err_packet(1062, "Duplicate entry '1' for key 't.PRIMARY'")]);
    assert_eq!(state(&failing), 2);
    assert_eq!(
        rejection_message(&failing),
        "Query failed: Duplicate entry '1' for key 't.PRIMARY'",
        "the sqlx path's `Query failed: ` prefix is part of the JS surface"
    );
    assert_eq!(text_commands(&harness.written()), vec!["SELECT 1"]);

    harness.serve(&one_column_result("a", &["1"]));
    assert_eq!(state(&following), 1);
}

#[test]
fn a_transaction_keeps_every_statement_on_the_connection_it_was_submitted_to() {
    // `beginTransaction` / `commit` are plain SQL, so the only thing making a
    // transaction atomic is that all three statements run on one connection in
    // submission order. A `Connection` handle owns one loop-driven connection
    // for life and this queue is FIFO, so that falls out — but it falls out of
    // code, not of a guarantee, which is why it is asserted rather than assumed.
    let mut harness = Harness::connected();
    let begin = harness.enqueue("START TRANSACTION", Answer::Undefined);
    let insert = harness.enqueue("INSERT INTO t VALUES (1)", Answer::ResultTuple);
    let commit = harness.enqueue("COMMIT", Answer::Undefined);

    assert_eq!(text_commands(&harness.written()), vec!["START TRANSACTION"]);
    harness.serve(&[ok_packet(0, 0)]);
    assert_eq!(state(&begin), 1);
    assert!(
        value(&begin).is_undefined(),
        "beginTransaction resolved undefined under sqlx and must keep doing so"
    );

    assert_eq!(
        text_commands(&harness.written()),
        vec!["INSERT INTO t VALUES (1)"]
    );
    harness.serve(&[ok_packet(1, 42)]);
    assert_eq!(state(&insert), 1);

    assert_eq!(text_commands(&harness.written()), vec!["COMMIT"]);
    harness.serve(&[ok_packet(0, 0)]);
    assert_eq!(state(&commit), 1);
}

#[test]
fn a_non_select_resolves_the_result_set_header_the_sqlx_path_built() {
    // `[ResultSetHeader, []]` with `affectedRows` / `insertId` is what every
    // mysql2 write path reads; Drizzle's insert mapper reads `insertId`.
    let mut harness = Harness::connected();
    let insert = harness.enqueue("INSERT INTO t VALUES (1)", Answer::ResultTuple);
    let _ = harness.written();
    harness.serve(&[ok_packet(3, 99)]);
    assert_eq!(state(&insert), 1);

    let tuple = value(&insert).as_pointer::<ArrayHeader>();
    // SAFETY: the value under test is the tuple the result builder produced.
    unsafe {
        let header = js_array_get(tuple, 0);
        assert_eq!(
            crate::object_field_by_name(header, "affectedRows").to_number(),
            3.0
        );
        assert_eq!(
            crate::object_field_by_name(header, "insertId").to_number(),
            99.0
        );
        let fields = js_array_get(tuple, 1).as_pointer::<ArrayHeader>();
        assert_eq!(js_array_length(fields), 0);
    }
}

#[test]
fn tearing_down_a_connection_settles_every_promise_it_still_owes() {
    // A dropped `JsPromise` never settles and no caller can recover from it.
    // This is the backstop for the transport dying with work outstanding — the
    // shape `Registry::abort` produces on ECONNRESET.
    let mut harness = Harness::connected();
    let active = harness.enqueue("SELECT 1", Answer::ResultTuple);
    let queued = harness.enqueue("SELECT 2", Answer::ResultTuple);
    let also_queued = harness.enqueue("SELECT 3", Answer::Undefined);
    let _ = harness.written();

    harness.core.fail("ECONNRESET read -54");
    for promise in [&active, &queued, &also_queued] {
        assert_eq!(state(promise), 2, "every outstanding promise must settle");
        assert!(
            rejection_message(promise).contains("ECONNRESET"),
            "the driver's reason must survive, not be replaced by a generic one: {}",
            rejection_message(promise)
        );
    }
    assert!(
        !harness.core.has_pending_work(),
        "a failed connection owes nothing, so it must stop holding the process open"
    );
}

#[test]
fn a_connection_that_never_finished_its_handshake_rejects_its_creator() {
    // `createConnection` resolves only once the server has accepted the
    // credentials. A connection that dies mid-handshake must reject rather than
    // leave `await mysql.createConnection(...)` hanging.
    let mut core = MysqlCore::new(&crate::MySqlConfig::default()).expect("a default config");
    let promise = perry_ffi::JsPromise::new();
    let raw = promise.as_raw();
    core.park_ready(promise, 1);
    assert!(core.has_pending_work());
    core.fail("ECONNREFUSED connect -61");
    // SAFETY: read-only inspection of the promise just settled above.
    let promise = unsafe { perry_ffi::JsPromise::from_raw(raw) };
    assert_eq!(state(&promise), 2);
    assert_eq!(
        rejection_message(&promise),
        "Failed to connect: ECONNREFUSED connect -61"
    );
}

#[test]
fn an_empty_select_reports_no_fields_as_the_sqlx_path_did() {
    // `raws_from_mysql_rows` took its column list from `rows[0]`, so a SELECT
    // matching nothing answered `fields: []`. Node's mysql2 reports the real
    // fields there; reproducing the old answer is deliberate, because programs
    // read `fields.length` today and this change must not move it.
    let mut harness = Harness::connected();
    let empty = harness.enqueue("SELECT a FROM t WHERE 0", Answer::ResultTuple);
    let _ = harness.written();
    harness.serve(&one_column_result("a", &[]));
    let tuple = value(&empty).as_pointer::<ArrayHeader>();
    // SAFETY: the value under test is the tuple the result builder produced.
    unsafe {
        assert_eq!(js_array_length(js_array_get(tuple, 0).as_pointer()), 0);
        assert_eq!(js_array_length(js_array_get(tuple, 1).as_pointer()), 0);
    }
}

/// `COM_STMT_PREPARE` reply for a statement with one parameter and one column.
fn prepare_reply(statement_id: u32) -> Vec<Vec<u8>> {
    let mut stmt = vec![0x00];
    stmt.extend_from_slice(&statement_id.to_le_bytes());
    stmt.extend_from_slice(&1u16.to_le_bytes());
    stmt.extend_from_slice(&1u16.to_le_bytes());
    stmt.push(0);
    stmt.extend_from_slice(&0u16.to_le_bytes());
    vec![
        stmt,
        column_def("?", ColumnType::MYSQL_TYPE_VAR_STRING, 63),
        eof_packet(),
        column_def("a", ColumnType::MYSQL_TYPE_VAR_STRING, 45),
        eof_packet(),
    ]
}

/// `COM_STMT_EXECUTE` reply: one binary row holding one VARCHAR.
fn execute_reply(value: &str) -> Vec<Vec<u8>> {
    let mut row = vec![0x00, 0x00];
    lenenc_str(&mut row, value.as_bytes());
    vec![
        vec![0x01],
        column_def("a", ColumnType::MYSQL_TYPE_VAR_STRING, 45),
        eof_packet(),
        row,
        eof_packet(),
    ]
}

/// The first byte of every command body in `bytes`, in wire order. 0x03 is
/// COM_QUERY, 0x16 COM_STMT_PREPARE, 0x17 COM_STMT_EXECUTE, 0x19 COM_STMT_CLOSE.
fn command_codes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 4 <= bytes.len() {
        let size =
            bytes[at] as usize | (bytes[at + 1] as usize) << 8 | (bytes[at + 2] as usize) << 16;
        if let Some(code) = bytes[at + 4..at + 4 + size].first() {
            out.push(*code);
        }
        at += 4 + size;
    }
    out
}

impl Harness {
    fn enqueue_prepared(&mut self, sql: &str) -> perry_ffi::JsPromise {
        let promise = perry_ffi::JsPromise::new();
        let raw = promise.as_raw();
        self.core.enqueue(Command::Request(Box::new(Request {
            request: QueryRequest::new(
                sql.to_string(),
                vec![ParamValue::String("x".into())],
                false,
                true,
            ),
            promise,
            deadline: connection::query_deadline(),
            context: "Query failed",
            answer: Answer::ResultTuple,
        })));
        // SAFETY: the promise is not resolved yet, and this clone is only read
        // back through `js_promise_state` / `js_promise_value`.
        unsafe { perry_ffi::JsPromise::from_raw(raw) }
    }
}

#[test]
fn a_bound_query_prepares_then_executes_on_the_same_connection() {
    // `execute()` is a prepared statement, always — that is the whole
    // difference from `query()` and what `force_prepared` encodes. The prepare
    // and the execute are two wire commands, and they must land on the one
    // connection the request was submitted to: an execute naming a statement id
    // the other connection prepared is `ER_UNKNOWN_STMT_HANDLER`.
    let mut harness = Harness::connected();
    let bound = harness.enqueue_prepared("SELECT a FROM t WHERE b = ?");
    assert_eq!(command_codes(&harness.written()), vec![0x16]);
    assert_eq!(state(&bound), 0);

    harness.serve(&prepare_reply(1));
    assert_eq!(
        command_codes(&harness.written()),
        vec![0x17],
        "the execute must follow the prepare without waiting for anything else"
    );
    assert_eq!(state(&bound), 0, "a prepare does not answer the caller");

    harness.serve(&execute_reply("hello"));
    assert_eq!(state(&bound), 1);
    assert_eq!(row_count(&bound), 1);
}

#[test]
fn the_same_sql_prepares_once_per_connection() {
    // Node's mysql2 caches prepared statements per connection, keyed by SQL.
    // Re-preparing every call is a wasted round trip on the hot path — the one
    // drizzle takes for every parameterised select.
    let mut harness = Harness::connected();
    let first = harness.enqueue_prepared("SELECT a FROM t WHERE b = ?");
    let _ = harness.written();
    harness.serve(&prepare_reply(1));
    let _ = harness.written();
    harness.serve(&execute_reply("hello"));
    assert_eq!(state(&first), 1);

    let second = harness.enqueue_prepared("SELECT a FROM t WHERE b = ?");
    assert_eq!(
        command_codes(&harness.written()),
        vec![0x17],
        "a cached statement must execute straight away, with no second prepare"
    );
    harness.serve(&execute_reply("again"));
    assert_eq!(state(&second), 1);
}

#[test]
fn the_statement_cache_is_bounded_and_closes_what_it_evicts() {
    // Unbounded caching walks the server's `max_prepared_stmt_count` (16382 by
    // default) for a program that builds SQL by interpolation, and every
    // statement past it fails with ER_MAX_PREPARED_STMT_COUNT_REACHED. The
    // eviction also exercises the `kick_at` path: COM_STMT_CLOSE gets no reply
    // at all, so without a deadline of our own the connection would sit on a
    // command that can never complete and a queue that can never advance.
    let mut harness = Harness::connected();
    let mut closed = Vec::new();
    for statement in 0..34u32 {
        let promise = harness.enqueue_prepared(&format!("SELECT a FROM t WHERE b{statement} = ?"));
        closed.extend(command_codes(&harness.written()));
        harness.serve(&prepare_reply(statement + 1));
        closed.extend(command_codes(&harness.written()));
        harness.serve(&execute_reply("row"));
        assert_eq!(
            state(&promise),
            1,
            "statement {statement} must still answer"
        );
        // Whatever the eviction queued goes out once the execute is done.
        let after = harness.written();
        closed.extend(command_codes(&after));
        if command_codes(&after).contains(&0x19) {
            // The close is a no-response command: one more turn completes it.
            harness.core.handle_timeout();
            harness.core.drain().expect("the close completes");
        }
    }
    assert!(
        closed.contains(&0x19),
        "a 33rd distinct statement must evict one and close it on the wire"
    );
    assert!(
        harness.core.is_idle(),
        "the connection must be free again once the close has completed"
    );
}

// ── Type conversion ───────────────────────────────────────────────

fn info(column_type: ColumnType) -> ColumnTypeInfo {
    ColumnTypeInfo {
        column_type,
        flags: ColumnFlags::empty(),
        character_set: 45,
    }
}

fn binary(column_type: ColumnType) -> ColumnTypeInfo {
    ColumnTypeInfo {
        column_type,
        flags: ColumnFlags::empty(),
        character_set: 63,
    }
}

#[test]
fn bigint_and_decimal_decode_as_numbers_not_strings() {
    // `turnloop_mysql::types::decode`'s default answers a DECIMAL as an exact
    // string and can answer a BIGINT as one too. The sqlx path read both
    // through `try_get::<f64>` / `try_get::<i64>`, so JS has always seen
    // numbers — lossily for a BIGINT past 2^53, which this reproduces rather
    // than silently starts fixing.
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_LONGLONG),
            WireValue::Bytes(b"9007199254740993")
        ),
        RawValue::Float64(9007199254740993_i64 as f64)
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_LONGLONG),
            WireValue::Scalar(Value::UInt(18446744073709551615))
        ),
        RawValue::Float64(18446744073709551615_u64 as f64)
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_NEWDECIMAL),
            WireValue::Bytes(b"1.2300")
        ),
        RawValue::Float64(1.23)
    );
}

#[test]
fn dates_keep_the_sqlx_format_and_drop_sub_second_precision() {
    // `chrono::NaiveDateTime` formatted with `%Y-%m-%d %H:%M:%S` never printed
    // microseconds, in either protocol. A DATETIME(6) therefore still answers
    // whole seconds; printing them now would change a string programs compare.
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_DATE),
            WireValue::Bytes(b"2026-09-16")
        ),
        RawValue::String("2026-09-16".into())
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_DATETIME),
            WireValue::Bytes(b"2026-09-16 04:05:06.123456")
        ),
        RawValue::String("2026-09-16 04:05:06".into())
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_TIMESTAMP),
            WireValue::Scalar(Value::Date(2026, 9, 16, 4, 5, 6, 123_456))
        ),
        RawValue::String("2026-09-16 04:05:06".into())
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_DATE),
            WireValue::Scalar(Value::Date(2026, 9, 16, 0, 0, 0, 0))
        ),
        RawValue::String("2026-09-16".into())
    );
}

#[test]
fn a_zero_date_is_null_as_it_was_under_sqlx() {
    // MySQL's `0000-00-00` is not a `chrono` date, so `try_get` failed and the
    // cell read `null`. A program storing zero dates sees `null` today.
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_DATE),
            WireValue::Bytes(b"0000-00-00")
        ),
        RawValue::Null
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_DATETIME),
            WireValue::Scalar(Value::Date(0, 0, 0, 0, 0, 0, 0))
        ),
        RawValue::Null
    );
}

#[test]
fn an_out_of_range_time_is_null_as_it_was_under_sqlx() {
    // MySQL's TIME spans -838:59:59..=838:59:59; `chrono::NaiveTime` cannot
    // hold either end, so sqlx's decode failed and the cell read `null`. Node's
    // mysql2 would answer the string — a pre-existing divergence this change
    // deliberately does not move.
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_TIME),
            WireValue::Bytes(b"12:34:56")
        ),
        RawValue::String("12:34:56".into())
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_TIME),
            WireValue::Bytes(b"-01:00:00")
        ),
        RawValue::Null
    );
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_TIME),
            WireValue::Scalar(Value::Time(false, 2, 3, 0, 0, 0))
        ),
        RawValue::Null,
        "51 hours is legal MySQL TIME and was null under sqlx"
    );
}

#[test]
fn blob_and_binary_columns_decode_as_lossy_strings_not_buffers() {
    // The sqlx catch-all tried `String` and then `Vec<u8>` lossily, so a BLOB
    // has always reached JS as a string. `turnloop_mysql`'s default policy
    // answers a Buffer there; handing one over would change the type of every
    // BLOB column a program reads.
    assert_eq!(
        convert::decode(
            binary(ColumnType::MYSQL_TYPE_BLOB),
            WireValue::Bytes(&[0xff, 0x41])
        ),
        RawValue::String("\u{fffd}A".into())
    );
    assert_eq!(
        convert::decode(
            binary(ColumnType::MYSQL_TYPE_STRING),
            WireValue::Bytes(b"raw")
        ),
        RawValue::String("raw".into())
    );
    assert_eq!(
        convert::decode(
            binary(ColumnType::MYSQL_TYPE_BIT),
            WireValue::Bytes(&[0x01])
        ),
        RawValue::String("\u{1}".into())
    );
}

#[test]
fn json_columns_decode_to_a_document_with_the_stored_key_order() {
    // Node's mysql2 hands back the parsed document, and drizzle's `json()`
    // mapper relies on it. Key order is the stored order, which needs
    // serde_json's `preserve_order`: alphabetising a document that round-trips
    // through the database is a difference nobody can explain later.
    let decoded = convert::decode(
        info(ColumnType::MYSQL_TYPE_JSON),
        WireValue::Bytes(br#"{"z":1,"a":[2,3]}"#),
    );
    let RawValue::Json(document) = decoded else {
        panic!("a JSON column must decode to a document, got {decoded:?}");
    };
    let keys: Vec<&str> = document
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["z", "a"]);

    // A document the parser rejects reads `null`, which is what
    // `try_get::<serde_json::Value>` did.
    assert_eq!(
        convert::decode(
            info(ColumnType::MYSQL_TYPE_JSON),
            WireValue::Bytes(b"{not json")
        ),
        RawValue::Null
    );
}

#[test]
fn year_columns_now_decode_as_numbers_where_the_sqlx_path_produced_null() {
    // A named departure. The sqlx arm had no YEAR case, and neither `String`
    // nor `Vec<u8>` is a legal sqlx decode target for it, so every YEAR column
    // read back as `null`. Answering the number is what Node's mysql2 does.
    assert_eq!(
        convert::decode(info(ColumnType::MYSQL_TYPE_YEAR), WireValue::Bytes(b"2026")),
        RawValue::Float64(2026.0)
    );
}

#[test]
fn a_null_cell_is_null_for_every_column_type() {
    // The wire says NULL directly in both protocols; nothing below may turn one
    // into an empty string or a zero.
    for column_type in [
        ColumnType::MYSQL_TYPE_LONG,
        ColumnType::MYSQL_TYPE_VAR_STRING,
        ColumnType::MYSQL_TYPE_DATETIME,
        ColumnType::MYSQL_TYPE_JSON,
        ColumnType::MYSQL_TYPE_BLOB,
    ] {
        assert_eq!(
            convert::decode(info(column_type), WireValue::Null),
            RawValue::Null
        );
    }
}

#[test]
fn every_column_type_round_trips_to_the_field_packet_id_mysql2_reports() {
    // `field.type` / `field.columnType` come from the result builder's
    // name → id table. This binding names a column from its wire type, so the
    // round trip has to land back on the same number the wire carried, or every
    // mysql2 consumer reading `field.type` sees a different value than it did
    // under sqlx.
    for column_type in [
        ColumnType::MYSQL_TYPE_TINY,
        ColumnType::MYSQL_TYPE_SHORT,
        ColumnType::MYSQL_TYPE_LONG,
        ColumnType::MYSQL_TYPE_FLOAT,
        ColumnType::MYSQL_TYPE_DOUBLE,
        ColumnType::MYSQL_TYPE_NULL,
        ColumnType::MYSQL_TYPE_TIMESTAMP,
        ColumnType::MYSQL_TYPE_LONGLONG,
        ColumnType::MYSQL_TYPE_INT24,
        ColumnType::MYSQL_TYPE_DATE,
        ColumnType::MYSQL_TYPE_TIME,
        ColumnType::MYSQL_TYPE_DATETIME,
        ColumnType::MYSQL_TYPE_YEAR,
        ColumnType::MYSQL_TYPE_BIT,
        ColumnType::MYSQL_TYPE_JSON,
        ColumnType::MYSQL_TYPE_NEWDECIMAL,
        ColumnType::MYSQL_TYPE_ENUM,
        ColumnType::MYSQL_TYPE_SET,
        ColumnType::MYSQL_TYPE_TINY_BLOB,
        ColumnType::MYSQL_TYPE_MEDIUM_BLOB,
        ColumnType::MYSQL_TYPE_LONG_BLOB,
        ColumnType::MYSQL_TYPE_BLOB,
        ColumnType::MYSQL_TYPE_VAR_STRING,
        ColumnType::MYSQL_TYPE_STRING,
        ColumnType::MYSQL_TYPE_GEOMETRY,
    ] {
        let name = convert::sqlx_type_name(info(column_type));
        assert_eq!(
            crate::mysql_type_id_from_name(name),
            f64::from(column_type as u8),
            "{name} must map back to its own wire id"
        );
        // Unsigned integers carry the same numeric type; only the flags differ,
        // exactly as Node's mysql2 reports them.
        let unsigned = ColumnTypeInfo {
            flags: ColumnFlags::UNSIGNED_FLAG,
            ..info(column_type)
        };
        assert_eq!(
            crate::mysql_type_id_from_name(convert::sqlx_type_name(unsigned)),
            f64::from(column_type as u8)
        );
    }
}

#[test]
fn bind_values_carry_every_supported_parameter_shape() {
    // One-for-one with the sqlx `query.bind(..)` chain. A bool binds as MySQL's
    // TINYINT 0/1, which is what sqlx encoded; anything else would make
    // `WHERE flag = ?` stop matching.
    let values = convert::bind_values(&[
        ParamValue::Null,
        ParamValue::String("hi".into()),
        ParamValue::Bytes(vec![0, 255]),
        ParamValue::Int(-7),
        ParamValue::Number(3.25),
        ParamValue::Bool(true),
        ParamValue::Bool(false),
        ParamValue::DateTime(
            chrono::NaiveDate::from_ymd_opt(2026, 9, 16)
                .unwrap()
                .and_hms_micro_opt(4, 5, 6, 789_000)
                .unwrap(),
        ),
    ]);
    assert_eq!(
        values,
        vec![
            Value::NULL,
            Value::Bytes(b"hi".to_vec()),
            Value::Bytes(vec![0, 255]),
            Value::Int(-7),
            Value::Double(3.25),
            Value::Int(1),
            Value::Int(0),
            Value::Date(2026, 9, 16, 4, 5, 6, 789_000),
        ]
    );
}

#[test]
fn the_config_asks_for_no_tls_and_no_compression() {
    // A config with no `ssl` option is plaintext, which is every MySQL
    // connection Perry has opened until now and stays the default. Asking for
    // TLS here would make the core emit an `SSLRequest` no caller wanted;
    // compression is a wire change with no caller. Multi-statement stays on
    // because sqlx negotiated it and `query("A; B")` works today.
    let config = connection::protocol_config(&crate::MySqlConfig::default());
    assert!(!config.tls);
    assert!(!config.compression);
    assert!(!config.local_infile);
    assert!(config.multiple_statements);
    assert!(
        config.connect_deadline.is_some(),
        "a connect with no deadline is a connect that can hang forever"
    );
}

// ── TLS ───────────────────────────────────────────────────────────

#[test]
fn an_ssl_option_makes_the_core_negotiate_tls() {
    // `tls` is what makes the core emit `SSLRequest` and `UpgradeTls` at all. A
    // config carrying an `ssl` option whose core stayed plaintext would connect
    // happily and send the password in the clear, which is the failure this
    // whole path exists to prevent.
    let config = connection::protocol_config(&crate::MySqlConfig {
        ssl: Some(crate::MySqlSslConfig::default()),
        ..Default::default()
    });
    assert!(config.tls);
}

#[test]
fn the_tls_options_name_the_host_unless_the_config_names_another() {
    let mut config = crate::MySqlConfig {
        host: "db.example.com".to_string(),
        ssl: Some(crate::MySqlSslConfig::default()),
        ..Default::default()
    };
    let options = tls_options(&config).expect("an ssl config installs a session");
    // The certificate is verified against the host being connected to, and no
    // ALPN protocol is offered.
    assert_eq!(options.servername, "db.example.com");
    assert!(options.reject_unauthorized);
    assert!(options.alpn.is_empty());
    assert!(options.ca_pem.is_empty());

    config.ssl = Some(crate::MySqlSslConfig {
        reject_unauthorized: false,
        ca: b"-----BEGIN CERTIFICATE-----\n".to_vec(),
        servername: Some("primary.internal".to_string()),
    });
    let options = tls_options(&config).expect("an ssl config installs a session");
    assert_eq!(options.servername, "primary.internal");
    assert!(!options.reject_unauthorized);
    assert_eq!(options.ca_pem, b"-----BEGIN CERTIFICATE-----\n".to_vec());

    // A plaintext config installs nothing, which is what makes an `UpgradeTls`
    // on such a connection a driver error instead of a silent downgrade.
    config.ssl = None;
    assert!(tls_options(&config).is_none());
}

#[test]
fn a_tls_connection_sends_only_the_ssl_request_before_the_upgrade() {
    // The whole mid-stream contract, in order: the core answers the greeting
    // with an `SSLRequest` packet and NOTHING else, asks for the upgrade
    // exactly once, and writes the handshake response — which carries the
    // credentials — only after the session is acknowledged. A core that wrote
    // the response alongside the request would put the password on the wire in
    // plaintext.
    let mut core = MysqlCore::new(&crate::MySqlConfig {
        ssl: Some(crate::MySqlSslConfig::default()),
        ..Default::default()
    })
    .expect("a config asking for TLS");
    core.transport_connected()
        .expect("MySQL's server speaks first, so there is nothing to send yet");
    let mut wire = Wire { seq: 0 };
    let bytes = wire.frame(&greeting_offering_ssl());
    core.receive(&bytes).expect("the greeting parses");
    core.drain().expect("the greeting is consumed");

    assert!(core.take_tls_request(), "the core must ask for the upgrade");
    assert!(
        !core.take_tls_request(),
        "the request is taken once, or the driver installs a second session"
    );

    let request = take_output(&mut core);
    // One packet: a 4-byte header and the 32-byte `SSLRequest` body. Anything
    // longer is the handshake response having gone out unencrypted.
    assert_eq!(
        request.len(),
        36,
        "only the SSLRequest may precede the upgrade"
    );
    let client_caps = u32::from_le_bytes([request[4], request[5], request[6], request[7]]);
    assert_ne!(
        client_caps & (1 << 11),
        0,
        "the SSLRequest must claim CLIENT_SSL, or the server keeps reading plaintext"
    );

    // The driver acknowledges only once the request above has been flushed;
    // the core refuses while it still has output, so this is the real ordering.
    core.tls_established(&perry_db_turnloop::TlsFacts::default())
        .expect("the acknowledgement releases the handshake response");
    let response = take_output(&mut core);
    assert!(
        !response.is_empty(),
        "the handshake response is written after the upgrade, not before"
    );

    // Greeting 0, SSLRequest 1, handshake response 2 — so the server's OK is 3.
    wire.seq = 3;
    let bytes = wire.frame(&ok_packet(0, 0));
    core.receive(&bytes).expect("the auth OK parses");
    core.drain().expect("authentication completes");
    assert!(
        core.is_ready(),
        "a TLS handshake must finish the connection, not just start it"
    );
}

#[test]
fn a_plaintext_config_never_asks_for_an_upgrade() {
    // The counter-case: the very same greeting, offering `CLIENT_SSL`, must not
    // move a config that asked for no TLS. Without this the test above would
    // pass on a core that upgraded whenever the server allowed it, and a
    // program that never mentioned `ssl` would silently change transports.
    let mut core = MysqlCore::new(&crate::MySqlConfig::default()).expect("a default config");
    core.transport_connected().expect("the server speaks first");
    let mut wire = Wire { seq: 0 };
    let bytes = wire.frame(&greeting_offering_ssl());
    core.receive(&bytes).expect("the greeting parses");
    core.drain().expect("the greeting is consumed");

    assert!(!core.take_tls_request());
    let response = take_output(&mut core);
    assert!(
        response.len() > 36,
        "a plaintext config answers the greeting with its handshake response, \
         not with a 36-byte SSLRequest"
    );
}
