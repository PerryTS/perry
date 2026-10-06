//! `node:sqlite` `DatabaseSync` and its `db.limits` object: ordinary objects
//! that own a native payload (#11919, `docs/native-payload-pattern.md`).
//!
//! `DatabaseSync` is a class-S family: SQLite calls the authorizer, scalar
//! functions and aggregates back synchronously. Each registration stores its
//! JS function in the payload's `callbacks` array (`set_callback`) and hands
//! SQLite a [`CallbackSite`](perry_runtime::native_payload::CallbackSite) as
//! userdata; the trampolines in `sqlite_callbacks.rs` find the owner through
//! the cell's traced owner edge. Every C call that can call back runs between
//! `native_payload::enter` and `finish`, with no payload borrow held, and a
//! JS throw inside SQLite is parked and rethrown after the C call returns.
//!
//! The payload exists only while the database is open: `close()` releases it
//! (deferred while a callback is running) and `open()` attaches a new one to
//! the same object. The configuration a reopen needs lives in the JS state.

use super::*;
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::gc::{RuntimeHandle, RuntimeHandleScope};
use perry_runtime::native_class_ids::{SQLITE_DATABASE_SYNC, SQLITE_LIMITS};
use perry_runtime::native_payload::{
    self, AttachMiss, CallEnd, CallbackSites, Lifecycle, NativePayloadFamily, OpenSerial,
    PayloadMiss, PayloadPrototype,
};
use perry_runtime::{
    buffer::BufferHeader, js_array_alloc, js_array_get, js_array_length, js_array_push_f64,
    js_nanbox_pointer, js_promise_rejected, js_promise_resolved, js_string_from_bytes, ArrayHeader,
    JSValue, Promise,
};
use rusqlite::ffi;
use std::cell::Cell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::rc::Rc;

perry_runtime::state_key_memo!(static MEMO_DB);
perry_runtime::state_key_memo!(static MEMO_LIMITS);

macro_rules! builtin {
    ($body:path, $n:tt) => {
        perry_runtime::fn_info!($body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN))
    };
}

pub(crate) static DB_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_DATABASE_SYNC,
    links_owner: true,
    name: "DatabaseSync",
    constructor_export: Some(("sqlite", "DatabaseSync")),
    constructor_length: 1,
    install_prototype: install_db_prototype,
};

pub(crate) static LIMITS_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_LIMITS,
    links_owner: false,
    name: "",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: install_limits_prototype,
};

/// What `open()` needs to (re)open the connection. Plain data; persisted in
/// the JS state while the database is closed.
#[derive(Clone)]
pub(crate) struct DbConfig {
    pub(crate) path: String,
    pub(crate) opts: NodeSqliteOptions,
    pub(crate) enable_load_extension: bool,
}

impl DbConfig {
    pub(crate) fn stmt_flags(&self) -> StmtFlags {
        StmtFlags {
            read_bigints: self.opts.read_bigints,
            return_arrays: self.opts.return_arrays,
            allow_bare_named_parameters: self.opts.allow_bare_named_parameters,
            allow_unknown_named_parameters: self.opts.allow_unknown_named_parameters,
        }
    }

    fn bits(&self) -> u32 {
        let o = &self.opts;
        [
            o.open,
            o.read_only,
            o.read_write,
            o.create,
            o.enable_foreign_keys,
            o.enable_dqs,
            o.read_bigints,
            o.return_arrays,
            o.allow_bare_named_parameters,
            o.allow_unknown_named_parameters,
            o.allow_extension,
            o.defensive,
            self.enable_load_extension,
        ]
        .iter()
        .enumerate()
        .fold(0, |bits, (i, on)| bits | (u32::from(*on) << i))
    }

    fn from_bits(path: String, bits: u32, timeout_ms: i32, limits: [Option<i32>; 11]) -> Self {
        let on = |i: u32| bits & (1 << i) != 0;
        DbConfig {
            path,
            opts: NodeSqliteOptions {
                open: on(0),
                read_only: on(1),
                read_write: on(2),
                create: on(3),
                enable_foreign_keys: on(4),
                enable_dqs: on(5),
                timeout_ms,
                read_bigints: on(6),
                return_arrays: on(7),
                allow_bare_named_parameters: on(8),
                allow_unknown_named_parameters: on(9),
                allow_extension: on(10),
                defensive: on(11),
                initial_limits: limits,
            },
            enable_load_extension: on(12),
        }
    }
}

/// A child C resource the connection must delete before it closes.
pub(crate) struct SessionCell {
    raw: std::cell::Cell<*mut ffi::sqlite3_session>,
}

impl SessionCell {
    pub(crate) fn new(raw: *mut ffi::sqlite3_session) -> Rc<Self> {
        Rc::new(SessionCell {
            raw: std::cell::Cell::new(raw),
        })
    }

    pub(crate) fn raw(&self) -> *mut ffi::sqlite3_session {
        self.raw.get()
    }

    /// Delete the session now (idempotent). Plain C; no callbacks.
    pub(crate) fn delete(&self) {
        let raw = self.raw.replace(std::ptr::null_mut());
        if !raw.is_null() {
            unsafe { ffi::sqlite3session_delete(raw) };
        }
    }
}

/// The open connection. The C resource is released in `Drop` before the
/// callback sites are freed (fields drop after the body).
pub(crate) struct NodeDb {
    pub(crate) raw: *mut ffi::sqlite3,
    pub(crate) sessions: Vec<Rc<SessionCell>>,
    sites: CallbackSites,
    auth_site: *mut c_void,
    next_callback: u32,
    pub(crate) agg_free: Vec<u32>,
    pub(crate) agg_len: u32,
    pub(crate) serial: OpenSerial,
    pub(crate) cfg: DbConfig,
    /// Shared with this open's statements: true until the connection
    /// closes. A statement's compiled `sqlite3_stmt` is finalized by its own
    /// drop while this holds, and by the connection's close otherwise.
    pub(crate) live: Rc<Cell<bool>>,
}

extern "C" {
    // In the bundled SQLite; libsqlite3-sys does not bind it.
    fn sqlite3_close_v2(db: *mut ffi::sqlite3) -> c_int;
}

impl Drop for NodeDb {
    fn drop(&mut self) {
        // From here the connection finalizes every statement; a statement
        // dropped later must not touch its (finalized) `sqlite3_stmt`.
        self.live.set(false);
        unsafe {
            for session in self.sessions.drain(..) {
                session.delete();
            }
            // A statement still on the connection belongs to an outer call
            // whose callback closed the database (deferred close); finalize
            // it here, while the sites it may call are still alive. Its own
            // frame sees the database closed and leaves it alone.
            loop {
                let stmt = ffi::sqlite3_next_stmt(self.raw, std::ptr::null_mut());
                if stmt.is_null() {
                    break;
                }
                ffi::sqlite3_finalize(stmt);
            }
            sqlite3_close_v2(self.raw);
        }
        #[cfg(test)]
        test_counters::DB_DROPS.with(|c| c.set(c.get() + 1));
    }
}

#[cfg(test)]
pub(crate) mod test_counters {
    thread_local! {
        pub(crate) static DB_DROPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
}

/// Native memory a connection keeps (page cache, schema, VDBE scratch); a
/// conservative estimate for GC pacing.
const DB_EXTERNAL_BYTES: usize = 64 * 1024;

pub(crate) fn throw_illegal_invocation() -> ! {
    throw_plain_type("Illegal invocation")
}

/// `this`'s open connection, throwing node's errors otherwise. Do not hold
/// the reference across a C call that can call back, an allocation or JS.
pub(crate) unsafe fn db_payload<'a>(this: f64) -> &'a mut NodeDb {
    match native_payload::payload_mut::<NodeDb>(this, &DB_FAMILY) {
        Ok(db) => db,
        Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    }
}

pub(crate) fn db_is_open(db: f64) -> bool {
    matches!(
        native_payload::lifecycle(db, &DB_FAMILY),
        Ok(Lifecycle::Open)
    )
}

/// Run one C call that can call back into JS, between `enter` and
/// `finish`. `f` must be plain C: it may not throw, convert results or call
/// JS other than through SQLite's callbacks.
pub(crate) unsafe fn guarded<R>(db: f64, f: impl FnOnce() -> R) -> (R, Result<(), CallEnd>) {
    let guard = match native_payload::enter(db, &DB_FAMILY) {
        Ok(guard) => guard,
        Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    };
    guard.call(f)
}

/// Throw what a guarded call ended with, outside every SQLite frame.
pub(crate) unsafe fn throw_call_end(end: CallEnd) -> ! {
    match end {
        CallEnd::Threw(value) => perry_runtime::exception::js_throw(value),
        CallEnd::Closed => throw_invalid_state("database is not open"),
    }
}

/// An `Error` value with a node error code, built without throwing.
pub(crate) fn error_value_with_code(message: &str, code: &'static str) -> f64 {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    perry_runtime::node_submodules::register_error_code_pub(msg, code);
    let err = perry_runtime::error::js_error_new_with_message(msg);
    js_nanbox_pointer(err as i64)
}

/// A SQLite error captured inside a guard (plain data).
pub(crate) struct CapturedError {
    pub(crate) message: String,
    pub(crate) code: i32,
}

pub(crate) unsafe fn capture_error(db: *mut ffi::sqlite3) -> CapturedError {
    CapturedError {
        message: sqlite_errmsg_raw(db),
        code: ffi::sqlite3_extended_errcode(db),
    }
}

pub(crate) unsafe fn throw_captured(error: CapturedError) -> ! {
    throw_sqlite_error_ext(&error.message, error.code)
}

// ---- configuration in the JS state ----------------------------------------

/// The configuration lives in one JS-state field, `cfg`:
/// `[path, flag bits, timeout, limits array | undefined]`.
unsafe fn store_config(db: f64, cfg: &DbConfig) {
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(db);
    let record = scope.root_raw_mut_ptr(js_array_alloc(4));
    let push = |value: f64| {
        let next = js_array_push_f64(record.get_raw_mut_ptr(), value);
        record.set_raw_mut_ptr(next);
    };
    push(f64_from_jsvalue(string_value(&cfg.path)));
    push(f64_from_jsvalue(JSValue::number(cfg.bits() as f64)));
    push(f64_from_jsvalue(JSValue::number(
        cfg.opts.timeout_ms as f64,
    )));
    if cfg.opts.initial_limits.iter().any(Option::is_some) {
        let limits = scope.root_raw_mut_ptr(js_array_alloc(NODE_SQLITE_LIMIT_COUNT as u32));
        for limit in cfg.opts.initial_limits {
            let value = f64_from_jsvalue(JSValue::number(limit.map_or(-1.0, f64::from)));
            let next = js_array_push_f64(limits.get_raw_mut_ptr(), value);
            limits.set_raw_mut_ptr(next);
        }
        push(js_nanbox_pointer(
            limits.get_raw_mut_ptr::<ArrayHeader>() as i64
        ));
    } else {
        push(undefined_f64());
    }
    let record = js_nanbox_pointer(record.get_raw_mut_ptr::<ArrayHeader>() as i64);
    native_payload::state_set(db.get_nanbox_f64(), &DB_FAMILY, b"cfg", record);
}

/// Re-state the flag bits after `enableDefensive` / `enableLoadExtension`.
unsafe fn store_config_bits(db: f64, cfg: &DbConfig) {
    let record = native_payload::state_get(db, &DB_FAMILY, b"cfg");
    if value_from_f64(record).is_pointer() {
        perry_runtime::array::js_array_set_f64(
            raw_addr_from_value(record) as *mut ArrayHeader,
            1,
            f64_from_jsvalue(JSValue::number(cfg.bits() as f64)),
        );
    }
}

fn number_of(value: f64) -> f64 {
    let js = value_from_f64(value);
    if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() {
        js.as_number()
    } else {
        0.0
    }
}

unsafe fn load_config(db: f64) -> DbConfig {
    let record = native_payload::state_get(db, &DB_FAMILY, b"cfg");
    let arr = raw_addr_from_value(record) as *const ArrayHeader;
    let field = |i: u32| {
        if arr.is_null() || i >= js_array_length(arr) {
            undefined_f64()
        } else {
            f64_from_jsvalue(js_array_get(arr, i))
        }
    };
    let path = string_key_from_js_value(value_from_f64(field(0))).unwrap_or_default();
    let bits = number_of(field(1)) as u32;
    let timeout = number_of(field(2)) as i32;
    let mut limits = [None; NODE_SQLITE_LIMIT_COUNT];
    let stored = field(3);
    if value_from_f64(stored).is_pointer() {
        let larr = raw_addr_from_value(stored) as *const ArrayHeader;
        for (i, slot) in limits.iter_mut().enumerate() {
            if (i as u32) < js_array_length(larr) {
                let value = number_of(f64_from_jsvalue(js_array_get(larr, i as u32)));
                if value >= 0.0 {
                    *slot = Some(value as i32);
                }
            }
        }
    }
    DbConfig::from_bits(path, bits, timeout, limits)
}

// ---- opening -----------------------------------------------------------

/// Open the C connection for `cfg` (rusqlite's flags, extended result codes
/// and default busy timeout, then node's pragmas and limits).
unsafe fn open_raw(cfg: &DbConfig) -> Result<*mut ffi::sqlite3, NodeSqliteBackupError> {
    let mut flags = ffi::SQLITE_OPEN_URI | ffi::SQLITE_OPEN_NOMUTEX | ffi::SQLITE_OPEN_EXRESCODE;
    if cfg.opts.read_only {
        flags |= ffi::SQLITE_OPEN_READONLY;
    }
    if cfg.opts.read_write {
        flags |= ffi::SQLITE_OPEN_READWRITE;
    }
    if cfg.opts.create {
        flags |= ffi::SQLITE_OPEN_CREATE;
    }
    let path = if cfg.path == ":memory:" {
        cfg.path.clone()
    } else {
        resolve_sqlite_path(&cfg.path)
    };
    let c_path = CString::new(path.clone()).map_err(|_| NodeSqliteBackupError {
        message: "path must not contain null bytes".to_string(),
        errcode: None,
        errstr: None,
    })?;
    let mut db: *mut ffi::sqlite3 = std::ptr::null_mut();
    let rc = ffi::sqlite3_open_v2(c_path.as_ptr(), &mut db, flags, std::ptr::null());
    if rc != ffi::SQLITE_OK {
        let errstr = sqlite_errstr(rc);
        let message = if db.is_null() {
            path
        } else {
            let msg = sqlite_errmsg_raw(db);
            ffi::sqlite3_close(db);
            if rc & 0xff == ffi::SQLITE_CANTOPEN {
                format!("{msg}: {path}")
            } else {
                msg
            }
        };
        return Err(NodeSqliteBackupError {
            message,
            errcode: Some(rc),
            errstr: Some(errstr),
        });
    }
    let fail = |db: *mut ffi::sqlite3, rc: c_int| {
        let error = NodeSqliteBackupError {
            message: sqlite_errmsg_raw(db),
            errcode: Some(rc),
            errstr: Some(sqlite_errstr(rc)),
        };
        ffi::sqlite3_close(db);
        error
    };
    let rc = ffi::sqlite3_busy_timeout(db, 5000);
    if rc != ffi::SQLITE_OK {
        return Err(fail(db, rc));
    }
    if cfg.opts.timeout_ms > 0 {
        let rc = ffi::sqlite3_busy_timeout(db, cfg.opts.timeout_ms);
        if rc != ffi::SQLITE_OK {
            return Err(fail(db, rc));
        }
    }
    let pragma = if cfg.opts.enable_foreign_keys {
        c"PRAGMA foreign_keys = ON"
    } else {
        c"PRAGMA foreign_keys = OFF"
    };
    let rc = ffi::sqlite3_exec(
        db,
        pragma.as_ptr(),
        None,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
    );
    if rc != ffi::SQLITE_OK {
        return Err(fail(db, ffi::sqlite3_extended_errcode(db)));
    }
    for (idx, value) in cfg.opts.initial_limits.iter().enumerate() {
        if let Some(value) = value {
            ffi::sqlite3_limit(db, limit_id(idx), *value);
        }
    }
    Ok(db)
}

fn limit_id(idx: usize) -> c_int {
    [
        ffi::SQLITE_LIMIT_LENGTH,
        ffi::SQLITE_LIMIT_SQL_LENGTH,
        ffi::SQLITE_LIMIT_COLUMN,
        ffi::SQLITE_LIMIT_EXPR_DEPTH,
        ffi::SQLITE_LIMIT_COMPOUND_SELECT,
        ffi::SQLITE_LIMIT_VDBE_OP,
        ffi::SQLITE_LIMIT_FUNCTION_ARG,
        ffi::SQLITE_LIMIT_ATTACHED,
        ffi::SQLITE_LIMIT_LIKE_PATTERN_LENGTH,
        ffi::SQLITE_LIMIT_VARIABLE_NUMBER,
        ffi::SQLITE_LIMIT_TRIGGER_DEPTH,
    ][idx]
}

/// CLOSED -> OPEN: open the connection and attach it to the same object.
unsafe fn open_db(this: f64, cfg: DbConfig) {
    let scope = RuntimeHandleScope::new();
    let this = scope.root_nanbox_f64(this);
    match native_payload::lifecycle(this.get_nanbox_f64(), &DB_FAMILY) {
        Ok(Lifecycle::Closed) => {}
        Ok(Lifecycle::Open) => throw_invalid_state("database is already open"),
        Ok(Lifecycle::Closing) => throw_invalid_state("database is closing"),
        Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    }
    let raw = match open_raw(&cfg) {
        Ok(raw) => raw,
        Err(error) => perry_runtime::exception::js_throw(sqlite_error_value(error)),
    };
    let configured = configure_node_sqlite_defensive(raw, cfg.opts.defensive)
        .and_then(|_| configure_node_sqlite_dqs(raw, cfg.opts.enable_dqs))
        .and_then(|_| configure_node_sqlite_load_extension(raw, cfg.enable_load_extension));
    if let Err(message) = configured {
        ffi::sqlite3_close(raw);
        throw_sqlite_error(&message);
    }
    let payload = NodeDb {
        raw,
        sessions: Vec::new(),
        sites: CallbackSites::new(),
        auth_site: std::ptr::null_mut(),
        next_callback: 1,
        agg_free: Vec::new(),
        agg_len: 0,
        serial: native_payload::next_open_serial(),
        cfg,
        live: Rc::new(Cell::new(true)),
    };
    match native_payload::attach(
        this.get_nanbox_f64(),
        &DB_FAMILY,
        payload,
        DB_EXTERNAL_BYTES,
    ) {
        Ok(()) => {}
        Err(AttachMiss::Open) => throw_invalid_state("database is already open"),
        Err(AttachMiss::Closing) => throw_invalid_state("database is closing"),
        Err(AttachMiss::Finalized | AttachMiss::Foreign) => {
            throw_invalid_state("database is not open")
        }
    }
}

// ---- construction ------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_call(_path: f64, _options: f64) -> f64 {
    throw_construct_required()
}

/// `new DatabaseSync(path, options)`.
#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_database_sync_new(
    path_value: f64,
    options_value: f64,
) -> f64 {
    let path = node_sqlite_database_path(path_value);
    let options = parse_node_sqlite_options(options_value);
    let open = options.open;
    let cfg = DbConfig {
        path,
        enable_load_extension: options.allow_extension,
        opts: options,
    };
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(native_payload::alloc_closed(&DB_FAMILY, &[]));
    let getter = |name: &str, info| {
        native_payload::define_own_accessor(db.get_nanbox_f64(), name, info, None, true, false)
    };
    getter("isOpen", builtin!(db_is_open_getter, 0));
    getter("isTransaction", builtin!(db_is_transaction_getter, 0));
    getter("limits", builtin!(db_limits_getter, 0));
    let type_symbol =
        perry_runtime::symbol::js_symbol_for(f64_from_jsvalue(string_value("sqlite-type")));
    let type_symbol = scope.root_nanbox_f64(type_symbol);
    perry_runtime::symbol::js_object_set_symbol_property(
        db.get_nanbox_f64(),
        type_symbol.get_nanbox_f64(),
        f64_from_jsvalue(string_value("node:sqlite")),
    );
    store_config(db.get_nanbox_f64(), &cfg);
    if open {
        open_db(db.get_nanbox_f64(), cfg);
    }
    db.get_nanbox_f64()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_native_dispatch(
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    construct: i32,
) -> f64 {
    let method_name = if method_name_ptr.is_null() || method_name_len == 0 {
        ""
    } else {
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(method_name_ptr, method_name_len))
    };
    let arg = |index: usize| -> f64 {
        if index < args_len && !args_ptr.is_null() {
            *args_ptr.add(index)
        } else {
            undefined_f64()
        }
    };
    if construct == perry_runtime::value::NATIVE_SQLITE_DISPATCH_PROTOTYPE {
        return match method_name {
            "DatabaseSync" => native_payload::prototype(&DB_FAMILY),
            "StatementSync" => native_payload::prototype(&STMT_FAMILY),
            "Session" => native_payload::prototype(&SESSION_FAMILY),
            _ => undefined_f64(),
        };
    }
    match (method_name, construct != 0) {
        ("DatabaseSync", true) => js_node_sqlite_database_sync_new(arg(0), arg(1)),
        ("DatabaseSync", false) => js_node_sqlite_database_sync_call(arg(0), arg(1)),
        ("Session" | "StatementSync", _) => throw_illegal_constructor(),
        ("backup", _) => js_nanbox_pointer(js_node_sqlite_backup(arg(0), arg(1), arg(2)) as i64),
        _ => undefined_f64(),
    }
}

/// Validate the `DatabaseSync` `path` argument per Node: a string or `file:` URL,
/// neither of which may contain null
/// bytes — with Node's exact `ERR_INVALID_ARG_TYPE` message (#6561).
pub(crate) unsafe fn node_sqlite_database_path(value: f64) -> String {
    const PATH_TYPE_MSG: &str =
        "The \"path\" argument must be a string, Uint8Array, or URL without null bytes.";
    let js = value_from_f64(value);
    let path = if js.is_any_string() {
        let ptr = perry_runtime::js_get_string_pointer_unified(value)
            as *const perry_runtime::StringHeader;
        string_from_header(ptr).unwrap_or_else(|| throw_type(PATH_TYPE_MSG))
    } else if let Some(bytes) = node_sqlite_path_bytes(value) {
        if bytes.contains(&0) {
            throw_type(PATH_TYPE_MSG);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    } else if js.is_pointer() {
        // URL object — accept `file:` URLs, decoding the percent-encoded
        // pathname like Node's `fileURLToPath`.
        let protocol = string_from_jsvalue(object_field(value, "protocol")).unwrap_or_default();
        let pathname = string_from_jsvalue(object_field(value, "pathname")).unwrap_or_default();
        if protocol != "file:" || pathname.is_empty() {
            throw_type(PATH_TYPE_MSG);
        }
        percent_decode_pathname(&pathname)
    } else {
        throw_type(PATH_TYPE_MSG);
    };
    if path.as_bytes().contains(&0) {
        throw_type(PATH_TYPE_MSG);
    }
    path
}

unsafe fn node_sqlite_path_bytes(value: f64) -> Option<Vec<u8>> {
    use perry_runtime::buffer::{is_registered_buffer, is_uint8array_buffer};
    let raw = raw_addr_from_value(value);
    if raw < 0x1000 {
        return None;
    }
    if (is_registered_buffer(raw) && is_uint8array_buffer(raw))
        || perry_runtime::typedarray::lookup_typed_array_kind(raw)
            == Some(perry_runtime::typedarray::KIND_UINT8)
    {
        return perry_runtime::buffer::bytes::no_gc(|scope| {
            let value = f64::from_bits(JSValue::pointer(raw as *const u8).bits());
            perry_runtime::buffer::bytes::bytes(value, scope)
                .ok()
                .map(<[u8]>::to_vec)
        });
    }
    None
}

// ---- prototype -----------------------------------------------------------

fn install_db_prototype(proto: &mut PayloadPrototype) {
    proto.method("open", builtin!(db_open_thunk, 0), 0);
    proto.method("close", builtin!(db_close_thunk, 0), 0);
    proto.method("exec", builtin!(db_exec_thunk, 1), 1);
    proto.method("prepare", builtin!(db_prepare_thunk, 2), 1);
    proto.method("serialize", builtin!(db_serialize_thunk, 1), 0);
    proto.method("deserialize", builtin!(db_deserialize_thunk, 1), 1);
    proto.method("function", builtin!(db_function_thunk, 3), 2);
    proto.method("aggregate", builtin!(db_aggregate_thunk, 2), 2);
    proto.method("enableDefensive", builtin!(db_enable_defensive_thunk, 1), 1);
    proto.method("setAuthorizer", builtin!(db_set_authorizer_thunk, 1), 1);
    proto.method("createTagStore", builtin!(db_create_tag_store_thunk, 1), 0);
    proto.method("createSession", builtin!(db_create_session_thunk, 1), 0);
    proto.method("applyChangeset", builtin!(db_apply_changeset_thunk, 2), 1);
    proto.method(
        "enableLoadExtension",
        builtin!(db_enable_load_extension_thunk, 1),
        1,
    );
    proto.method("loadExtension", builtin!(db_load_extension_thunk, 1), 1);
    proto.method("location", builtin!(db_location_thunk, 1), 0);
    proto.symbol_method(
        "dispose",
        "[Symbol.dispose]",
        builtin!(db_dispose_thunk, 0),
        0,
    );
}

extern "C" fn db_open_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        if !native_payload::is_instance(this, &DB_FAMILY) {
            throw_illegal_invocation();
        }
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this);
        let cfg = load_config(this.get_nanbox_f64());
        open_db(this.get_nanbox_f64(), cfg);
    }
    undefined_f64()
}

/// Release the connection now, or after the outermost running C call.
unsafe fn close_db(this: f64) {
    native_payload::close(this, &DB_FAMILY);
    // node drops every registration with the connection; a reopen starts
    // with none.
    native_payload::state_set(this, &DB_FAMILY, b"callbacks", undefined_f64());
    native_payload::state_set(this, &DB_FAMILY, b"aggStates", undefined_f64());
}

extern "C" fn db_close_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        match native_payload::lifecycle(this, &DB_FAMILY) {
            Ok(Lifecycle::Open) => close_db(this),
            Ok(_) | Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
            Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
        }
    }
    undefined_f64()
}

extern "C" fn db_dispose_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        if db_is_open(this) {
            close_db(this);
        }
    }
    undefined_f64()
}

extern "C" fn db_is_open_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let this = this.as_f64();
    if !native_payload::is_instance(this, &DB_FAMILY) {
        throw_illegal_invocation();
    }
    bool_f64(db_is_open(this))
}

extern "C" fn db_is_transaction_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let raw = db_payload(this.as_f64()).raw;
        bool_f64(ffi::sqlite3_get_autocommit(raw) == 0)
    }
}

extern "C" fn db_exec_thunk(_c: *const ClosureHeader, this: JsThis, sql: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let raw = db_payload(this.get_nanbox_f64()).raw;
        let sql = string_from_value(sql, "sql");
        let Ok(c_sql) = CString::new(sql) else {
            throw_sqlite_error_ext("SQL string must not contain null bytes", ffi::SQLITE_MISUSE);
        };
        let (error, end) = guarded(this.get_nanbox_f64(), || exec_raw(raw, &c_sql));
        if let Err(end) = end {
            throw_call_end(end);
        }
        if let Some(error) = error {
            throw_captured(error);
        }
    }
    undefined_f64()
}

/// `sqlite3_exec` capturing its error as plain data (callable in a guard).
pub(crate) unsafe fn exec_raw(raw: *mut ffi::sqlite3, sql: &CStr) -> Option<CapturedError> {
    let mut error_message: *mut c_char = std::ptr::null_mut();
    let rc = ffi::sqlite3_exec(
        raw,
        sql.as_ptr(),
        None,
        std::ptr::null_mut(),
        &mut error_message,
    );
    if rc == ffi::SQLITE_OK {
        return None;
    }
    let code = ffi::sqlite3_extended_errcode(raw);
    let message = if error_message.is_null() {
        sqlite_errmsg_raw(raw)
    } else {
        let message = CStr::from_ptr(error_message).to_string_lossy().into_owned();
        ffi::sqlite3_free(error_message.cast());
        message
    };
    Some(CapturedError { message, code })
}

/// Statement options over the database defaults (`prepare(sql, options)`).
unsafe fn parse_statement_options(defaults: StmtFlags, options_value: f64) -> StmtFlags {
    let js = value_from_f64(options_value);
    if js.is_undefined() {
        return defaults;
    }
    if js.is_null() || !is_object_like(options_value) {
        throw_type("The \"options\" argument must be an object.");
    }
    StmtFlags {
        read_bigints: bool_option(options_value, "readBigInts", defaults.read_bigints),
        return_arrays: bool_option(options_value, "returnArrays", defaults.return_arrays),
        allow_bare_named_parameters: bool_option(
            options_value,
            "allowBareNamedParameters",
            defaults.allow_bare_named_parameters,
        ),
        allow_unknown_named_parameters: bool_option(
            options_value,
            "allowUnknownNamedParameters",
            defaults.allow_unknown_named_parameters,
        ),
    }
}

extern "C" fn db_prepare_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    sql_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let defaults = db_payload(this.get_nanbox_f64()).cfg.stmt_flags();
        if !value_from_f64(sql_value).is_any_string() {
            throw_type("The \"sql\" argument must be of type string");
        }
        let sql = string_from_value(sql_value, "sql");
        let flags = parse_statement_options(defaults, options_value);
        let Ok(c_sql) = CString::new(sql) else {
            throw_sqlite_error("SQL string must not contain null bytes");
        };
        let raw = db_payload(this.get_nanbox_f64()).raw;
        // Compiled once, as node does: the statement owns it from here.
        let stmt = prepare_guarded(&this, raw, &c_sql);
        let open = db_payload(this.get_nanbox_f64());
        let (serial, live) = (open.serial, open.live.clone());
        new_statement(&this, serial, live, c_sql, stmt, flags)
    }
}

/// Compile `sql` on the open connection, under a guard (the authorizer
/// runs here). Throws node's error after cleaning up.
pub(crate) unsafe fn prepare_guarded(
    db: &RuntimeHandle<'_>,
    raw: *mut ffi::sqlite3,
    sql: &CStr,
) -> *mut ffi::sqlite3_stmt {
    prepare_guarded_with_link(db, raw, sql).0
}

pub(crate) unsafe fn prepare_guarded_with_link(
    db: &RuntimeHandle<'_>,
    raw: *mut ffi::sqlite3,
    sql: &CStr,
) -> (*mut ffi::sqlite3_stmt, native_payload::OwnerLink) {
    let guard = match native_payload::enter(db.get_nanbox_f64(), &DB_FAMILY) {
        Ok(guard) => guard,
        Err(PayloadMiss::Closed) => throw_invalid_state("database is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    };
    let link = guard.owner_link();
    let ((stmt, error), end) = guard.call(|| {
        let mut stmt = std::ptr::null_mut();
        let rc = ffi::sqlite3_prepare_v2(raw, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
        let error = (rc != ffi::SQLITE_OK).then(|| capture_error(raw));
        (stmt, error)
    });
    // A deferred close released inside the call finalized every statement
    // of the connection, this one included.
    if let Err(end) = end {
        if !stmt.is_null() && db_is_open(db.get_nanbox_f64()) {
            ffi::sqlite3_finalize(stmt);
        }
        throw_call_end(end);
    }
    if let Some(error) = error {
        if !stmt.is_null() {
            ffi::sqlite3_finalize(stmt);
        }
        throw_captured(error);
    }
    (stmt, link)
}

extern "C" fn db_serialize_thunk(_c: *const ClosureHeader, this: JsThis, schema: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let schema = if value_from_f64(schema).is_undefined() {
            "main".to_string()
        } else {
            string_from_value(schema, "attachedDb")
        };
        let schema = CString::new(schema)
            .unwrap_or_else(|_| throw_type("The \"attachedDb\" argument must be a string"));
        let raw = db_payload(this.get_nanbox_f64()).raw;
        js_nanbox_pointer(sqlite_serialize_to_buffer(raw, &schema) as i64)
    }
}

extern "C" fn db_deserialize_thunk(_c: *const ClosureHeader, this: JsThis, image: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let bytes = sqlite_image_bytes(image);
        let raw = db_payload(this.get_nanbox_f64()).raw;
        sqlite_deserialize_main(raw, &bytes);
    }
    undefined_f64()
}

/// Take `count` consecutive callback indices and a userdata site for the
/// first. The payload borrow ends before any JS-heap allocation.
unsafe fn reserve_callbacks(
    this: f64,
    count: u32,
    use_bigint_arguments: bool,
) -> (u32, *mut c_void) {
    let link = native_payload::owner_link(this, &DB_FAMILY)
        .unwrap_or_else(|_| throw_invalid_state("database is not open"));
    let db = db_payload(this);
    let base = db.next_callback;
    db.next_callback = base
        .checked_add(count)
        .filter(|next| *next < SITE_BIGINT_ARGS)
        .unwrap_or_else(|| throw_invalid_state("too many registered functions"));
    let flag = if use_bigint_arguments {
        SITE_BIGINT_ARGS
    } else {
        0
    };
    let site = db.sites.site(link, base | flag);
    (base, site)
}

extern "C" fn db_function_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    name_value: f64,
    options_or_function_value: f64,
    function_value: f64,
) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let name = sqlite_function_name(string_from_value(name_value, "name"));
        let (options_value, callback) =
            if closure_ptr_from_value(options_or_function_value).is_some() {
                (undefined_f64(), options_or_function_value)
            } else {
                let options_js = value_from_f64(options_or_function_value);
                if options_js.is_undefined() && value_from_f64(function_value).is_undefined() {
                    node_sqlite_function_arg(options_or_function_value, "function");
                }
                if options_js.is_null()
                    || options_js.is_undefined()
                    || !is_object_like(options_or_function_value)
                {
                    throw_type("The \"options\" argument must be an object.");
                }
                (
                    options_or_function_value,
                    node_sqlite_function_arg(function_value, "function"),
                )
            };
        let callback = scope.root_nanbox_f64(callback);
        let use_bigint_arguments =
            node_sqlite_bool_option_exact(options_value, "useBigIntArguments", false);
        let varargs = node_sqlite_bool_option_exact(options_value, "varargs", false);
        let deterministic = node_sqlite_bool_option_exact(options_value, "deterministic", false);
        let direct_only = node_sqlite_bool_option_exact(options_value, "directOnly", false);
        let argc = if varargs {
            -1
        } else {
            node_sqlite_closure_arity(callback.get_nanbox_f64())
        };
        let mut text_rep = ffi::SQLITE_UTF8;
        if deterministic {
            text_rep |= ffi::SQLITE_DETERMINISTIC;
        }
        if direct_only {
            text_rep |= ffi::SQLITE_DIRECTONLY;
        }
        let (index, site) = reserve_callbacks(this.get_nanbox_f64(), 1, use_bigint_arguments);
        native_payload::set_callback(
            this.get_nanbox_f64(),
            &DB_FAMILY,
            index,
            callback.get_nanbox_f64(),
        );
        let raw = db_payload(this.get_nanbox_f64()).raw;
        let rc = ffi::sqlite3_create_function_v2(
            raw,
            name.as_ptr(),
            argc,
            text_rep,
            site,
            Some(node_sqlite_scalar_trampoline),
            None,
            None,
            None,
        );
        if rc != ffi::SQLITE_OK {
            throw_sqlite_error_from_db(raw);
        }
    }
    undefined_f64()
}

extern "C" fn db_aggregate_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    name_value: f64,
    options_value: f64,
) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let name = sqlite_function_name(string_from_value(name_value, "name"));
        if value_from_f64(options_value).is_null() || !is_object_like(options_value) {
            throw_plain_type("The \"options\" argument must be an object.");
        }
        let options = scope.root_nanbox_f64(options_value);
        let start = object_field(options.get_nanbox_f64(), "start");
        if start.is_undefined() {
            throw_type("The \"options.start\" argument must be a function or a primitive value.");
        }
        let start = scope.root_nanbox_u64(start.bits());
        let step = node_sqlite_optional_callback_option(options.get_nanbox_f64(), "step", true)
            .unwrap_or_else(|| throw_type("The \"options.step\" argument must be a function."));
        let step = scope.root_nanbox_f64(step);
        let result =
            node_sqlite_optional_callback_option(options.get_nanbox_f64(), "result", false)
                .unwrap_or_else(undefined_f64);
        let result = scope.root_nanbox_f64(result);
        let inverse =
            node_sqlite_optional_callback_option(options.get_nanbox_f64(), "inverse", true)
                .unwrap_or_else(undefined_f64);
        let inverse = scope.root_nanbox_f64(inverse);
        let has_inverse = !value_from_f64(inverse.get_nanbox_f64()).is_undefined();
        let use_bigint_arguments =
            node_sqlite_bool_option_exact(options.get_nanbox_f64(), "useBigIntArguments", false);
        let varargs = node_sqlite_bool_option_exact(options.get_nanbox_f64(), "varargs", false);
        let direct_only =
            node_sqlite_bool_option_exact(options.get_nanbox_f64(), "directOnly", false);
        let argc = if varargs {
            -1
        } else {
            node_sqlite_closure_arity(step.get_nanbox_f64()).saturating_sub(1)
        };
        let mut text_rep = ffi::SQLITE_UTF8;
        if direct_only {
            text_rep |= ffi::SQLITE_DIRECTONLY;
        }
        let (base, site) =
            reserve_callbacks(this.get_nanbox_f64(), AGG_SLOTS, use_bigint_arguments);
        // Each store may collect: read every value from its handle.
        let owner = this.get_nanbox_f64();
        native_payload::set_callback(
            owner,
            &DB_FAMILY,
            base + AGG_START,
            f64::from_bits(start.get_nanbox_u64()),
        );
        let owner = this.get_nanbox_f64();
        native_payload::set_callback(owner, &DB_FAMILY, base + AGG_STEP, step.get_nanbox_f64());
        let owner = this.get_nanbox_f64();
        native_payload::set_callback(
            owner,
            &DB_FAMILY,
            base + AGG_RESULT,
            result.get_nanbox_f64(),
        );
        let owner = this.get_nanbox_f64();
        native_payload::set_callback(
            owner,
            &DB_FAMILY,
            base + AGG_INVERSE,
            inverse.get_nanbox_f64(),
        );
        let raw = db_payload(this.get_nanbox_f64()).raw;
        let rc = ffi::sqlite3_create_window_function(
            raw,
            name.as_ptr(),
            argc,
            text_rep,
            site,
            Some(node_sqlite_aggregate_step),
            Some(node_sqlite_aggregate_final),
            if has_inverse {
                Some(node_sqlite_aggregate_value)
            } else {
                None
            },
            if has_inverse {
                Some(node_sqlite_aggregate_inverse)
            } else {
                None
            },
            None,
        );
        if rc != ffi::SQLITE_OK {
            throw_sqlite_error_from_db(raw);
        }
    }
    undefined_f64()
}

extern "C" fn db_set_authorizer_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    callback_value: f64,
) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let js = value_from_f64(callback_value);
        let enable = if js.is_null() {
            false
        } else {
            if closure_ptr_from_value(callback_value).is_none() {
                throw_type("The \"callback\" argument must be a function or null.");
            }
            true
        };
        native_payload::set_callback(
            this.get_nanbox_f64(),
            &DB_FAMILY,
            AUTHORIZER_INDEX,
            if enable {
                callback_value
            } else {
                undefined_f64()
            },
        );
        let link = native_payload::owner_link(this.get_nanbox_f64(), &DB_FAMILY)
            .unwrap_or_else(|_| throw_invalid_state("database is not open"));
        let db = db_payload(this.get_nanbox_f64());
        if db.auth_site.is_null() {
            db.auth_site = db.sites.site(link, AUTHORIZER_INDEX);
        }
        let (raw, site) = (db.raw, db.auth_site);
        let rc = ffi::sqlite3_set_authorizer(
            raw,
            if enable {
                Some(node_sqlite_authorizer_trampoline)
            } else {
                None
            },
            if enable { site } else { std::ptr::null_mut() },
        );
        if rc != ffi::SQLITE_OK {
            throw_sqlite_error_from_db(raw);
        }
    }
    undefined_f64()
}

extern "C" fn db_enable_defensive_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    active: f64,
) -> f64 {
    unsafe {
        let js = value_from_f64(active);
        if !js.is_bool() {
            throw_type("The \"active\" argument must be a boolean.");
        }
        let active = js.as_bool();
        let raw = db_payload(this.as_f64()).raw;
        if let Err(message) = configure_node_sqlite_defensive(raw, active) {
            throw_sqlite_error(&message);
        }
        let db = db_payload(this.as_f64());
        db.cfg.opts.defensive = active;
        let cfg = db.cfg.clone();
        store_config_bits(this.as_f64(), &cfg);
    }
    undefined_f64()
}

extern "C" fn db_enable_load_extension_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    allow_value: f64,
) -> f64 {
    unsafe {
        let js = value_from_f64(allow_value);
        if !js.is_bool() {
            throw_type("The \"allow\" argument must be a boolean");
        }
        let allow = js.as_bool();
        let this = this.as_f64();
        if !native_payload::is_instance(this, &DB_FAMILY) {
            throw_illegal_invocation();
        }
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this);
        let mut cfg = match native_payload::payload_mut::<NodeDb>(this.get_nanbox_f64(), &DB_FAMILY)
        {
            Ok(db) => db.cfg.clone(),
            Err(_) => load_config(this.get_nanbox_f64()),
        };
        if allow && !cfg.opts.allow_extension {
            throw_invalid_state(
                "Cannot enable extension loading because it was disabled at database creation.",
            );
        }
        if let Ok(db) = native_payload::payload_mut::<NodeDb>(this.get_nanbox_f64(), &DB_FAMILY) {
            if let Err(message) = configure_node_sqlite_load_extension(db.raw, allow) {
                throw_sqlite_error(&message);
            }
            db.cfg.enable_load_extension = allow;
        }
        cfg.enable_load_extension = allow;
        store_config_bits(this.get_nanbox_f64(), &cfg);
    }
    undefined_f64()
}

extern "C" fn db_load_extension_thunk(_c: *const ClosureHeader, this: JsThis, path: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        let db = db_payload(this.get_nanbox_f64());
        if !db.cfg.opts.allow_extension || !db.cfg.enable_load_extension {
            throw_invalid_state("extension loading is not allowed");
        }
        let path = string_from_value(path, "path");
        let c_path = CString::new(path)
            .unwrap_or_else(|_| throw_type("The \"path\" argument must not contain null bytes"));
        let raw = db_payload(this.get_nanbox_f64()).raw;
        sqlite_load_extension_or_throw(raw, &c_path);
    }
    undefined_f64()
}

extern "C" fn db_location_thunk(_c: *const ClosureHeader, this: JsThis, db_name: f64) -> f64 {
    unsafe {
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64(this.as_f64());
        db_payload(this.get_nanbox_f64());
        let db_name = if value_from_f64(db_name).is_undefined() {
            "main".to_string()
        } else {
            string_from_value(db_name, "dbName")
        };
        let c_name = CString::new(db_name)
            .unwrap_or_else(|_| throw_type("The \"dbName\" argument must not contain null bytes"));
        let raw = db_payload(this.get_nanbox_f64()).raw;
        let filename = ffi::sqlite3_db_filename(raw, c_name.as_ptr());
        if filename.is_null() {
            return null_f64();
        }
        let filename = CStr::from_ptr(filename).to_str().unwrap_or("");
        if filename.is_empty() {
            null_f64()
        } else {
            f64_from_jsvalue(string_value(filename))
        }
    }
}

extern "C" fn db_create_tag_store_thunk(_c: *const ClosureHeader, this: JsThis, max: f64) -> f64 {
    unsafe { new_tag_store(this.as_f64(), node_sqlite_tag_store_capacity(max)) }
}

extern "C" fn db_create_session_thunk(_c: *const ClosureHeader, this: JsThis, options: f64) -> f64 {
    unsafe { new_session(this.as_f64(), options) }
}

extern "C" fn db_apply_changeset_thunk(
    _c: *const ClosureHeader,
    this: JsThis,
    changeset: f64,
    options: f64,
) -> f64 {
    unsafe { apply_changeset(this.as_f64(), changeset, options) }
}

// ---- db.limits -----------------------------------------------------------

fn install_limits_prototype(_proto: &mut PayloadPrototype) {}

extern "C" fn db_limits_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    {
        let this = this.as_f64();
        if !native_payload::is_instance(this, &DB_FAMILY) {
            throw_illegal_invocation();
        }
        let existing = native_payload::state_get_memo(this, &DB_FAMILY, b"limits", &MEMO_LIMITS);
        if value_from_f64(existing).is_pointer() {
            return existing;
        }
        let scope = RuntimeHandleScope::new();
        let db = scope.root_nanbox_f64(this);
        let limits = scope.root_nanbox_f64(native_payload::alloc(&LIMITS_FAMILY, (), 0, &[]));
        native_payload::state_set_memo(
            limits.get_nanbox_f64(),
            &LIMITS_FAMILY,
            b"db",
            db.get_nanbox_f64(),
            &MEMO_DB,
        );
        for (name, get, set) in limit_accessors() {
            native_payload::define_own_accessor(
                limits.get_nanbox_f64(),
                name,
                get,
                Some(set),
                true,
                false,
            );
        }
        native_payload::state_set_memo(
            db.get_nanbox_f64(),
            &DB_FAMILY,
            b"limits",
            limits.get_nanbox_f64(),
            &MEMO_LIMITS,
        );
        limits.get_nanbox_f64()
    }
}

unsafe fn limit_get(this: f64, idx: usize) -> f64 {
    if !native_payload::is_instance(this, &LIMITS_FAMILY) {
        throw_illegal_invocation();
    }
    let db = native_payload::state_get_memo(this, &LIMITS_FAMILY, b"db", &MEMO_DB);
    let raw = db_payload(db).raw;
    f64_from_jsvalue(JSValue::number(
        ffi::sqlite3_limit(raw, limit_id(idx), -1) as f64
    ))
}

unsafe fn limit_set(this: f64, idx: usize, name: &str, value: f64) -> f64 {
    if !native_payload::is_instance(this, &LIMITS_FAMILY) {
        throw_illegal_invocation();
    }
    let db = native_payload::state_get_memo(this, &LIMITS_FAMILY, b"db", &MEMO_DB);
    db_payload(db);
    let new_value = non_negative_i32_value(value_from_f64(value), name, true);
    let raw = db_payload(db).raw;
    ffi::sqlite3_limit(raw, limit_id(idx), new_value);
    undefined_f64()
}

macro_rules! limit_thunks {
    ($( $idx:expr, $name:literal, $get:ident, $set:ident; )*) => {
        $(
            extern "C" fn $get(_c: *const ClosureHeader, this: JsThis) -> f64 {
                unsafe { limit_get(this.as_f64(), $idx) }
            }
            extern "C" fn $set(_c: *const ClosureHeader, this: JsThis, value: f64) -> f64 {
                unsafe { limit_set(this.as_f64(), $idx, $name, value) }
            }
        )*
        fn limit_accessors() -> Vec<(
            &'static str,
            *const perry_runtime::closure::JsFunctionInfo,
            *const perry_runtime::closure::JsFunctionInfo,
        )> {
            vec![$( ($name, builtin!($get, 0), builtin!($set, 1)), )*]
        }
    };
}

limit_thunks! {
    0, "length", limit_get_length, limit_set_length;
    1, "sqlLength", limit_get_sql_length, limit_set_sql_length;
    2, "column", limit_get_column, limit_set_column;
    3, "exprDepth", limit_get_expr_depth, limit_set_expr_depth;
    4, "compoundSelect", limit_get_compound_select, limit_set_compound_select;
    5, "vdbeOp", limit_get_vdbe_op, limit_set_vdbe_op;
    6, "functionArg", limit_get_function_arg, limit_set_function_arg;
    7, "attach", limit_get_attach, limit_set_attach;
    8, "likePatternLength", limit_get_like_pattern_length, limit_set_like_pattern_length;
    9, "variableNumber", limit_get_variable_number, limit_set_variable_number;
    10, "triggerDepth", limit_get_trigger_depth, limit_set_trigger_depth;
}

// ---- sqlite.backup() -------------------------------------------------------

/// `backup(sourceDb, path, options)`: a promise for the page count. The
/// progress callback runs between steps through `call_from_native` inside
/// one guard, so a throw or a `close()` from it ends the backup cleanly.
#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_backup(
    source_db_value: f64,
    path_value: f64,
    options_value: f64,
) -> *mut Promise {
    if !value_from_f64(source_db_value).is_pointer() {
        throw_type("The \"sourceDb\" argument must be an object.");
    }
    if !native_payload::is_instance(source_db_value, &DB_FAMILY) {
        throw_type("The \"sourceDb\" argument must be an instance of DatabaseSync.");
    }
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(source_db_value);
    db_payload(db.get_nanbox_f64());
    let path = path_like_from_value(path_value, "path");
    let options = parse_node_sqlite_backup_options(options_value);
    let progress = options.progress.map(|p| scope.root_nanbox_f64(p));
    let raw = db_payload(db.get_nanbox_f64()).raw;
    let (result, end) = guarded(db.get_nanbox_f64(), || {
        perform_node_sqlite_backup(raw, &path, &options, |total, remaining| {
            let Some(progress) = &progress else {
                return true;
            };
            let info = backup_progress_info(total, remaining);
            native_payload::call_from_native(
                db.get_nanbox_f64(),
                progress.get_nanbox_f64(),
                undefined_f64(),
                &[info],
            )
            .is_ok()
        })
    });
    match end {
        Err(CallEnd::Threw(value)) => return js_promise_rejected(value),
        Err(CallEnd::Closed) => {
            return js_promise_rejected(error_value_with_code(
                "database is not open",
                "ERR_INVALID_STATE",
            ))
        }
        Ok(()) => {}
    }
    match result {
        Ok(total_pages) => {
            js_promise_resolved(f64::from_bits(JSValue::number(total_pages as f64).bits()))
        }
        Err(error) => js_promise_rejected(sqlite_error_value(error)),
    }
}
