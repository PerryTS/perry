//! `node:sqlite` `SQLTagStore` (`db.createTagStore()`): an ordinary object
//! that owns a native payload (#11919). The payload is plain data: an LRU of
//! the SQL texts it has compiled, each stamped with the database open it
//! belongs to. Running a template compiles it on the database (statements own
//! no C resource), so nothing here outlives a `close()`.

use super::*;
use perry_runtime::closure::{ClosureHeader, JsThis};
use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::native_class_ids::SQLITE_TAG_STORE;
use perry_runtime::native_payload::{
    self, NativePayloadFamily, OpenSerial, PayloadMiss, PayloadPrototype,
};
use perry_runtime::{js_array_get, js_array_is_array, js_array_length, ArrayHeader, JSValue};
use std::collections::VecDeque;
use std::ffi::CString;

perry_runtime::state_key_memo!(static MEMO_DB);
perry_runtime::birth_memo!(static TAG_STORE_BIRTH);

macro_rules! builtin {
    ($body:path, $n:tt) => {
        perry_runtime::fn_info!($body, $n; with_declared($n), with_flags(perry_runtime::closure::FN_BUILTIN))
    };
}

macro_rules! builtin_rest {
    ($body:path) => {
        perry_runtime::fn_info!(
            $body, 1;
            with_rest(0),
            with_declared(0),
            with_flags(perry_runtime::closure::FN_BUILTIN)
        )
    };
}

pub(crate) static TAG_STORE_FAMILY: NativePayloadFamily = NativePayloadFamily {
    class_id: SQLITE_TAG_STORE,
    links_owner: false,
    name: "SQLTagStore",
    constructor_export: None,
    constructor_length: 0,
    install_prototype: install_tag_store_prototype,
};

pub(crate) struct NodeTagStore {
    capacity: usize,
    /// Least recently used first.
    cache: VecDeque<(String, OpenSerial)>,
}

impl NodeTagStore {
    /// Record a use of `sql` on open `serial`; a stale entry is replaced.
    fn touch(&mut self, sql: &str, serial: OpenSerial) {
        if self.capacity == 0 {
            return;
        }
        self.cache.retain(|(cached, _)| cached != sql);
        self.cache.push_back((sql.to_string(), serial));
        while self.cache.len() > self.capacity {
            self.cache.pop_front();
        }
    }
}

pub(crate) fn node_sqlite_tag_store_capacity(value: f64) -> usize {
    let js = value_from_f64(value);
    let number = if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() {
        js.as_number()
    } else {
        return 1000;
    };
    if !number.is_finite() {
        return if number.is_sign_positive() {
            i32::MAX as usize
        } else {
            0
        };
    }
    let truncated = number.trunc();
    if truncated <= 0.0 {
        0
    } else if truncated >= i32::MAX as f64 {
        i32::MAX as usize
    } else {
        truncated as usize
    }
}

/// `db.createTagStore(maxSize)`.
pub(crate) unsafe fn new_tag_store(db: f64, capacity: usize) -> f64 {
    let scope = RuntimeHandleScope::new();
    let db = scope.root_nanbox_f64(db);
    db_payload(db.get_nanbox_f64());
    let store = scope.root_nanbox_f64(native_payload::alloc_with_state(
        &TAG_STORE_FAMILY,
        NodeTagStore {
            capacity,
            cache: VecDeque::new(),
        },
        std::mem::size_of::<NodeTagStore>(),
        &[],
        &[(b"db", db.get_nanbox_f64())],
        &[
            super::statement_sync::own_getter("capacity", builtin!(tag_store_capacity_getter, 0)),
            super::statement_sync::own_getter("db", builtin!(tag_store_db_getter, 0)),
            super::statement_sync::own_getter("size", builtin!(tag_store_size_getter, 0)),
        ],
        &TAG_STORE_BIRTH,
    ));
    store.get_nanbox_f64()
}

unsafe fn store_payload<'a>(this: f64) -> &'a mut NodeTagStore {
    match native_payload::payload_mut::<NodeTagStore>(this, &TAG_STORE_FAMILY) {
        Ok(store) => store,
        Err(PayloadMiss::Closed) => throw_invalid_state("SQLTagStore is not open"),
        Err(PayloadMiss::Foreign) => throw_illegal_invocation(),
    }
}

/// The SQL text of a tagged template (`?` between the parts) and its values.
unsafe fn template_args(args_arr: *const ArrayHeader) -> (String, Vec<f64>) {
    let args = node_args_from_array(args_arr);
    let strings_value = args.first().copied().unwrap_or_else(undefined_f64);
    let is_array = value_from_f64(js_array_is_array(strings_value));
    if !is_array.is_bool() || !is_array.as_bool() {
        throw_type("First argument must be an array of strings (template literal).");
    }
    let strings_ptr = raw_addr_from_value(strings_value) as *const ArrayHeader;
    if strings_ptr.is_null() {
        throw_type("First argument must be an array of strings (template literal).");
    }
    let strings_len = js_array_length(strings_ptr);
    let mut sql = String::new();
    for index in 0..strings_len {
        let Some(part) = string_key_from_js_value(js_array_get(strings_ptr, index)) else {
            throw_type("Template literal parts must be strings.");
        };
        sql.push_str(&part);
        if index + 1 < strings_len {
            sql.push('?');
        }
    }
    (sql, args.into_iter().skip(1).collect())
}

/// Compile the template on the store's database (recording it in the LRU)
/// and bind its values positionally.
macro_rules! with_template {
    ($this:expr, $rest:expr, |$stepper:ident| $body:expr) => {{
        let scope = RuntimeHandleScope::new();
        let this = scope.root_nanbox_f64($this);
        store_payload(this.get_nanbox_f64());
        let db = scope.root_nanbox_f64(native_payload::state_get(
            this.get_nanbox_f64(),
            &TAG_STORE_FAMILY,
            b"db",
        ));
        let rest = scope.root_nanbox_f64($rest);
        let (sql, values) =
            template_args(raw_addr_from_value(rest.get_nanbox_f64()) as *const ArrayHeader);
        let value_roots: Vec<_> = values.iter().map(|v| scope.root_nanbox_f64(*v)).collect();
        let (serial, flags) = {
            let db = db_payload(db.get_nanbox_f64());
            (db.serial, db.cfg.stmt_flags())
        };
        let Ok(c_sql) = CString::new(sql.clone()) else {
            throw_sqlite_error("SQL string must not contain null bytes");
        };
        let $stepper = Stepper::start(&db, &c_sql, flags, false, |raw_db, raw_stmt| {
            let values: Vec<f64> = value_roots.iter().map(|v| v.get_nanbox_f64()).collect();
            bind_node_sqlite_positional_params(raw_db, raw_stmt, &values)
        });
        store_payload(this.get_nanbox_f64()).touch(&sql, serial);
        $body
    }};
}

extern "C" fn tag_store_run_thunk(_c: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    unsafe { with_template!(this.as_f64(), rest, |stepper| run_to_completion(stepper)) }
}

extern "C" fn tag_store_get_thunk(_c: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    unsafe { with_template!(this.as_f64(), rest, |stepper| first_row(stepper)) }
}

extern "C" fn tag_store_all_thunk(_c: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    unsafe { with_template!(this.as_f64(), rest, |stepper| all_rows(stepper)) }
}

extern "C" fn tag_store_iterate_thunk(_c: *const ClosureHeader, this: JsThis, rest: f64) -> f64 {
    unsafe {
        let rows = with_template!(this.as_f64(), rest, |stepper| all_rows(stepper));
        perry_runtime::array::array_values_iter(rows)
    }
}

extern "C" fn tag_store_clear_thunk(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        store_payload(this.as_f64()).cache.clear();
    }
    undefined_f64()
}

extern "C" fn tag_store_size_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let size = store_payload(this.as_f64()).cache.len();
        f64_from_jsvalue(JSValue::number(size as f64))
    }
}

extern "C" fn tag_store_capacity_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        let capacity = store_payload(this.as_f64()).capacity;
        f64_from_jsvalue(JSValue::number(capacity as f64))
    }
}

extern "C" fn tag_store_db_getter(_c: *const ClosureHeader, this: JsThis) -> f64 {
    unsafe {
        store_payload(this.as_f64());
        native_payload::state_get_memo(this.as_f64(), &TAG_STORE_FAMILY, b"db", &MEMO_DB)
    }
}

fn install_tag_store_prototype(proto: &mut PayloadPrototype) {
    proto.method("run", builtin_rest!(tag_store_run_thunk), 0);
    proto.method("get", builtin_rest!(tag_store_get_thunk), 0);
    proto.method("all", builtin_rest!(tag_store_all_thunk), 0);
    proto.method("iterate", builtin_rest!(tag_store_iterate_thunk), 0);
    proto.method("clear", builtin!(tag_store_clear_thunk, 0), 0);
}
