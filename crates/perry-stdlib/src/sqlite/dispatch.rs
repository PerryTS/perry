//! Handle method/property dispatch for the registry-backed `bun:sqlite`
//! `Database` and `Statement` handles. `node:sqlite` objects are ordinary
//! objects whose methods live on their prototypes.

use super::*;
use crate::common::{get_handle, Handle};
use perry_runtime::{js_array_alloc, js_array_push_f64, js_nanbox_pointer, ArrayHeader, JSValue};

/// `js_class_method_bind` retains the name pointer in the closure, so make a
/// non-static forwarded property slice impossible to pass accidentally.
unsafe fn bind_static_handle_method(handle: Handle, method: &'static [u8]) -> f64 {
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(js_nanbox_pointer(handle), method.as_ptr(), method.len())
}

fn database_method_name_static(property: &str) -> Option<&'static [u8]> {
    match property {
        "close" => Some(b"close"),
        "exec" => Some(b"exec"),
        "prepare" => Some(b"prepare"),
        "query" => Some(b"query"),
        "run" => Some(b"run"),
        "transaction" => Some(b"transaction"),
        "serialize" => Some(b"serialize"),
        "deserialize" => Some(b"deserialize"),
        "loadExtension" => Some(b"loadExtension"),
        "__perry_dispose__" => Some(b"__perry_dispose__"),
        "@@__perry_wk_dispose" => Some(b"@@__perry_wk_dispose"),
        _ => None,
    }
}

fn statement_method_name_static(property: &str) -> Option<&'static [u8]> {
    match property {
        "run" => Some(b"run"),
        "get" => Some(b"get"),
        "all" => Some(b"all"),
        "values" => Some(b"values"),
        "iterate" => Some(b"iterate"),
        "columns" => Some(b"columns"),
        "safeIntegers" => Some(b"safeIntegers"),
        "finalize" => Some(b"finalize"),
        _ => None,
    }
}

pub(crate) unsafe fn packed_args_array(args: &[f64]) -> *mut ArrayHeader {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let arr = scope.root_raw_mut_ptr(js_array_alloc(args.len() as u32));
    for value in args {
        let next = js_array_push_f64(arr.get_raw_mut_ptr(), *value);
        arr.set_raw_mut_ptr(next);
    }
    arr.get_raw_mut_ptr::<ArrayHeader>()
}

pub unsafe fn dispatch_bun_sqlite_database_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_bun_sqlite_is_database_handle(handle) == 0 {
        return None;
    }
    let arg0 = args.first().copied().unwrap_or_else(undefined_f64);
    match method {
        "close" => {
            js_bun_sqlite_database_close(handle);
            Some(undefined_f64())
        }
        "__perry_dispose__" | "@@__perry_wk_dispose" => {
            bun_sqlite_database_dispose(handle);
            Some(undefined_f64())
        }
        "exec" => {
            bun_sqlite_database_exec(handle, arg0);
            Some(undefined_f64())
        }
        "prepare" | "query" => Some(js_nanbox_pointer(js_bun_sqlite_database_query(
            handle, arg0,
        ))),
        "run" => {
            let params = packed_args_array(args.get(1..).unwrap_or_default());
            Some(js_nanbox_pointer(
                js_bun_sqlite_database_run(handle, arg0, params) as i64,
            ))
        }
        "transaction" => Some(js_nanbox_pointer(
            js_bun_sqlite_database_transaction(handle, arg0) as i64,
        )),
        "serialize" => Some(js_nanbox_pointer(
            js_bun_sqlite_database_serialize(handle, arg0) as i64,
        )),
        "deserialize" => {
            bun_sqlite_database_deserialize(handle, arg0);
            Some(undefined_f64())
        }
        "loadExtension" => {
            js_bun_sqlite_database_load_extension(handle, arg0);
            Some(undefined_f64())
        }
        _ => None,
    }
}

pub unsafe fn dispatch_bun_sqlite_database_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_bun_sqlite_is_database_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "filename" => Some(f64_from_jsvalue(JSValue::string_ptr(
            js_bun_sqlite_database_filename(handle),
        ))),
        "inTransaction" => Some(bun_sqlite_database_in_transaction(handle)),
        "isOpen" => Some(bun_sqlite_database_is_open(handle)),
        _ => Some(bind_static_handle_method(
            handle,
            database_method_name_static(property_name)?,
        )),
    }
}

pub unsafe fn dispatch_bun_sqlite_statement_method(
    handle: Handle,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if js_bun_sqlite_is_statement_handle(handle) == 0 {
        return None;
    }
    let args_arr = packed_args_array(args);
    match method {
        "run" => Some(js_nanbox_pointer(
            js_bun_sqlite_statement_run(handle, args_arr) as i64,
        )),
        "get" => Some(js_bun_sqlite_statement_get(handle, args_arr)),
        "all" => Some(js_nanbox_pointer(
            js_bun_sqlite_statement_all(handle, args_arr) as i64,
        )),
        "values" => Some(js_nanbox_pointer(
            js_bun_sqlite_statement_values(handle, args_arr) as i64,
        )),
        "safeIntegers" => Some(js_bun_sqlite_statement_safe_integers(
            handle,
            args.first().copied().unwrap_or_else(undefined_f64),
        )),
        "finalize" => {
            js_bun_sqlite_statement_finalize(handle);
            Some(undefined_f64())
        }
        "iterate" => Some(bun_sqlite_statement_iterate(handle, args_arr)),
        "columns" => Some(js_nanbox_pointer(
            bun_sqlite_statement_columns(handle) as i64
        )),
        _ => None,
    }
}

pub unsafe fn dispatch_bun_sqlite_statement_property(
    handle: Handle,
    property_name: &str,
) -> Option<f64> {
    if js_bun_sqlite_is_statement_handle(handle) == 0 {
        return None;
    }
    match property_name {
        "source" => Some(f64_from_jsvalue(JSValue::string_ptr(
            bun_sqlite_statement_source(handle),
        ))),
        _ => Some(bind_static_handle_method(
            handle,
            statement_method_name_static(property_name)?,
        )),
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_is_database_handle(handle: Handle) -> i32 {
    i32::from(get_handle::<BunSqliteDbHandle>(handle).is_some())
}

#[no_mangle]
pub unsafe extern "C" fn js_bun_sqlite_is_statement_handle(handle: Handle) -> i32 {
    i32::from(get_handle::<BunSqliteStmtHandle>(handle).is_some())
}

#[cfg(test)]
mod static_method_name_tests {
    use super::*;

    fn assert_static_lookup(lookup: fn(&str) -> Option<&'static [u8]>, names: &[&str]) {
        for name in names {
            let owned = (*name).to_owned();
            let found = lookup(&owned).expect("known method must resolve");
            assert_eq!(found, name.as_bytes());
            assert_ne!(
                found.as_ptr(),
                owned.as_ptr(),
                "lookup borrowed the forwarded property name for {name}"
            );
            assert_eq!(
                found.as_ptr(),
                lookup(name).unwrap().as_ptr(),
                "lookup must always return the same static literal for {name}"
            );
        }
        assert!(lookup("notASqliteMethod").is_none());
    }

    #[test]
    fn sqlite_method_name_lookups_return_static_literals() {
        assert_static_lookup(
            database_method_name_static,
            &[
                "close",
                "exec",
                "prepare",
                "query",
                "run",
                "transaction",
                "serialize",
                "deserialize",
                "loadExtension",
                "__perry_dispose__",
                "@@__perry_wk_dispose",
            ],
        );
        assert_static_lookup(
            statement_method_name_static,
            &[
                "run",
                "get",
                "all",
                "values",
                "iterate",
                "columns",
                "safeIntegers",
                "finalize",
            ],
        );
    }
}
