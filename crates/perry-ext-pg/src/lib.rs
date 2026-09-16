//! Native bindings for the npm `pg` PostgreSQL client — uses only
//! perry-ffi.
//!
//! Since turnloop P7 a connection is **loop-driven state**: one turnloop socket
//! and a `turnloop_postgres::Connection` sans-I/O core, driven from the event
//! loop's own completion dispatch (`turnloop_io`). No thread is held at any
//! point. The legacy transport — `sqlx::postgres` bridged through
//! `spawn_blocking` + `tokio::Handle::current().block_on`, which borrowed a
//! tokio blocking-pool thread for every round trip — remains for the
//! connections that decline: a `worker_threads` agent (no loop of its own), the
//! `tokio-wait-driver` A/B arm, and any config whose `host` names a
//! Unix-domain socket, which this transport cannot reach.
//!
//! Mirrors perry-stdlib's existing surface: `Client` (pre-connect
//! / connected handle states with `.connect()` deferring the TCP
//! handshake), `Pool` (lazy `connect_lazy`-style + eager
//! `pg.createPool`), parameterized `query()` with `Null`/`String`/
//! `Number`/`Int`/`Bool` param types, result objects with
//! `rows`/`fields`/`rowCount`/`command` keys, row objects keyed by
//! column name. BigInt param support deferred — perry-ffi's BigInt
//! surface is in place (v0.5.556) but the JS-side array iteration
//! shape needs an extra adapter; followup once any wrapper actually
//! demands it.

mod turnloop_io;

/// Production binaries receive the async-bridge symbols from perry-stdlib; a
/// standalone `cargo test -p perry-ext-pg` binary has no stdlib archive, so it
/// supplies its own. Same file as `perry-ext-ioredis`'s.
#[cfg(test)]
mod test_async_shims;

use perry_ffi::{
    alloc_string, build_object_shape, get_handle, get_handle_mut, js_array_alloc, js_array_get,
    js_array_push, js_object_alloc_with_shape, js_object_get_field, js_object_set_field,
    object_field_by_name, register_handle, spawn_blocking, take_handle, ArrayHeader, Handle,
    JsPromise, JsValue, ObjectHeader, Promise, StringHeader,
};
use sqlx::postgres::{PgColumn, PgConnection, PgPool, PgPoolOptions, PgRow};
use sqlx::{Column, Connection, Row, TypeInfo};

/// What node-pg's `ssl` option asked for.
///
/// `pg` accepts `ssl: true`, `ssl: "require"` and an options object; all three
/// mean the same thing to the wire (send an `SSLRequest` and refuse a server
/// that declines), and differ only in the trust material they carry.
#[derive(Debug, Clone, Default)]
pub struct PgSslConfig {
    /// Node's `rejectUnauthorized`, default `true`.
    pub reject_unauthorized: bool,
    /// Explicit trust roots, PEM. Replaces the default set, as in Node.
    pub ca: Vec<u8>,
    /// Override the name verified and sent as SNI. Node calls it `servername`.
    pub servername: Option<String>,
}

/// Connection config — same field shape as perry-stdlib's PgConfig.
#[derive(Debug, Clone)]
pub struct PgConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
    /// `None` is plaintext. `Some` makes the core send an `SSLRequest` and
    /// refuse a server that answers `N` — `pg`'s own reading of `ssl: true`,
    /// and the only safe one: a client that asked for TLS and silently got
    /// none would send its password in the clear.
    pub ssl: Option<PgSslConfig>,
}

impl Default for PgConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 5432,
            user: "postgres".to_string(),
            password: String::new(),
            database: None,
            ssl: None,
        }
    }
}

impl PgConfig {
    /// The URL the **legacy** sqlx transport connects with.
    ///
    /// `sslmode` is carried even though this crate's sqlx is built without a
    /// TLS backend, and precisely because of that: `Require`/`VerifyFull` make
    /// sqlx answer "TLS upgrade required by connect options but SQLx was built
    /// without TLS support enabled" and REFUSE. Leaving it off would make a
    /// client that asked for `ssl` and then declined this transport — a
    /// Unix-socket host, or a thread with no loop of its own — connect in
    /// PLAINTEXT and send its password in the clear. A silent downgrade is the
    /// one outcome worse than a refused connection, and it only became
    /// reachable when `ssl` became a field this binding parses at all.
    pub fn to_url(&self) -> String {
        let db = self
            .database
            .as_ref()
            .map(|d| format!("/{}", d))
            .unwrap_or_default();
        let sslmode = match self.ssl.as_ref() {
            None => "",
            // `verify-full` rather than `require` when the caller wanted the
            // certificate checked, so the spelling stays truthful if a TLS
            // backend is ever compiled in.
            Some(ssl) if ssl.reject_unauthorized => "?sslmode=verify-full",
            Some(_) => "?sslmode=require",
        };
        format!(
            "postgres://{}:{}@{}:{}{}{}",
            self.user, self.password, self.host, self.port, db, sslmode
        )
    }
}

/// The keys of pg's result object, in the order both transports write them.
///
/// A constant rather than two literal lists because `turnloop_io::result`
/// builds the same object from owned data: a key added to one builder and not
/// the other would be a shape divergence that only shows up at runtime, on
/// whichever transport the program happened to take.
pub(crate) const RESULT_KEYS: [&str; 4] = ["rows", "fields", "rowCount", "command"];

/// The keys of one `result.fields[i]`, ditto.
pub(crate) const FIELD_KEYS: [&str; 7] = [
    "name",
    "tableID",
    "columnID",
    "dataTypeID",
    "dataTypeSize",
    "dataTypeModifier",
    "format",
];

/// pg's `result.command`: the first whitespace-delimited word of the statement,
/// uppercased.
///
/// Derived from the statement text, not from the server's CommandComplete tag.
/// The two differ — `WITH … INSERT` tags as `INSERT` but starts with `WITH` —
/// and the text is where this binding has always taken it.
fn command_of(sql: &str) -> String {
    sql.split_whitespace()
        .next()
        .unwrap_or("SELECT")
        .to_uppercase()
}

unsafe fn jsvalue_to_string(value: JsValue) -> Option<String> {
    if value.is_string() {
        let ptr = value.as_string_ptr();
        if !ptr.is_null() {
            let len = (*ptr).byte_len as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            return std::str::from_utf8(bytes).ok().map(String::from);
        }
    }
    None
}

/// Object layout matches perry-stdlib's positional convention:
///   field 0: host (string)
///   field 1: port (number)
///   field 2: user (string)
///   field 3: password (string)
///   field 4: database (string, optional)
unsafe fn parse_pg_config(config: JsValue) -> PgConfig {
    let mut result = PgConfig::default();
    let obj_ptr = config.as_pointer::<ObjectHeader>();
    if obj_ptr.is_null() {
        return result;
    }

    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 0)) {
        result.host = s;
    }
    let port_val = js_object_get_field(obj_ptr, 1);
    if port_val.is_number() {
        result.port = port_val.to_number() as u16;
    }
    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 2)) {
        result.user = s;
    }
    if let Some(s) = jsvalue_to_string(js_object_get_field(obj_ptr, 3)) {
        result.password = s;
    }
    let db_val = js_object_get_field(obj_ptr, 4);
    if !db_val.is_undefined() && !db_val.is_null() {
        if let Some(s) = jsvalue_to_string(db_val) {
            result.database = Some(s);
        }
    }
    // `ssl` is read BY NAME rather than by position. The five fields above are
    // positional because perry-stdlib's own `PgConfig` fixes their order, but
    // `ssl` is optional and a user object literal that omits `database` would
    // put it at a different index. `object_field_by_name` goes through the
    // runtime's property lookup, which is what a user's `{ host, ssl }` needs.
    result.ssl = parse_pg_ssl(object_field_by_name(config, "ssl"));
    result
}

/// node-pg's `ssl`: `false`/absent, `true`, `"require"`, or an options object.
///
/// Anything truthy that is not an object means "TLS with the defaults", which
/// is what `ssl: true` means in `pg`. An object contributes `rejectUnauthorized`
/// (default `true`, as Node), `ca`, and `servername`.
unsafe fn parse_pg_ssl(value: JsValue) -> Option<PgSslConfig> {
    if value.is_undefined() || value.is_null() {
        return None;
    }
    if let Some(text) = jsvalue_to_string(value) {
        // `ssl: "disable"` is libpq's spelling for off; every other string
        // (`"require"`, `"prefer"`, `"verify-full"`) asks for TLS.
        if text.eq_ignore_ascii_case("disable") || text.eq_ignore_ascii_case("false") {
            return None;
        }
        return Some(PgSslConfig {
            reject_unauthorized: true,
            ..PgSslConfig::default()
        });
    }
    let mut ssl = PgSslConfig {
        reject_unauthorized: true,
        ..PgSslConfig::default()
    };
    let obj = value.as_pointer::<ObjectHeader>();
    if obj.is_null() {
        // `ssl: true` — a boolean, no fields to read.
        return value.to_bool().then_some(ssl);
    }
    let reject = object_field_by_name(value, "rejectUnauthorized");
    if !reject.is_undefined() && !reject.is_null() {
        ssl.reject_unauthorized = reject.to_bool();
    }
    if let Some(ca) = jsvalue_to_bytes(object_field_by_name(value, "ca")) {
        ssl.ca = ca;
    }
    if let Some(name) = jsvalue_to_string(object_field_by_name(value, "servername")) {
        ssl.servername = Some(name);
    }
    Some(ssl)
}

/// A `ca` may be a string or a Buffer — `fs.readFileSync` returns the latter.
///
/// The Buffer read goes through the **canonical runtime registry** rather than
/// perry-ffi's local one: this crate is a separately linked archive and cannot
/// see a Buffer the program runtime allocated. `perry-ext-net` learned the same
/// thing about `ca`/`cert`/`key` and its comment is the precedent.
unsafe fn jsvalue_to_bytes(value: JsValue) -> Option<Vec<u8>> {
    if let Some(text) = jsvalue_to_string(value) {
        return Some(text.into_bytes());
    }
    extern "C" {
        fn js_value_buffer_or_typedarray_data(value: f64, out_len: *mut u32) -> *const u8;
    }
    let mut len = 0u32;
    let data = js_value_buffer_or_typedarray_data(f64::from_bits(value.bits()), &mut len);
    if data.is_null() || len == 0 {
        None
    } else {
        Some(std::slice::from_raw_parts(data, len as usize).to_vec())
    }
}

/// Convert a single column value to a JsValue, mapping common
/// PostgreSQL OIDs to JS scalars. Unknown types fall back to a
/// string read.
fn column_value_to_jsvalue(row: &PgRow, index: usize) -> JsValue {
    let col = &row.columns()[index];
    let type_name = col.type_info().name();
    match type_name {
        "INT4" | "INT2" => row
            .try_get::<i32, _>(index)
            .map(JsValue::from_int32)
            .unwrap_or(JsValue::NULL),
        "INT8" => row
            .try_get::<i64, _>(index)
            .map(|n| JsValue::from_number(n as f64))
            .unwrap_or(JsValue::NULL),
        "FLOAT4" | "FLOAT8" | "NUMERIC" => row
            .try_get::<f64, _>(index)
            .map(JsValue::from_number)
            .unwrap_or(JsValue::NULL),
        "VARCHAR" | "CHAR" | "TEXT" | "BPCHAR" | "NAME" => row
            .try_get::<String, _>(index)
            .map(|s| JsValue::from_string_ptr(alloc_string(&s).as_raw()))
            .unwrap_or(JsValue::NULL),
        "BOOL" => row
            .try_get::<bool, _>(index)
            .map(JsValue::from_bool)
            .unwrap_or(JsValue::NULL),
        _ => row
            .try_get::<String, _>(index)
            .map(|s| JsValue::from_string_ptr(alloc_string(&s).as_raw()))
            .unwrap_or(JsValue::NULL),
    }
}

/// Build a row object keyed by column names. Replaces perry-stdlib's
/// `js_object_alloc(0, n)` no-shape pattern with a perry-ffi
/// shape-aware allocation — same observable behavior since user code
/// accesses `row.id` through dynamic property lookup either way.
fn row_to_js_object(row: &PgRow) -> *mut ObjectHeader {
    let cols: Vec<&str> = row.columns().iter().map(|c| c.name()).collect();
    let (packed, shape_id) = build_object_shape(&cols);
    let obj = unsafe {
        js_object_alloc_with_shape(
            shape_id,
            cols.len() as u32,
            packed.as_ptr(),
            packed.len() as u32,
        )
    };
    for i in 0..cols.len() {
        let val = column_value_to_jsvalue(row, i);
        unsafe { js_object_set_field(obj, i as u32, val) };
    }
    obj
}

/// Build a `FieldDef`-shaped object matching node-pg's `result.fields[i]`
/// (#4917): `dataTypeID` is the numeric type OID, `tableID`/`columnID` come
/// from the RowDescription (0 for expression columns, like Node).
/// `dataTypeSize`/`dataTypeModifier` are not exposed by sqlx 0.8 and report
/// the "unknown/variable" sentinel -1. Twin of
/// `perry_stdlib::pg::types::column_to_field_def` — keep in sync.
fn column_to_field_def(col: &PgColumn) -> *mut ObjectHeader {
    let (packed, shape_id) = build_object_shape(&FIELD_KEYS);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 7, packed.as_ptr(), packed.len() as u32) };
    let name_str = alloc_string(col.name());
    let table_id = col.relation_id().map(|oid| oid.0 as f64).unwrap_or(0.0);
    let column_id = col
        .relation_attribute_no()
        .map(|attno| attno as f64)
        .unwrap_or(0.0);
    let data_type_id = col.type_info().oid().map(|oid| oid.0 as f64).unwrap_or(0.0);
    let format_str = alloc_string("text");
    unsafe {
        js_object_set_field(obj, 0, JsValue::from_string_ptr(name_str.as_raw()));
        js_object_set_field(obj, 1, JsValue::from_number(table_id));
        js_object_set_field(obj, 2, JsValue::from_number(column_id));
        js_object_set_field(obj, 3, JsValue::from_number(data_type_id));
        js_object_set_field(obj, 4, JsValue::from_number(-1.0));
        js_object_set_field(obj, 5, JsValue::from_number(-1.0));
        js_object_set_field(obj, 6, JsValue::from_string_ptr(format_str.as_raw()));
    }
    obj
}

/// Wrap a query outcome in pg's `{ rows, fields, rowCount, command }`
/// result object.
fn rows_to_pg_result(rows: Vec<PgRow>, columns: &[PgColumn], command: &str) -> JsValue {
    let (packed, shape_id) = build_object_shape(&RESULT_KEYS);
    let result_obj =
        unsafe { js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32) };

    // rows array
    let mut rows_arr = unsafe { js_array_alloc(rows.len() as u32) };
    for row in &rows {
        let row_obj = row_to_js_object(row);
        rows_arr = unsafe { js_array_push(rows_arr, JsValue::from_object_ptr(row_obj)) };
    }
    unsafe { js_object_set_field(result_obj, 0, JsValue::from_object_ptr(rows_arr)) };

    // fields array
    let mut fields_arr = unsafe { js_array_alloc(columns.len() as u32) };
    for col in columns {
        let field_obj = column_to_field_def(col);
        fields_arr = unsafe { js_array_push(fields_arr, JsValue::from_object_ptr(field_obj)) };
    }
    unsafe { js_object_set_field(result_obj, 1, JsValue::from_object_ptr(fields_arr)) };

    unsafe {
        js_object_set_field(result_obj, 2, JsValue::from_number(rows.len() as f64));
        let cmd_str = alloc_string(command);
        js_object_set_field(result_obj, 3, JsValue::from_string_ptr(cmd_str.as_raw()));
    }
    JsValue::from_object_ptr(result_obj)
}

fn empty_pg_result(command: &str, row_count: u64) -> JsValue {
    let value = rows_to_pg_result(Vec::new(), &[], command);
    let obj: *mut ObjectHeader = value.as_pointer();
    if !obj.is_null() {
        unsafe {
            js_object_set_field(obj, 2, JsValue::from_number(row_count as f64));
        }
    }
    value
}

#[derive(Clone, Debug)]
enum ParamValue {
    Null,
    String(String),
    Number(f64),
    Int(i64),
    Bool(bool),
}

unsafe fn extract_params_from_jsvalue(params: JsValue) -> Vec<ParamValue> {
    let arr_ptr = params.as_pointer::<ArrayHeader>();
    if arr_ptr.is_null() {
        return Vec::new();
    }
    // Pull the array length out of the header — the layout matches
    // perry-runtime's `ArrayHeader { length: u32, capacity: u32 }`.
    let length = (*arr_ptr).length;

    let mut result = Vec::with_capacity(length as usize);
    for i in 0..length {
        let element = js_array_get(arr_ptr, i);
        let param = if element.is_null() || element.is_undefined() {
            ParamValue::Null
        } else if element.is_string() {
            jsvalue_to_string(element)
                .map(ParamValue::String)
                .unwrap_or(ParamValue::Null)
        } else if element.is_int32() {
            ParamValue::Int(element.to_int32() as i64)
        } else if element.is_bool() {
            ParamValue::Bool(element.to_bool())
        } else if element.is_number() {
            let n = element.to_number();
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                ParamValue::Int(n as i64)
            } else {
                ParamValue::Number(n)
            }
        } else {
            ParamValue::Null
        };
        result.push(param);
    }
    result
}

fn is_row_returning_query(sql: &str) -> bool {
    let trimmed = sql.trim_start();
    let upper = trimmed.get(..10).unwrap_or(trimmed).to_uppercase();
    upper.starts_with("SELECT")
        || upper.starts_with("SHOW")
        || upper.starts_with("DESC")
        || upper.starts_with("EXPLAIN")
        || upper.starts_with("WITH")
}

unsafe fn read_sql(sql_ptr: *const u8) -> String {
    if sql_ptr.is_null() {
        return String::new();
    }
    let header = sql_ptr as *const StringHeader;
    let len = (*header).byte_len as usize;
    let data = sql_ptr.add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or("").to_string()
}

// ── Connection (Client) ───────────────────────────────────────────

/// Wraps a `PgConnection` so it can sit in the handle registry.
/// Pre-connect: `pending_config = Some, connection = None`.
/// Connected:   `pending_config = None, connection = Some`.
///
/// On the turnloop transport neither field is ever set: the connection is loop
/// state, keyed by this handle in `turnloop_io`'s thread-local table (the core
/// owns `JsPromise`s and so is neither `Send` nor `Sync`, which the handle
/// registry requires). `turnloop` is what routes the entry points.
pub struct PgConnectionHandle {
    pub connection: Option<PgConnection>,
    pub pending_config: Option<PgConfig>,
    /// `Some` exactly when this client lives on turnloop, carrying the config
    /// its connection will be built from.
    ///
    /// The transport is decided **once, at `new Client()`**, and never changes —
    /// P1's rule for sockets, for the same reason: a client that switched
    /// mid-life would have two different sessions on the same server, and
    /// `client.query('BEGIN')` would silently stop meaning anything.
    ///
    /// It carries the config rather than a bare flag because `pending_config`
    /// is *taken* by `connect()`, while the turnloop core is built when the
    /// socket opens — later than that.
    pub(crate) turnloop: Option<PgConfig>,
    /// `connect()` has been called on this client at least once.
    ///
    /// Mirrors the sqlx path's `pending_config.take()`: a second `connect()`
    /// there finds `None` and resolves `undefined` without touching the
    /// network, and so does this one.
    pub(crate) connect_started: bool,
}

impl PgConnectionHandle {
    pub fn new(conn: PgConnection) -> Self {
        Self {
            connection: Some(conn),
            pending_config: None,
            turnloop: None,
            connect_started: false,
        }
    }
    pub fn pending(config: PgConfig) -> Self {
        Self {
            connection: None,
            pending_config: Some(config),
            turnloop: None,
            connect_started: false,
        }
    }
    /// A client on the turnloop transport. `connect_started` is true for the
    /// combined `pg.connect(config)` entry, whose caller already has a
    /// connection in hand.
    pub(crate) fn turnloop(config: PgConfig, connect_started: bool) -> Self {
        Self {
            connection: None,
            pending_config: None,
            turnloop: Some(config),
            connect_started,
        }
    }
}

/// Take a client handle back out of the registry. `true` if it was there.
pub(crate) fn forget_client(handle: Handle) -> bool {
    take_handle::<PgConnectionHandle>(handle).is_some()
}

/// Take a pool handle back out of the registry. `true` if it was there.
pub(crate) fn forget_pool(handle: Handle) -> bool {
    take_handle::<PgPoolHandle>(handle).is_some()
}

/// The config of a turnloop client, or `None` if this handle is not one (or is
/// not a client at all).
fn client_turnloop_config(handle: Handle) -> Option<PgConfig> {
    get_handle::<PgConnectionHandle>(handle).and_then(|h| h.turnloop.clone())
}

/// The config of a turnloop pool, or `None`.
fn pool_turnloop_config(handle: Handle) -> Option<PgConfig> {
    get_handle::<PgPoolHandle>(handle).and_then(|h| h.turnloop.clone())
}

/// What `client.connect()` should do with this handle.
enum ConnectRoute {
    /// Not a turnloop client (or not a live handle) — fall through to sqlx.
    Legacy,
    /// First `connect()` on a turnloop client; open the socket.
    Turnloop(PgConfig),
    /// `connect()` has already run once. The sqlx path resolves `undefined`
    /// here because it took `pending_config` the first time round.
    AlreadyStarted,
}

fn client_connect_route(handle: Handle) -> ConnectRoute {
    let Some(record) = get_handle_mut::<PgConnectionHandle>(handle) else {
        return ConnectRoute::Legacy;
    };
    let Some(config) = record.turnloop.clone() else {
        return ConnectRoute::Legacy;
    };
    if record.connect_started {
        return ConnectRoute::AlreadyStarted;
    }
    record.connect_started = true;
    ConnectRoute::Turnloop(config)
}

/// `new Client(config)` — sync constructor, no TCP touch.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue (passed as f64 at the FFI
/// boundary).
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_new(config_f: f64) -> Handle {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    // The transport is decided here and never revisited; see
    // `PgConnectionHandle::turnloop`.
    if turnloop_io::supports(&pg_config) {
        return register_handle(PgConnectionHandle::turnloop(pg_config, false));
    }
    register_handle(PgConnectionHandle::pending(pg_config))
}

/// `client.connect()` — opens the TCP connection using the config
/// stored at `js_pg_client_new` time. No-op success if already
/// connected.
#[no_mangle]
pub extern "C" fn js_pg_client_connect(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    // Routed before the promise moves: `turnloop_io::client_connect` takes it
    // by value, so asking afterwards would have dropped it — and a dropped
    // `JsPromise` is a promise that never settles.
    match client_connect_route(client_handle) {
        ConnectRoute::Turnloop(config) => {
            turnloop_io::client_connect(client_handle, &config, promise);
            return raw;
        }
        ConnectRoute::AlreadyStarted => {
            promise.resolve_undefined();
            return raw;
        }
        ConnectRoute::Legacy => {}
    }

    // Snapshot the pending config before entering spawn_blocking —
    // can't hold a `&mut` across the boundary.
    let pending =
        get_handle_mut::<PgConnectionHandle>(client_handle).and_then(|h| h.pending_config.take());

    let Some(pg_config) = pending else {
        promise.resolve_undefined();
        return raw;
    };

    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgConnection::connect(&pg_config.to_url()).await });
        match result {
            Ok(conn) => {
                if let Some(h) = get_handle_mut::<PgConnectionHandle>(client_handle) {
                    h.connection = Some(conn);
                }
                promise.resolve_undefined();
            }
            Err(e) => promise.reject_string(&format!("Failed to connect: {}", e)),
        }
    });
    raw
}

/// Combined `pg.connect(config)` — sync `new` + async connect; older
/// API kept for back-compat with perry-stdlib callers.
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_connect(config_f: f64) -> *mut Promise {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    if turnloop_io::supports(&pg_config) {
        turnloop_io::connect_new_client(pg_config, promise);
        return raw;
    }

    spawn_blocking(move || {
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgConnection::connect(&pg_config.to_url()).await });
        match result {
            Ok(conn) => {
                let handle = register_handle(PgConnectionHandle::new(conn));
                promise.resolve(JsValue::from_number(handle as f64));
            }
            Err(e) => promise.reject_string(&format!("Failed to connect: {}", e)),
        }
    });
    raw
}

/// `client.end()` — close the connection.
#[no_mangle]
pub extern "C" fn js_pg_client_end(client_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if client_turnloop_config(client_handle).is_some() {
        turnloop_io::client_end(client_handle, promise);
        return raw;
    }
    spawn_blocking(move || {
        if let Some(mut wrapper) = take_handle::<PgConnectionHandle>(client_handle) {
            if let Some(conn) = wrapper.connection.take() {
                let result = tokio::runtime::Handle::current().block_on(conn.close());
                match result {
                    Ok(()) => promise.resolve_undefined(),
                    Err(e) => promise.reject_string(&format!("Failed to close connection: {}", e)),
                }
            } else {
                promise.reject_string("Connection already closed");
            }
        } else {
            promise.reject_string("Invalid client handle");
        }
    });
    raw
}

/// `client.query(sql)` — no params.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query(
    client_handle: Handle,
    sql_ptr: *const u8,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let command = command_of(&sql);

    let promise = JsPromise::new();
    let raw = promise.as_raw();

    if client_turnloop_config(client_handle).is_some() {
        // `fetch_all` regardless of the statement, which is why a
        // non-parameterised `INSERT` reports `rowCount: 0` on both transports.
        turnloop_io::client_query(
            client_handle,
            promise,
            turnloop_io::Statement {
                sql,
                params: Vec::new(),
                kind: turnloop_io::ResultKind::Rows,
                command,
            },
        );
        return raw;
    }

    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgConnectionHandle>(client_handle)
                .ok_or_else(|| "Invalid client handle".to_string())?;
            let conn = wrapper
                .connection
                .as_mut()
                .ok_or_else(|| "Connection already closed".to_string())?;
            sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .fetch_all(conn)
                .await
                .map_err(|e| format!("Query failed: {}", e))
        });
        match outcome {
            Ok(rows) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                let result = rows_to_pg_result(rows, &columns, &command);
                promise.resolve(result);
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `client.query(sql, params)` — parameterized.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_client_query_params(
    client_handle: Handle,
    sql_ptr: *const u8,
    params_f: f64,
) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let params = JsValue::from_bits(params_f.to_bits());
    let param_values = extract_params_from_jsvalue(params);
    let command = command_of(&sql);
    let is_select = is_row_returning_query(&sql);

    let promise = JsPromise::new();
    let raw = promise.as_raw();

    if client_turnloop_config(client_handle).is_some() {
        let kind = turnloop_io::ResultKind::for_sql(&sql);
        turnloop_io::client_query(
            client_handle,
            promise,
            turnloop_io::Statement {
                sql,
                params: param_values,
                kind,
                command,
            },
        );
        return raw;
    }

    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgConnectionHandle>(client_handle)
                .ok_or_else(|| "Invalid client handle".to_string())?;
            let conn = wrapper
                .connection
                .as_mut()
                .ok_or_else(|| "Connection already closed".to_string())?;
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()));
            for p in &param_values {
                query = match p {
                    ParamValue::Null => query.bind(Option::<String>::None),
                    ParamValue::String(s) => query.bind(s.clone()),
                    ParamValue::Number(n) => query.bind(*n),
                    ParamValue::Int(i) => query.bind(*i),
                    ParamValue::Bool(b) => query.bind(*b),
                };
            }
            if is_select {
                let rows = query
                    .fetch_all(conn)
                    .await
                    .map_err(|e| format!("Query failed: {}", e))?;
                Ok::<_, String>(QueryOutcome::Rows(rows))
            } else {
                let exec_result = query
                    .execute(conn)
                    .await
                    .map_err(|e| format!("Query failed: {}", e))?;
                Ok(QueryOutcome::RowsAffected(exec_result.rows_affected()))
            }
        });
        match outcome {
            Ok(QueryOutcome::Rows(rows)) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                promise.resolve(rows_to_pg_result(rows, &columns, &command));
            }
            Ok(QueryOutcome::RowsAffected(n)) => {
                promise.resolve(empty_pg_result(&command, n));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

enum QueryOutcome {
    Rows(Vec<PgRow>),
    RowsAffected(u64),
}

// ── Pool ──────────────────────────────────────────────────────────

pub struct PgPoolHandle {
    pub pool: Option<PgPool>,
    pub pending_url: Option<String>,
    /// `Some` exactly when this pool lives on turnloop; see
    /// `PgConnectionHandle::turnloop`. What "pool" means on that transport is
    /// spelled out in `turnloop_io`'s module docs — it is one pipelined
    /// connection, not ten.
    pub(crate) turnloop: Option<PgConfig>,
}

impl PgPoolHandle {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: Some(pool),
            pending_url: None,
            turnloop: None,
        }
    }
    pub fn pending(url: String) -> Self {
        Self {
            pool: None,
            pending_url: Some(url),
            turnloop: None,
        }
    }
    pub(crate) fn turnloop(config: PgConfig) -> Self {
        Self {
            pool: None,
            pending_url: None,
            turnloop: Some(config),
        }
    }

    pub async fn ensure_pool(&mut self) -> Result<&PgPool, String> {
        if self.pool.is_none() {
            let url = self
                .pending_url
                .take()
                .ok_or_else(|| "Pool config missing".to_string())?;
            let pool = PgPoolOptions::new()
                .max_connections(10)
                .connect(&url)
                .await
                .map_err(|e| format!("Failed to create pool: {}", e))?;
            self.pool = Some(pool);
        }
        Ok(self.pool.as_ref().unwrap())
    }
}

/// `new Pool(config)` — sync constructor; sqlx's pool is built lazily
/// on first query (sqlx 0.8's `connect_lazy` panics outside a Tokio
/// runtime, so we can't even pre-arm it here).
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_new(config_f: f64) -> Handle {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    if turnloop_io::supports(&pg_config) {
        return register_handle(PgPoolHandle::turnloop(pg_config));
    }
    register_handle(PgPoolHandle::pending(pg_config.to_url()))
}

/// `pg.createPool(config)` — async eager pool factory (back-compat
/// with perry-stdlib's older entry).
///
/// # Safety
/// `config_f` is a NaN-boxed JsValue.
#[no_mangle]
pub unsafe extern "C" fn js_pg_create_pool(config_f: f64) -> *mut Promise {
    let config = JsValue::from_bits(config_f.to_bits());
    let pg_config = parse_pg_config(config);
    let promise = JsPromise::new();
    let raw = promise.as_raw();

    if turnloop_io::supports(&pg_config) {
        turnloop_io::create_pool(pg_config, promise);
        return raw;
    }

    spawn_blocking(move || {
        let url = pg_config.to_url();
        let result = tokio::runtime::Handle::current()
            .block_on(async move { PgPoolOptions::new().max_connections(10).connect(&url).await });
        match result {
            Ok(pool) => {
                let handle = register_handle(PgPoolHandle::new(pool));
                promise.resolve(JsValue::from_number(handle as f64));
            }
            Err(e) => promise.reject_string(&format!("Failed to create pool: {}", e)),
        }
    });
    raw
}

/// `pool.query(sql)` — runs against the lazy-built sqlx pool.
///
/// # Safety
/// `sql_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_pg_pool_query(pool_handle: Handle, sql_ptr: *const u8) -> *mut Promise {
    let sql = read_sql(sql_ptr);
    let command = command_of(&sql);

    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if let Some(config) = pool_turnloop_config(pool_handle) {
        // `fetch_all` on the pool too, so `rowCount` is the collected row count
        // exactly as it is today.
        turnloop_io::pool_query(
            pool_handle,
            &config,
            promise,
            turnloop_io::Statement {
                sql,
                params: Vec::new(),
                kind: turnloop_io::ResultKind::Rows,
                command,
            },
        );
        return raw;
    }
    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let wrapper = get_handle_mut::<PgPoolHandle>(pool_handle)
                .ok_or_else(|| "Invalid pool handle".to_string())?;
            let pool = wrapper.ensure_pool().await?;
            sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .fetch_all(pool)
                .await
                .map_err(|e| format!("Query failed: {}", e))
        });
        match outcome {
            Ok(rows) => {
                let columns: Vec<_> = if !rows.is_empty() {
                    rows[0].columns().to_vec()
                } else {
                    Vec::new()
                };
                promise.resolve(rows_to_pg_result(rows, &columns, &command));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `pool.end()` — close all connections in the pool.
#[no_mangle]
pub extern "C" fn js_pg_pool_end(pool_handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    if pool_turnloop_config(pool_handle).is_some() {
        turnloop_io::pool_end(pool_handle, promise);
        return raw;
    }
    spawn_blocking(move || {
        if let Some(mut wrapper) = take_handle::<PgPoolHandle>(pool_handle) {
            tokio::runtime::Handle::current().block_on(async move {
                if let Some(pool) = wrapper.pool.take() {
                    pool.close().await;
                }
            });
            promise.resolve_undefined();
        } else {
            promise.reject_string("Invalid pool handle");
        }
    });
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_plaintext_config_asks_the_legacy_transport_for_no_tls() {
        let url = PgConfig {
            host: "db".into(),
            port: 5432,
            user: "u".into(),
            password: "p".into(),
            database: Some("d".into()),
            ssl: None,
        }
        .to_url();
        assert_eq!(url, "postgres://u:p@db:5432/d");
    }

    #[test]
    fn an_ssl_config_makes_the_legacy_transport_refuse_rather_than_downgrade() {
        // sqlx here is built with no TLS backend, so `sslmode=verify-full`
        // makes it answer "TLS upgrade required by connect options but SQLx was
        // built without TLS support enabled". That refusal is the point: the
        // alternative is a client that asked for `ssl`, declined this
        // transport, and sent its password in plaintext.
        let config = PgConfig {
            host: "db".into(),
            port: 5432,
            user: "u".into(),
            password: "p".into(),
            database: Some("d".into()),
            ssl: Some(PgSslConfig {
                reject_unauthorized: true,
                ca: Vec::new(),
                servername: None,
            }),
        };
        assert_eq!(
            config.to_url(),
            "postgres://u:p@db:5432/d?sslmode=verify-full"
        );

        // `rejectUnauthorized: false` still requires TLS — it only relaxes what
        // is checked about the certificate, never whether there is one.
        let unverified = PgConfig {
            ssl: Some(PgSslConfig {
                reject_unauthorized: false,
                ca: Vec::new(),
                servername: None,
            }),
            ..config
        };
        assert_eq!(
            unverified.to_url(),
            "postgres://u:p@db:5432/d?sslmode=require"
        );
    }

    #[test]
    fn pg_config_defaults() {
        let cfg = PgConfig::default();
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.user, "postgres");
        assert!(cfg.database.is_none());
    }

    #[test]
    fn to_url_omits_db_when_absent() {
        let cfg = PgConfig::default();
        let url = cfg.to_url();
        assert_eq!(url, "postgres://postgres:@localhost:5432");
    }

    #[test]
    fn to_url_with_db() {
        let mut cfg = PgConfig::default();
        cfg.database = Some("mydb".to_string());
        cfg.user = "u".to_string();
        cfg.password = "p".to_string();
        cfg.host = "db.example.com".to_string();
        cfg.port = 5433;
        assert_eq!(cfg.to_url(), "postgres://u:p@db.example.com:5433/mydb");
    }

    #[test]
    fn is_row_returning_query_classifier() {
        assert!(is_row_returning_query("SELECT * FROM x"));
        assert!(is_row_returning_query("  select 1"));
        assert!(is_row_returning_query("WITH cte AS ..."));
        assert!(!is_row_returning_query("INSERT INTO x VALUES (1)"));
        assert!(!is_row_returning_query("UPDATE x SET y = 1"));
    }

    #[test]
    fn client_new_returns_handle() {
        let cfg_obj = unsafe {
            let (packed, shape_id) =
                build_object_shape(&["host", "port", "user", "password", "database"]);
            let obj = js_object_alloc_with_shape(shape_id, 5, packed.as_ptr(), packed.len() as u32);
            let host_str = alloc_string("localhost");
            js_object_set_field(obj, 0, JsValue::from_string_ptr(host_str.as_raw()));
            js_object_set_field(obj, 1, JsValue::from_number(5432.0));
            JsValue::from_object_ptr(obj)
        };
        let h = unsafe { js_pg_client_new(f64::from_bits(cfg_obj.bits())) };
        assert!(h > 0);
    }
}
