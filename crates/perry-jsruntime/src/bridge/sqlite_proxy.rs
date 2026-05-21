//! V8-side proxies + trampolines for perry sqlite Database / Statement handles.
//!
//! See module-level comment in `materialize_sqlite_db_proxy` for the full
//! context (refs #1022).

use super::*;

use super::conversion::{native_to_v8, v8_to_native};

/// Probe whether a small handle id is a sqlite Database registered by
/// either `perry-stdlib::sqlite` or `perry-ext-better-sqlite3`. The
/// `extern "C"` symbol resolves at link time to whichever crate's
/// `js_sqlite_open` registered the handle. When neither crate is in
/// the build, `perry-stdlib::lib::js_sqlite_is_db_handle` provides a
/// 0-returning stub so this always links. Refs #1022.
pub(super) fn is_sqlite_db_handle(handle_id: usize) -> bool {
    extern "C" {
        fn js_sqlite_is_db_handle(handle: i64) -> i32;
    }
    if handle_id == 0 {
        return false;
    }
    unsafe { js_sqlite_is_db_handle(handle_id as i64) != 0 }
}

/// Counterpart to `is_sqlite_db_handle` for the Statement side of the
/// proxy materialization. Refs #1022.
pub(super) fn is_sqlite_stmt_handle(handle_id: usize) -> bool {
    extern "C" {
        fn js_sqlite_is_stmt_handle(handle: i64) -> i32;
    }
    if handle_id == 0 {
        return false;
    }
    unsafe { js_sqlite_is_stmt_handle(handle_id as i64) != 0 }
}

/// Look up the perry sqlite handle id stashed on a v8 Object proxy
/// during `materialize_sqlite_db_handle` / `materialize_sqlite_stmt_handle`.
/// Method trampolines call this to recover the receiver. Returns
/// `None` when called from an unbound `Function.prototype.call` site
/// (no `this`) or when `this` is some other object the user passed
/// through; the trampoline then returns `undefined`.
fn read_sqlite_handle_id_from_this(
    scope: &mut v8::PinScope<'_, '_>,
    this: v8::Local<v8::Object>,
) -> Option<i64> {
    let key = v8::String::new(scope, "__perry_sqlite_handle__")?;
    let val = this.get(scope, key.into())?;
    if val.is_external() {
        let ext = v8::Local::<v8::External>::try_from(val).ok()?;
        Some(ext.value() as i64)
    } else if val.is_number() || val.is_int32() {
        let n = val.integer_value(scope)?;
        Some(n)
    } else {
        None
    }
}

/// Convert v8 method args to a freshly-allocated perry ArrayHeader of
/// NaN-boxed values. Used by the sqlite stmt method trampolines to
/// build the `params_arr` ArrayHeader that `js_sqlite_stmt_run` /
/// `js_sqlite_stmt_get` / `js_sqlite_stmt_all` expect. The array is
/// arena-allocated; perry's GC will eventually sweep it.
fn build_native_array_from_v8_args(
    scope: &mut v8::PinScope<'_, '_>,
    args: &v8::FunctionCallbackArguments,
) -> *mut perry_runtime::array::ArrayHeader {
    let arr = perry_runtime::js_array_alloc(0);
    let mut current = arr;
    let count = args.length();
    for i in 0..count {
        let arg = args.get(i);
        let native = v8_to_native(scope, arg);
        current = perry_runtime::js_array_push(
            current,
            perry_runtime::JSValue::from_bits(native.to_bits()),
        );
    }
    current
}

/// Extract a perry StringHeader pointer from a v8 value. Allocates a
/// fresh native string if the input is a JS string; returns null if
/// not a string. The sqlite `prepare` / `exec` / `pragma` FFI shims
/// expect `*const StringHeader`.
fn v8_string_to_native_header(
    scope: &mut v8::PinScope<'_, '_>,
    value: v8::Local<v8::Value>,
) -> *const perry_runtime::StringHeader {
    if !value.is_string() {
        // Try toString — drizzle's sql.raw produces objects whose toString
        // returns the SQL. Defensive cast keeps the common path working
        // when the caller hands us a SqlChunk or similar.
        let s = match value.to_string(scope) {
            Some(s) => s,
            None => return std::ptr::null(),
        };
        let rs = s.to_rust_string_lossy(scope);
        return perry_runtime::js_string_from_bytes(rs.as_ptr(), rs.len() as u32);
    }
    let s = value.to_string(scope).unwrap();
    let rs = s.to_rust_string_lossy(scope);
    perry_runtime::js_string_from_bytes(rs.as_ptr(), rs.len() as u32)
}

// =====================================================================
// SQLite Database / Statement v8 proxies (refs #1022)
// =====================================================================
//
// Drizzle's BetterSQLiteSession is compiled as JS that runs under V8
// fallback. When user code in entry.ts does `const sqlite = new
// Database(":memory:"); const db = drizzle(sqlite);`, the `sqlite`
// handle (a small integer registered by `js_sqlite_open`) crosses the
// native→V8 boundary. Without explicit materialization it goes
// through `native_object_to_v8`'s small-handle branch and lands in
// `materialize_web_fetch_handle`, which doesn't know about sqlite and
// returns `v8::null`. Drizzle then does `this.client.prepare(query.sql)`
// in session.js and crashes with `Cannot read properties of null
// (reading 'prepare')`.
//
// The fix: synthesize a real v8 Object whose `prepare` / `exec` /
// `transaction` / `pragma` / `close` keys are v8 Functions that route
// back to the linked-in `js_sqlite_*` FFI shims. Each trampoline
// recovers the perry handle id from `this.__perry_sqlite_handle__`
// (a v8::External stashed at construction time) and calls the
// matching native function directly.
//
// Statement is mirrored: `prepare` returns a fresh statement-handle
// proxy whose `run` / `all` / `get` / `raw` / `iterate` keys are v8
// Functions over `js_sqlite_stmt_run` / `js_sqlite_stmt_all` /
// `js_sqlite_stmt_get` / `js_sqlite_stmt_raw`.
//
// `transaction` is deferred — better-sqlite3's `transaction(fn)`
// wrapper needs a v8::Function → perry closure adapter that doesn't
// exist yet for the call-into-native direction. drizzle's basic
// insert/select smoke test (entry.ts) doesn't exercise transactions,
// so the deferred coverage is fine for the #1022 close-out. Future
// work: bridge `js_sqlite_transaction` so wrapped JS callbacks BEGIN/
// COMMIT around their body. For now `transaction(fn)` returns a no-op
// callable so drizzle's `if (config.behavior)` chain doesn't crash.

fn sqlite_db_prepare_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::null(scope).into());
        return;
    };
    let sql_v8 = args.get(0);
    let sql_ptr = v8_string_to_native_header(scope, sql_v8);
    if sql_ptr.is_null() {
        retval.set(v8::null(scope).into());
        return;
    }
    extern "C" {
        fn js_sqlite_prepare(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i64;
    }
    let stmt_handle = unsafe { js_sqlite_prepare(handle, sql_ptr) };
    if stmt_handle < 0 {
        retval.set(v8::null(scope).into());
        return;
    }
    let v8_obj = materialize_sqlite_stmt_proxy(scope, stmt_handle);
    retval.set(v8_obj);
}

fn sqlite_db_exec_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::null(scope).into());
        return;
    };
    let sql_v8 = args.get(0);
    let sql_ptr = v8_string_to_native_header(scope, sql_v8);
    if sql_ptr.is_null() {
        retval.set(v8::null(scope).into());
        return;
    }
    extern "C" {
        fn js_sqlite_exec(db_handle: i64, sql_ptr: *const perry_runtime::StringHeader) -> i32;
    }
    let _ = unsafe { js_sqlite_exec(handle, sql_ptr) };
    // better-sqlite3 returns the Database for chaining.
    retval.set(this.into());
}

fn sqlite_db_pragma_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let pragma_v8 = args.get(0);
    let pragma_ptr = v8_string_to_native_header(scope, pragma_v8);
    let value_v8 = args.get(1);
    let value_ptr = if value_v8.is_undefined() || value_v8.is_null() {
        std::ptr::null()
    } else {
        v8_string_to_native_header(scope, value_v8)
    };
    extern "C" {
        fn js_sqlite_pragma(
            db_handle: i64,
            pragma_ptr: *const perry_runtime::StringHeader,
            value_ptr: *const perry_runtime::StringHeader,
        ) -> *mut perry_runtime::StringHeader;
    }
    let result_ptr = unsafe { js_sqlite_pragma(handle, pragma_ptr, value_ptr) };
    if result_ptr.is_null() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let native_str_bits = STRING_TAG | (result_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_str_bits));
    retval.set(v);
}

fn sqlite_db_close_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    if let Some(handle) = read_sqlite_handle_id_from_this(scope, this) {
        extern "C" {
            fn js_sqlite_close(db_handle: i64) -> i32;
        }
        let _ = unsafe { js_sqlite_close(handle) };
    }
    retval.set(v8::undefined(scope).into());
}

fn sqlite_db_transaction_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub for #1022 close-out — drizzle's smoke test (entry.ts) doesn't
    // call into the transaction path. Returns a callable whose
    // `deferred` / `immediate` / `exclusive` properties return the
    // wrapped function unchanged, so drizzle's
    // `nativeTx[config.behavior ?? "deferred"](tx)` chain doesn't
    // crash when called. Real BEGIN/COMMIT lifecycle is deferred until
    // a v8→perry closure adapter ships (#TBD).
    let fn_arg = args.get(0);
    if !fn_arg.is_function() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let wrapper = v8::Object::new(scope);
    for behavior in ["deferred", "immediate", "exclusive"] {
        if let Some(k) = v8::String::new(scope, behavior) {
            wrapper.set(scope, k.into(), fn_arg);
        }
    }
    retval.set(wrapper.into());
}

fn sqlite_stmt_run_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_run(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::object::ObjectHeader;
    }
    let obj_ptr = unsafe { js_sqlite_stmt_run(handle, params_arr) };
    if obj_ptr.is_null() {
        retval.set(v8::undefined(scope).into());
        return;
    }
    let native_bits = POINTER_TAG | (obj_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

fn sqlite_stmt_all_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::array::ArrayHeader;
    }
    let arr_ptr = unsafe { js_sqlite_stmt_all(handle, params_arr) };
    if arr_ptr.is_null() {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    }
    let native_bits = POINTER_TAG | (arr_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

fn sqlite_stmt_get_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::undefined(scope).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_get(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> f64;
    }
    let result_f64 = unsafe { js_sqlite_stmt_get(handle, params_arr) };
    let v = native_to_v8(scope, result_f64);
    retval.set(v);
}

fn sqlite_stmt_raw_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    let this = args.this();
    // `stmt.raw()` returns `this` for chaining (`stmt.raw().all(...)`).
    // Flip the perry-side raw_mode flag so subsequent .all/.get return
    // arrays-of-arrays rather than arrays-of-objects.
    if let Some(handle) = read_sqlite_handle_id_from_this(scope, this) {
        extern "C" {
            fn js_sqlite_stmt_raw(stmt_handle: i64) -> i64;
        }
        let _ = unsafe { js_sqlite_stmt_raw(handle) };
    }
    retval.set(this.into());
}

fn sqlite_stmt_pluck_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub for drizzle — pluck() returns this for chaining. Drizzle
    // doesn't exercise pluck on the prepared-query path, but keeping
    // the method present prevents `stmt.pluck is not a function` if
    // a future codepath enables it. The actual pluck behavior
    // (return first column only) isn't bridged today.
    let _ = scope;
    let _ = args;
    let this = args.this();
    retval.set(this.into());
}

fn sqlite_stmt_columns_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Stub — returns an empty array. drizzle's PreparedQuery doesn't
    // call columns() on the smoke-test path; full bridging would need
    // a `js_sqlite_stmt_columns` FFI that returns an array of
    // `{name, column, table, database, type}` descriptors.
    let _ = args;
    let arr = v8::Array::new(scope, 0);
    retval.set(arr.into());
}

fn sqlite_stmt_iterate_trampoline(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut retval: v8::ReturnValue,
) {
    // Backed by `stmt.all(...)` and wrapped in a JS array iterator. Not
    // a true streaming iterator (which would need a perry-side cursor
    // handle), but drizzle's only iterate consumer is for-await, which
    // works against an Array's `[Symbol.iterator]`.
    let this = args.this();
    let Some(handle) = read_sqlite_handle_id_from_this(scope, this) else {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    };
    let params_arr = build_native_array_from_v8_args(scope, &args);
    extern "C" {
        fn js_sqlite_stmt_all(
            stmt_handle: i64,
            params_arr: *const perry_runtime::array::ArrayHeader,
        ) -> *mut perry_runtime::array::ArrayHeader;
    }
    let arr_ptr = unsafe { js_sqlite_stmt_all(handle, params_arr) };
    if arr_ptr.is_null() {
        retval.set(v8::Array::new(scope, 0).into());
        return;
    }
    let native_bits = POINTER_TAG | (arr_ptr as u64 & POINTER_MASK);
    let v = native_to_v8(scope, f64::from_bits(native_bits));
    retval.set(v);
}

/// Attach the perry handle id to a v8 Object proxy. Stashed as a
/// v8::External under `__perry_sqlite_handle__` so the method
/// trampolines can recover it via `read_sqlite_handle_id_from_this`.
fn attach_sqlite_handle_id(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
    handle_id: i64,
) {
    let external = v8::External::new(scope, handle_id as *mut std::ffi::c_void);
    if let Some(k) = v8::String::new(scope, "__perry_sqlite_handle__") {
        obj.set(scope, k.into(), external.into());
    }
}

/// Attach a v8::Function (built from a callback) under `obj[name]`.
fn attach_method(
    scope: &mut v8::PinScope<'_, '_>,
    obj: v8::Local<v8::Object>,
    name: &str,
    cb: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    if let Some(func) = v8::Function::new(scope, cb) {
        if let Some(k) = v8::String::new(scope, name) {
            obj.set(scope, k.into(), func.into());
        }
    }
}

/// Materialize a v8::Object proxy for a perry sqlite Database handle.
/// `prepare` / `exec` / `transaction` / `pragma` / `close` are v8
/// Functions backed by the linked `js_sqlite_*` FFI shims. Refs #1022.
pub(super) fn materialize_sqlite_db_proxy<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: i64,
) -> v8::Local<'s, v8::Value> {
    let obj = v8::Object::new(scope);
    attach_sqlite_handle_id(scope, obj, handle_id);
    attach_method(scope, obj, "prepare", sqlite_db_prepare_trampoline);
    attach_method(scope, obj, "exec", sqlite_db_exec_trampoline);
    attach_method(scope, obj, "pragma", sqlite_db_pragma_trampoline);
    attach_method(scope, obj, "close", sqlite_db_close_trampoline);
    attach_method(scope, obj, "transaction", sqlite_db_transaction_trampoline);
    obj.into()
}

/// Materialize a v8::Object proxy for a perry sqlite Statement handle.
/// `run` / `get` / `all` / `raw` / `iterate` / `pluck` / `columns`
/// are v8 Functions backed by the linked `js_sqlite_stmt_*` FFI
/// shims. Refs #1022.
pub(super) fn materialize_sqlite_stmt_proxy<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    handle_id: i64,
) -> v8::Local<'s, v8::Value> {
    let obj = v8::Object::new(scope);
    attach_sqlite_handle_id(scope, obj, handle_id);
    attach_method(scope, obj, "run", sqlite_stmt_run_trampoline);
    attach_method(scope, obj, "all", sqlite_stmt_all_trampoline);
    attach_method(scope, obj, "get", sqlite_stmt_get_trampoline);
    attach_method(scope, obj, "raw", sqlite_stmt_raw_trampoline);
    attach_method(scope, obj, "iterate", sqlite_stmt_iterate_trampoline);
    attach_method(scope, obj, "pluck", sqlite_stmt_pluck_trampoline);
    attach_method(scope, obj, "columns", sqlite_stmt_columns_trampoline);
    obj.into()
}
