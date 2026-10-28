//! Handle-dispatch extension for better-sqlite3 `Database` and `Statement`.
//!
//! `new Database(path)` and `db.prepare(sql)` return native handle ids. A call
//! on a statically typed receiver is lowered through the native table
//! (`native_table/databases.rs`). A call whose receiver type was erased (`any`,
//! an object field, a Map value) reaches the runtime's handle-method dispatch,
//! which asks each registered extension in turn. This one claims only ids that
//! are live `SqliteDbHandle` / `SqliteStmtHandle` entries of THIS crate and
//! only the methods the typed table supports, with the same argument
//! conventions: `run`/`get`/`all` take their variadic parameters as one array.
//! Everything else is left unclaimed so a user object keeps its own methods.

use std::sync::Once;

use perry_ffi::{
    alloc_string, get_handle, js_array_alloc, js_array_push, ArrayHeader, Handle, JsValue,
    StringHeader, TransientRootScope, TransientRootedNanbox,
};

use crate::{SqliteDbHandle, SqliteStmtHandle};

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
    /// Bind a method of a handle-backed receiver as a value, so
    /// `const f = stmt.get` and `typeof stmt.get === "function"` behave as
    /// they do in node. `perry-ext-ws` uses this for the same reason.
    fn js_class_method_bind(receiver: f64, name: *const u8, len: usize) -> f64;
}

/// The methods the typed table and the method dispatch above serve. A read of
/// one of these names yields the bound method rather than `undefined`.
const STMT_METHODS: &[&str] = &["run", "get", "all", "raw"];
const DB_METHODS: &[&str] = &["prepare", "exec", "close"];

/// Idempotent; called from `js_sqlite_open`, so it runs before any method call
/// on a Database can be made.
pub(crate) fn ensure_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        // SAFETY: a plain registration with a `'static` function pointer.
        unsafe { js_register_handle_method_dispatch_extension(sqlite_method_dispatch_ext) };
        // Without this, every documented data property on a Database or a
        // Statement reads `undefined`: method calls have both the typed table
        // and the dispatch above, and property reads had neither.
        unsafe { js_register_handle_property_dispatch_extension(sqlite_property_dispatch_ext) };
    });
}

/// The parameter array for a statement call. Each push returns the array to
/// use next and nothing else is held, so no earlier pointer outlives a push.
unsafe fn rooted_params_array(rooted: &[TransientRootedNanbox]) -> *mut ArrayHeader {
    rooted
        .iter()
        .fold(js_array_alloc(rooted.len() as u32), |arr, v| {
            js_array_push(arr, JsValue::from_bits(v.get().to_bits()))
        })
}

fn bits(v: JsValue) -> f64 {
    f64::from_bits(v.bits())
}

fn arg(args: &[f64], i: usize) -> f64 {
    args.get(i)
        .copied()
        .unwrap_or_else(|| bits(JsValue::UNDEFINED))
}

/// A heap `StringHeader` for a string argument (short or heap), or null.
fn string_ptr(value: f64) -> *const StringHeader {
    match JsValue::from_bits(value.to_bits()).to_owned_string() {
        Some(s) => alloc_string(&s).as_raw() as *const StringHeader,
        None => std::ptr::null(),
    }
}

/// Dispatch one method; `None` means "not claimed".
unsafe fn call(handle: Handle, name: &str, args: &[f64]) -> Option<f64> {
    let undefined = JsValue::UNDEFINED;
    if get_handle::<SqliteDbHandle>(handle).is_some() {
        return Some(match name {
            "prepare" => {
                let stmt = crate::js_sqlite_prepare(handle, string_ptr(arg(args, 0)));
                if stmt < 0 {
                    bits(undefined)
                } else {
                    bits(JsValue::from_handle(stmt as u64))
                }
            }
            "exec" => {
                crate::js_sqlite_exec(handle, string_ptr(arg(args, 0)));
                bits(JsValue::from_handle(handle as u64))
            }
            "close" => {
                crate::js_sqlite_close(handle);
                bits(undefined)
            }
            _ => return None,
        });
    }
    if get_handle::<SqliteStmtHandle>(handle).is_some() {
        return Some(match name {
            "raw" => {
                crate::js_sqlite_stmt_raw(handle);
                bits(JsValue::from_handle(handle as u64))
            }
            "run" | "get" | "all" => {
                let scope = TransientRootScope::enter();
                // Short strings become heap strings first (the parameter
                // reader only knows heap ones); every value stays rooted
                // while later ones allocate, then the array is filled
                // without allocating (capacity is exact).
                let rooted: Vec<_> = args
                    .iter()
                    .map(|v| {
                        let jv = JsValue::from_bits(v.to_bits());
                        let v = if jv.is_short_string() {
                            bits(JsValue::from_string_ptr(
                                alloc_string(&jv.to_owned_string().unwrap_or_default()).as_raw(),
                            ))
                        } else {
                            *v
                        };
                        scope.root_nanbox(v)
                    })
                    .collect();
                let params = rooted_params_array(&rooted);
                match name {
                    "run" => {
                        let r = crate::js_sqlite_stmt_run(handle, params);
                        bits(if r.is_null() {
                            undefined
                        } else {
                            JsValue::from_object_ptr(r)
                        })
                    }
                    "get" => crate::js_sqlite_stmt_get(handle, params),
                    _ => bits(JsValue::from_object_ptr(crate::js_sqlite_stmt_all(
                        handle, params,
                    ))),
                }
            }
            _ => return None,
        });
    }
    None
}

/// # Safety
/// Called by the runtime's composite dispatcher with a live name and argument
/// slice, and an `out` pointer for one `f64`.
unsafe extern "C" fn sqlite_method_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    if handle <= 0 || name_ptr.is_null() || name_len == 0 || out.is_null() {
        return 0;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return 0;
    };
    let args: &[f64] = if args_len == 0 || args_ptr.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    match call(handle as Handle, name, args) {
        Some(value) => {
            *out = value;
            1
        }
        None => 0,
    }
}

/// Read one property; `None` means "not claimed".
///
/// better-sqlite3 exposes these as data properties on the Database and the
/// Statement. Perry hands both to JS as handle ids, so a property read has no
/// object to find them on and arrives here instead.
unsafe fn property_of(handle: Handle, name: &str) -> Option<f64> {
    if let Some(db) = get_handle::<SqliteDbHandle>(handle) {
        let conn = db.conn.lock().ok()?;
        return Some(match name {
            // `Connection::path` is None for a temporary database and
            // ":memory:" for an in-memory one, which is what better-sqlite3
            // reports verbatim.
            "name" => {
                // rusqlite reports an in-memory database as None or "";
                // better-sqlite3 reports it as ":memory:".
                let path = match conn.path() {
                    None | Some("") => ":memory:".to_string(),
                    Some(p) => p.to_string(),
                };
                bits(JsValue::from_string_ptr(alloc_string(&path).as_raw() as *mut StringHeader))
            }
            // The handle resolved, so the database is open; `close()` drops it.
            "open" => bits(JsValue::from_bool(true)),
            "inTransaction" => bits(JsValue::from_bool(!conn.is_autocommit())),
            "memory" => bits(JsValue::from_bool(
                conn.path().is_none_or(|p| p.is_empty() || p == ":memory:"),
            )),
            // `js_sqlite_open` always opens read-write (`Connection::open`),
            // and no flag here can request otherwise, so this is false by
            // construction. It needs revisiting if open flags are honoured.
            "readonly" => bits(JsValue::from_bool(false)),
            _ if DB_METHODS.contains(&name) => js_class_method_bind(
                bits(JsValue::from_handle(handle as u64)),
                name.as_ptr(),
                name.len(),
            ),
            _ => return None,
        });
    }
    if let Some(stmt) = get_handle::<SqliteStmtHandle>(handle) {
        return Some(match name {
            // Already stored: this is the statement's own SQL text.
            "source" => bits(JsValue::from_string_ptr(
                alloc_string(&stmt.sql).as_raw() as *mut StringHeader,
            )),
            // `reader` is "this statement returns rows"; `readonly` is "it
            // writes nothing". Both come from a fresh prepare, since the
            // handle stores only the SQL.
            "reader" | "readonly" => {
                let db = get_handle::<SqliteDbHandle>(stmt.db_handle)?;
                let conn = db.conn.lock().ok()?;
                let prepared = conn.prepare(&stmt.sql).ok()?;
                bits(JsValue::from_bool(if name == "reader" {
                    prepared.column_count() > 0
                } else {
                    prepared.readonly()
                }))
            }
            // No statement is left stepping between calls here, so a
            // statement is never busy once a call has returned.
            "busy" => bits(JsValue::from_bool(false)),
            "database" => bits(JsValue::from_handle(stmt.db_handle as u64)),
            _ if STMT_METHODS.contains(&name) => js_class_method_bind(
                bits(JsValue::from_handle(handle as u64)),
                name.as_ptr(),
                name.len(),
            ),
            _ => return None,
        });
    }
    None
}

unsafe extern "C" fn sqlite_property_dispatch_ext(
    handle: i64,
    name_ptr: *const u8,
    name_len: usize,
    out: *mut f64,
) -> i32 {
    if handle <= 0 || name_ptr.is_null() || name_len == 0 || out.is_null() {
        return 0;
    }
    let Ok(name) = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)) else {
        return 0;
    };
    match property_of(handle as Handle, name) {
        Some(value) => {
            *out = value;
            1
        }
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dispatch(handle: i64, name: &[u8]) -> i32 {
        let mut out = 0.0;
        unsafe {
            sqlite_method_dispatch_ext(
                handle,
                name.as_ptr(),
                name.len(),
                std::ptr::null(),
                0,
                &mut out,
            )
        }
    }

    #[test]
    fn unknown_methods_and_foreign_handles_are_unclaimed() {
        let path = alloc_string(":memory:");
        let db = unsafe { crate::js_sqlite_open(path.as_raw() as *const _) };
        assert_eq!(dispatch(db, b"nope"), 0);
        let foreign = perry_ffi::register_handle(String::from("another registry type"));
        assert_eq!(dispatch(foreign, b"exec"), 0);
    }

    #[test]
    fn database_close_is_claimed() {
        let path = alloc_string(":memory:");
        let db = unsafe { crate::js_sqlite_open(path.as_raw() as *const _) };
        assert_eq!(dispatch(db, b"close"), 1);
        assert!(get_handle::<SqliteDbHandle>(db).is_none());
    }
}
