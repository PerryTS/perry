//! JS-ABI entry points for the `perry/container`, `perry/compose` and
//! `perry/workloads` calls whose TypeScript shape does not map 1:1 onto a C
//! FFI signature (#11211).
//!
//! Codegen's native dispatch table can hand a runtime function a NaN-boxed
//! value (`NA_F64`) or a string pointer (`NA_STR`, which JSON-stringifies
//! objects and arrays), but it has no way to express the `i32` flags the
//! `js_container_*` exports take (`force`, `all`, `timeout`, `tail`), nor to
//! split an options object (`{ tail }`, `{ env, workdir }`, `{ service, tail }`)
//! into separate FFI parameters. The functions here take every argument as the
//! NaN-boxed `f64` the program passed — `undefined` for an omitted optional
//! argument — apply the documented defaults from `types/perry/*/index.d.ts`,
//! and forward to the existing exports, which stay the single implementation.
//!
//! Every string pointer handed on is rooted in a [`RuntimeHandleScope`] for
//! the duration of the forwarded call: the forwarded exports allocate their
//! promise before they read their string arguments.

use super::*;
use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::{js_promise_new_cross_thread, JSValue, Promise, StringHeader};

fn is_nullish(value: f64) -> bool {
    let v = JSValue::from_bits(value.to_bits());
    v.is_undefined() || v.is_null()
}

/// A string (or JSON-stringified object/array) argument as a rooted
/// `StringHeader` pointer; `undefined`/`null` become a null pointer, which the
/// forwarded exports read as "absent".
unsafe fn str_arg(scope: &RuntimeHandleScope, value: f64) -> *const StringHeader {
    if is_nullish(value) {
        return std::ptr::null();
    }
    let ptr = perry_runtime::value::js_value_to_str_ptr_for_ffi(value) as *const StringHeader;
    if !ptr.is_null() {
        scope.root_string_ptr(ptr);
    }
    ptr
}

/// An owned Rust string holding a JS string argument, or the JSON encoding of
/// an object/array argument; `undefined`/`null` → `None`.
unsafe fn owned_arg(value: f64) -> Option<String> {
    if is_nullish(value) {
        return None;
    }
    let ptr = perry_runtime::value::js_value_to_str_ptr_for_ffi(value) as *const StringHeader;
    string_from_header(ptr)
}

/// Materialise `s` as a JS string rooted in `scope`; `None` → null pointer.
unsafe fn rooted_str(scope: &RuntimeHandleScope, s: Option<&str>) -> *const StringHeader {
    match s {
        None => std::ptr::null(),
        Some(s) => {
            let ptr = string_to_js(s);
            scope.root_string_ptr(ptr);
            ptr
        }
    }
}

/// A JS number argument, or `None` when it is absent or not a finite number.
fn number_arg(value: f64) -> Option<f64> {
    let v = JSValue::from_bits(value.to_bits());
    let n = if v.is_int32() {
        v.as_int32() as f64
    } else if v.is_number() {
        v.as_number()
    } else {
        return None;
    };
    n.is_finite().then_some(n)
}

/// An integer argument for the `i32` FFI slots, with `-1` ("no value") for an
/// absent or negative one.
fn count_arg(value: f64) -> i32 {
    match number_arg(value) {
        Some(n) if n >= 0.0 => n.min(i32::MAX as f64) as i32,
        _ => -1,
    }
}

/// A boolean flag argument (`force`, `all`): JS truthiness, absent → false.
fn flag_arg(value: f64) -> i32 {
    if is_nullish(value) {
        return 0;
    }
    (perry_runtime::value::js_is_truthy(value) != 0) as i32
}

/// Parse an options-object argument into a JSON value (`Null` when absent).
unsafe fn options_arg(value: f64) -> serde_json::Value {
    owned_arg(value)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}

// ============ perry/container ============

/// `stop(id: string, timeout?: number): Promise<void>` — an omitted timeout
/// leaves the backend's own default (docker: 10 s) in place.
#[no_mangle]
pub unsafe extern "C" fn js_container_api_stop(id: f64, timeout: f64) -> *mut Promise {
    let scope = RuntimeHandleScope::new();
    js_container_stop(str_arg(&scope, id), count_arg(timeout))
}

/// `remove(id: string, force?: boolean): Promise<void>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_remove(id: f64, force: f64) -> *mut Promise {
    let scope = RuntimeHandleScope::new();
    js_container_remove(str_arg(&scope, id), flag_arg(force))
}

/// `list(all?: boolean): Promise<string>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_list(all: f64) -> *mut Promise {
    js_container_list(flag_arg(all))
}

/// `logs(id: string, options?: { tail?: number }): Promise<string>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_logs(id: f64, options: f64) -> *mut Promise {
    let tail = match options_arg(options).get("tail").and_then(|t| t.as_f64()) {
        Some(t) if t.is_finite() && t >= 0.0 => t.min(i32::MAX as f64) as i32,
        _ => -1,
    };
    let scope = RuntimeHandleScope::new();
    js_container_logs(str_arg(&scope, id), tail)
}

/// `exec(id: string, cmd: string[], options?: { env?, workdir? }): Promise<string>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_exec(id: f64, cmd: f64, options: f64) -> *mut Promise {
    let opts = options_arg(options);
    let env_json = opts
        .get("env")
        .filter(|e| e.is_object())
        .map(|e| e.to_string());
    let workdir = opts
        .get("workdir")
        .and_then(|w| w.as_str())
        .map(str::to_string);
    let cmd_json = owned_arg(cmd);
    let scope = RuntimeHandleScope::new();
    let id_ptr = str_arg(&scope, id);
    js_container_exec(
        id_ptr,
        rooted_str(&scope, cmd_json.as_deref()),
        rooted_str(&scope, env_json.as_deref()),
        rooted_str(&scope, workdir.as_deref()),
    )
}

/// `removeImage(reference: string, force?: boolean): Promise<void>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_removeImage(reference: f64, force: f64) -> *mut Promise {
    let scope = RuntimeHandleScope::new();
    js_container_removeImage(str_arg(&scope, reference), flag_arg(force))
}

/// `removeIfExists(id: string, force?: boolean): Promise<string>`
#[no_mangle]
pub unsafe extern "C" fn js_container_api_removeIfExists(id: f64, force: f64) -> *mut Promise {
    let scope = RuntimeHandleScope::new();
    js_container_removeIfExists(str_arg(&scope, id), flag_arg(force))
}

// ============ perry/compose ============

/// `logs(handle, options?: { service?: string; tail?: number }): Promise<string>`
#[no_mangle]
pub unsafe extern "C" fn js_compose_api_logs(handle: f64, options: f64) -> *mut Promise {
    let opts = options_arg(options);
    let service = opts
        .get("service")
        .and_then(|s| s.as_str())
        .map(str::to_string);
    let tail = opts
        .get("tail")
        .and_then(|t| t.as_f64())
        .filter(|t| t.is_finite() && *t >= 0.0)
        .unwrap_or(-1.0);
    let scope = RuntimeHandleScope::new();
    js_container_compose_logs(handle, rooted_str(&scope, service.as_deref()), tail)
}

// ============ perry/workloads ============

/// `inspectGraph(graph): Promise<string>` — resolves with a JSON-encoded
/// `GraphStatus`.
///
/// Given the JSON string `graph()` returns, it inspects that graph WITHOUT
/// starting any node (as `types/perry/workloads/index.d.ts` documents): every
/// node reads `"pending"`. Given the handle `runGraph()` resolved with, it
/// reports the live status of that running graph.
#[no_mangle]
pub unsafe extern "C" fn js_workload_api_inspectGraph(graph: f64) -> *mut Promise {
    let v = JSValue::from_bits(graph.to_bits());
    if !v.is_string() && !v.is_short_string() {
        return js_workload_inspectGraph(handle_id_from_f64(graph));
    }
    let graph_json = owned_arg(graph).unwrap_or_default();
    let promise = js_promise_new_cross_thread();
    crate::container::executor::spawn_for_promise_deferred(
        promise as *mut u8,
        async move {
            let graph: perry_container_compose::WorkloadGraph =
                serde_json::from_str(&graph_json)
                    .map_err(|e| format!("Failed to parse graph: {}", e))?;
            let backend = get_global_backend().await.map_err(|e| e.to_string())?;
            let engine =
                perry_container_compose::WorkloadGraphEngine::new(graph, Arc::clone(backend));
            let status = engine.status().await.map_err(|e| e.to_string())?;
            serde_json::to_string(&status).map_err(|e| e.to_string())
        },
        |json| {
            let str_ptr = perry_runtime::js_string_from_bytes(json.as_ptr(), json.len() as u32);
            JSValue::string_ptr(str_ptr).bits()
        },
    );
    promise
}
