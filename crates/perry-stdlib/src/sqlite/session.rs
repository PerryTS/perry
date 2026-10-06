//! `node:sqlite` `Session` (`db.createSession()`) and `db.applyChangeset()`.
//!
//! A session is an ordinary object that owns a native payload (#11919). Its
//! `sqlite3_session` must be deleted before the connection closes, so the
//! database payload and the session payload share one `SessionCell`: whichever
//! goes first (an explicit `close()`, the database's release or a sweep)
//! deletes the C session, and the other finds it gone. `applyChangeset` is a
//! class-C family: its `filter` / `onConflict` live for one call, in stack
//! userdata of runtime handles.

use super::*;
use perry_runtime::buffer::bytes::{from_slice, Brand};
use perry_runtime::buffer::{
    is_any_array_buffer, is_data_view, is_registered_buffer, BufferHeader,
};
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::native_class_ids::SQLITE_SESSION;
use perry_runtime::native_payload::{
    self, NativePayloadFamily, OpenSerial, PayloadMiss, PayloadPrototype,
};
use rusqlite::ffi;
use std::ffi::CString;
use std::os::raw::{c_int, c_void};

perry_runtime::birth_memo!(static SESSION_BIRTH);
use std::rc::Rc;

perry_runtime::state_key_memo!(static MEMO_DB);

macro_rules! builtin {
    ($body:path, $n:tt) => {
        perry_runtime::fn_info!($body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN))
    };
}

pub(crate) static SESSION_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_SESSION,
    links_owner: false,
    name: "Session",
    constructor_export: Some(("sqlite", "Session")),
    constructor_length: 0,
    install_prototype: install_session_prototype,
};

pub(crate) struct NodeSession {
    cell: Rc<SessionCell>,
    serial: OpenSerial,
}

impl Drop for NodeSession {
    fn drop(&mut self) {
        self.cell.delete();
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_call(_arg0: f64, _arg1: f64) -> f64 {
    throw_illegal_constructor()
}

#[no_mangle]
pub unsafe extern "C" fn js_node_sqlite_session_new(_arg0: f64, _arg1: f64) -> f64 {
    throw_illegal_constructor()
}

/// `db.createSession(options)`.
pub(crate) unsafe fn new_session(db: f64, options_value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(db);
    validate_optional_object(options_value);
    let db_name = string_option(options_value, "db", Some("main")).unwrap_or_else(|| "main".into());
    let table_name = string_option(options_value, "table", None);
    let raw = db_payload(db.get_nanbox_f64()).raw;
    let db_name_c = CString::new(db_name)
        .unwrap_or_else(|_| throw_type("The \"options.db\" argument must not contain null bytes"));
    let table_name_c = table_name.as_ref().map(|name| {
        CString::new(name.as_str()).unwrap_or_else(|_| {
            throw_type("The \"options.table\" argument must not contain null bytes")
        })
    });
    let mut raw_session: *mut ffi::sqlite3_session = std::ptr::null_mut();
    let rc = ffi::sqlite3session_create(raw, db_name_c.as_ptr(), &mut raw_session);
    if rc != ffi::SQLITE_OK {
        throw_sqlite_error_from_db(raw);
    }
    let table_ptr = table_name_c
        .as_ref()
        .map(|name| name.as_ptr())
        .unwrap_or(std::ptr::null());
    let rc = ffi::sqlite3session_attach(raw_session, table_ptr);
    if rc != ffi::SQLITE_OK {
        let error = capture_error(raw);
        ffi::sqlite3session_delete(raw_session);
        throw_captured(error);
    }
    let cell = SessionCell::new(raw_session);
    let serial = {
        let open = db_payload(db.get_nanbox_f64());
        open.sessions.retain(|session| !session.raw().is_null());
        open.sessions.push(cell.clone());
        open.serial
    };
    native_payload::alloc_with_state(
        &SESSION_FAMILY,
        NodeSession { cell, serial },
        std::mem::size_of::<NodeSession>(),
        &[],
        &[(b"db", db.get_nanbox_f64())],
        &[],
        &SESSION_BIRTH,
    )
}

/// The open C session of `this`: node's "database is not open" first, then
/// "session is not open".
unsafe fn live_session(this: f64) -> *mut ffi::sqlite3_session {
    if !native_payload::is_instance(this, &SESSION_FAMILY) {
        throw_illegal_invocation();
    }
    let db = native_payload::state_get_memo(this, &SESSION_FAMILY, b"db", &MEMO_DB);
    let db_serial = match native_payload::payload_mut::<NodeDb>(db, &DB_FAMILY) {
        Ok(open) => open.serial,
        Err(_) => throw_invalid_state("database is not open"),
    };
    match native_payload::payload_mut::<NodeSession>(this, &SESSION_FAMILY) {
        Ok(session) if session.serial == db_serial && !session.cell.raw().is_null() => {
            session.cell.raw()
        }
        Ok(_) | Err(PayloadMiss::Closed) => throw_invalid_state("session is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    }
}

unsafe fn session_blob(
    this: f64,
    make_blob: unsafe extern "C" fn(
        *mut ffi::sqlite3_session,
        *mut c_int,
        *mut *mut c_void,
    ) -> c_int,
) -> f64 {
    let raw_session = live_session(this);
    let db = native_payload::state_get_memo(this, &SESSION_FAMILY, b"db", &MEMO_DB);
    let raw_db = db_payload(db).raw;
    let mut len: c_int = 0;
    let mut data: *mut c_void = std::ptr::null_mut();
    let rc = make_blob(raw_session, &mut len, &mut data);
    if rc != ffi::SQLITE_OK {
        let error = capture_error(raw_db);
        if !data.is_null() {
            ffi::sqlite3_free(data);
        }
        throw_captured(error);
    }
    let len = len.max(0) as usize;
    let bytes: &[u8] = if len > 0 && !data.is_null() {
        std::slice::from_raw_parts(data as *const u8, len)
    } else {
        &[]
    };
    let buffer = from_slice(Brand::Uint8Array, bytes);
    if !data.is_null() {
        ffi::sqlite3_free(data);
    }
    buffer
}

extern "C" fn session_changeset_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { session_blob(this.as_f64(), ffi::sqlite3session_changeset) }
}

extern "C" fn session_patchset_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe { session_blob(this.as_f64(), ffi::sqlite3session_patchset) }
}

extern "C" fn session_close_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let this = this.as_f64();
        live_session(this);
        native_payload::close(this, &SESSION_FAMILY);
    }
    undefined_f64()
}

extern "C" fn session_dispose_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    let this = this.as_f64();
    native_payload::close(this, &SESSION_FAMILY);
    undefined_f64()
}

fn install_session_prototype(proto: &mut PayloadPrototype) {
    proto.method("changeset", builtin!(session_changeset_thunk, 0), 0);
    proto.method("patchset", builtin!(session_patchset_thunk, 0), 0);
    proto.method("close", builtin!(session_close_thunk, 0), 0);
    proto.symbol_method(
        "dispose",
        "[Symbol.dispose]",
        builtin!(session_dispose_thunk, 0),
        0,
    );
}

pub(crate) unsafe fn changeset_bytes_from_value(value: f64) -> Vec<u8> {
    let addr = raw_addr_from_value(value);
    if addr != 0 {
        if (is_registered_buffer(addr) && !is_any_array_buffer(addr) && !is_data_view(addr))
            || perry_runtime::typedarray::lookup_typed_array_kind(addr)
                == Some(perry_runtime::typedarray::KIND_UINT8)
        {
            let bytes = perry_runtime::buffer::bytes::no_gc(|scope| {
                let value =
                    f64::from_bits(perry_runtime::JSValue::pointer(addr as *const u8).bits());
                perry_runtime::buffer::bytes::bytes(value, scope)
                    .ok()
                    .map(<[u8]>::to_vec)
            });
            if let Some(bytes) = bytes {
                return bytes;
            }
        }
    }
    throw_type("The \"changeset\" argument must be a Uint8Array.");
}

/// `db.applyChangeset(changeset, options)`: one guarded C call whose
/// callbacks are this call's arguments.
pub(crate) unsafe fn apply_changeset(db: f64, changeset_value: f64, options_value: f64) -> f64 {
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(db);
    db_payload(db.get_nanbox_f64());
    let changeset = changeset_bytes_from_value(changeset_value);
    validate_optional_object(options_value);
    let filter = function_option(options_value, "filter").map(|f| scope.root_nanbox_f64(f));
    let on_conflict =
        function_option(options_value, "onConflict").map(|f| scope.root_nanbox_f64(f));
    let mut context = ChangesetApplyContext {
        owner: &db,
        filter: filter.as_ref(),
        on_conflict: on_conflict.as_ref(),
    };
    let raw = db_payload(db.get_nanbox_f64()).raw;
    let has_filter = context.filter.is_some();
    let context_ptr = &mut context as *mut ChangesetApplyContext<'_, '_> as *mut c_void;
    let ((rc, error), end) = guarded(db.get_nanbox_f64(), || {
        let rc = ffi::sqlite3changeset_apply(
            raw,
            changeset.len() as c_int,
            changeset.as_ptr() as *mut c_void,
            if has_filter {
                Some(node_sqlite_changeset_filter)
            } else {
                None
            },
            Some(node_sqlite_changeset_conflict),
            context_ptr,
        );
        let error = (rc != ffi::SQLITE_OK && rc != ffi::SQLITE_ABORT).then(|| capture_error(raw));
        (rc, error)
    });
    if let Err(end) = end {
        throw_call_end(end);
    }
    if let Some(error) = error {
        throw_captured(error);
    }
    bool_f64(rc == ffi::SQLITE_OK)
}
