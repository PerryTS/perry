//! Issue #841 — wire up named exports + namespace imports for five
//! Node.js submodules that Perry's manifest had registered but whose
//! FFI export tables defaulted to a `TAG_TRUE` sentinel cell:
//!
//!   - `node:timers/promises` (setTimeout / setImmediate / setInterval / scheduler.*)
//!   - `node:readline/promises` (createInterface, Interface, Readline)
//!   - `node:stream/promises` (pipeline, finished)
//!   - `node:stream/consumers` (text, json, buffer, arrayBuffer, bytes, blob)
//!   - `node:sys` (deprecated alias for node:util — re-exports format, inspect, etc.)
//!
//! Pre-fix `import { setTimeout } from "node:timers/promises"; typeof setTimeout`
//! reported `"boolean"` (the value was literally `true`) and `import * as ns
//! from "node:..."` errored at compile time with the "switch to named imports"
//! diagnostic. This module ships per-export function singletons whose `typeof`
//! is `"function"`, plus per-submodule namespace stubs whose properties point
//! at the same singletons.
//!
//! The thunks are deliberately minimal — they throw `Error("<api> is not yet
//! implemented in Perry")` when invoked. Full functional implementations of
//! these APIs are tracked separately under the #793 Node compatibility
//! roadmap. The fix here is strictly about restoring the import surface so
//! consuming code can at least introspect the bindings (typeof checks,
//! `=== util.format` comparisons, dynamic-shape introspection) without
//! tripping over `true`-as-a-function downstream errors.

use std::cell::RefCell;
use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::closure::{
    js_closure_alloc, js_closure_call0, js_closure_call1, js_closure_call2, js_closure_call_array,
    js_closure_get_capture_ptr, js_closure_set_capture_ptr, js_register_closure_arity,
    ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

/// One entry per named export of one submodule.
struct ExportSpec {
    name: &'static str,
    thunk: ExportThunk,
}

enum ExportThunk {
    Fn1(extern "C" fn(*const ClosureHeader, f64) -> f64),
    Fn2(extern "C" fn(*const ClosureHeader, f64, f64) -> f64),
}

impl ExportThunk {
    fn as_ptr(&self) -> *const u8 {
        match self {
            ExportThunk::Fn1(f) => *f as *const u8,
            ExportThunk::Fn2(f) => *f as *const u8,
        }
    }
}

/// One entry per submodule. `exports` lists every named export the
/// codegen / parity tests reach for; the codegen's lookup is keyed by
/// `(submodule_key, export_name)` and falls back to `TAG_TRUE` if no
/// matching entry is found (preserving the pre-#841 behavior for any
/// future export Perry doesn't yet know about).
struct SubmoduleSpec {
    /// Stable key — matches the prefix used in the generated FFI symbol
    /// names (`js_node_submod_<key>_export_<name>`).
    key: &'static str,
    exports: &'static [ExportSpec],
}

// ----- thunks -----
//
// One thunk per (submodule, export). All thunks share the same shape:
// they raise an explicit `Error` describing what's missing. Closure
// dispatch invokes them via `js_closure_call0` / `js_closure_call1`
// regardless of declared arity, so a single `(_closure, _arg) -> f64`
// signature is sufficient — Perry's closure ABI tolerates an arg shape
// mismatch on the receiving side (the value is just ignored).

macro_rules! thunk {
    ($name:ident, $msg:expr) => {
        extern "C" fn $name(_closure: *const ClosureHeader, _arg: f64) -> f64 {
            let msg: &'static str = $msg;
            let bytes = msg.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            let err = crate::error::js_error_new_with_message(header);
            let bits = JSValue::pointer(err as *const u8).bits();
            crate::exception::js_throw(f64::from_bits(bits))
        }
    };
}

/// node:timers/promises.setTimeout(delay, value?) — a Promise that resolves
/// with `value` (or undefined) after `delay` ms. Composes the existing
/// promise-returning timer primitive; the closure dispatch pads a missing
/// `value` arg with undefined (arity registered in `ensure_export_singleton`).
/// Refs #1213.
extern "C" fn timers_promises_set_timeout(
    _closure: *const ClosureHeader,
    delay_ms: f64,
    value: f64,
) -> f64 {
    let promise = crate::timer::js_set_timeout_value(delay_ms, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

/// node:timers/promises.setImmediate(value?) — a Promise that resolves with
/// `value` (or undefined) on a later turn. Refs #1213.
extern "C" fn timers_promises_set_immediate(_closure: *const ClosureHeader, value: f64) -> f64 {
    let promise = crate::timer::js_set_timeout_value(0.0, value);
    crate::value::js_nanbox_pointer(promise as i64)
}

thunk!(
    thunk_timers_setInterval,
    "node:timers/promises.setInterval is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_timers_scheduler,
    "node:timers/promises.scheduler is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(thunk_readline_createInterface, "node:readline/promises.createInterface is not yet implemented in Perry (tracked by issue #793).");
thunk!(
    thunk_readline_Interface,
    "node:readline/promises.Interface is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_readline_Readline,
    "node:readline/promises.Readline is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_streamP_pipeline,
    "node:stream/promises.pipeline is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_streamP_finished,
    "node:stream/promises.finished is not yet implemented in Perry (tracked by issue #793)."
);

thunk!(
    thunk_consumers_text,
    "node:stream/consumers.text is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_json,
    "node:stream/consumers.json is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_buffer,
    "node:stream/consumers.buffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_arrayBuffer,
    "node:stream/consumers.arrayBuffer is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_bytes,
    "node:stream/consumers.bytes is not yet implemented in Perry (tracked by issue #793)."
);
thunk!(
    thunk_consumers_blob,
    "node:stream/consumers.blob is not yet implemented in Perry (tracked by issue #793)."
);

// node:sys is a deprecated alias for node:util — point each export at
// the same thunks until util's named-export surface is wired up. The
// parity test compares `sys.format === util.format` for identity; for
// now both report `typeof === "function"` (passing the typeof gate) but
// the strict-equality check still diverges. That divergence is
// pre-existing (node:util's named exports lower to NativeModuleRef =>
// `typeof === "object"` today) — it's the parent-module half of #793.
thunk!(thunk_sys_format, "node:sys.format is not yet implemented in Perry (use node:util.format; node:sys is deprecated).");
thunk!(thunk_sys_inspect, "node:sys.inspect is not yet implemented in Perry (use node:util.inspect; node:sys is deprecated).");
thunk!(thunk_sys_debuglog, "node:sys.debuglog is not yet implemented in Perry (use node:util.debuglog; node:sys is deprecated).");
thunk!(thunk_sys_deprecate, "node:sys.deprecate is not yet implemented in Perry (use node:util.deprecate; node:sys is deprecated).");
thunk!(thunk_sys_promisify, "node:sys.promisify is not yet implemented in Perry (use node:util.promisify; node:sys is deprecated).");
thunk!(thunk_sys_callbackify, "node:sys.callbackify is not yet implemented in Perry (use node:util.callbackify; node:sys is deprecated).");
thunk!(thunk_sys_isArray, "node:sys.isArray is not yet implemented in Perry (use node:util.isArray; node:sys is deprecated).");

// ----- node:diagnostics_channel thunks (#906 follow-up) -----
//
// Pino reads `require('node:diagnostics_channel').tracingChannel('pino_asJson')`
// at top-level module init in `lib/tools.js`. Without these, the codegen
// catch-all returned TAG_TRUE so `diagChan.tracingChannel(...)` threw
// `TypeError: (boolean).tracingChannel is not a function` before any of
// pino's actual logging logic ran. Two of the thunks here construct
// non-trivial return values:
//
//   - `tracingChannel(name)` returns a TracingChannel-shaped stub object
//     whose `hasSubscribers` is `false`. Pino tests that property before
//     entering the tracing branch (`lib/tools.js::asJson`):
//         if (asJsonChan.hasSubscribers === false) {
//             return _asJson.call(this, obj, msg, num, time)
//         }
//     so the fast path is taken and `traceSync` is never invoked. The
//     returned object also carries `subscribe` / `unsubscribe` / `traceSync` /
//     `tracePromise` / `traceCallback` slots set to no-op closures, just
//     in case a consumer doesn't gate on `hasSubscribers`.
//
//   - `channel(name)` mirrors the same shape with `hasSubscribers: false`
//     and a `publish` no-op — same minimal "satisfies type probe" goal.
//
// Other entries (`subscribe`, `unsubscribe`, `publish`, `hasSubscribers`)
// surface as no-op thrower thunks the same way the other submodules do —
// real-tracing semantics are a follow-up under #793.

extern "C" fn thunk_diag_noop(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[derive(Hash, Eq, PartialEq, Clone)]
enum DiagChannelKey {
    String(String),
    Symbol(u64),
}

struct DiagChannelState {
    name: f64,
    obj: *mut ObjectHeader,
    subscribers: Vec<f64>,
    stores: Vec<(f64, Option<f64>)>,
}

struct DiagTracingState {
    obj: *mut ObjectHeader,
    events: [i64; 5],
}

// Known follow-ups for the thread-local state below:
//
// * #1309 — channels are pinned for the process lifetime. Entries are
//   never removed from `DIAG_CHANNEL_BY_KEY`/`DIAG_CHANNELS`, so a
//   long-running service that mints per-request channel names leaks
//   memory unboundedly. Node holds channels weakly; mirroring that
//   needs either a GC post-sweep hook or a weak-ref primitive.
//
// * #1310 — these maps are `thread_local!`, so `parallelMap`/`spawn`
//   workers see an empty world. A `publish` from a worker thread
//   silently no-ops against subscribers registered on the main
//   thread, diverging from Node's process-global model.
thread_local! {
    static DIAG_CHANNEL_BY_KEY: RefCell<HashMap<DiagChannelKey, i64>> = RefCell::new(HashMap::new());
    static DIAG_CHANNELS: RefCell<HashMap<i64, DiagChannelState>> = RefCell::new(HashMap::new());
    static DIAG_TRACES: RefCell<HashMap<i64, DiagTracingState>> = RefCell::new(HashMap::new());
    static DIAG_PENDING_UNCAUGHT: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    static DIAG_SUPPRESS_UNCAUGHT_DRAIN: RefCell<usize> = const { RefCell::new(0) };
    static NEXT_DIAG_ID: RefCell<i64> = const { RefCell::new(1) };
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
fn bool_value(v: bool) -> f64 {
    f64::from_bits(if v { TAG_TRUE } else { TAG_FALSE })
}

fn boxed_ptr<T>(p: *const T) -> f64 {
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

fn decode_string_value(value: f64) -> Option<String> {
    let bits = value.to_bits();
    let tag = bits & crate::value::TAG_MASK;
    if tag == crate::value::SHORT_STRING_TAG {
        let js = crate::value::JSValue::from_bits(bits);
        let mut buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let len = js.short_string_to_buf(&mut buf);
        return Some(String::from_utf8_lossy(&buf[..len]).into_owned());
    }
    if tag != crate::value::STRING_TAG {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, (*ptr).byte_len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn channel_key(name: f64) -> Option<DiagChannelKey> {
    if let Some(s) = decode_string_value(name) {
        return Some(DiagChannelKey::String(s));
    }
    let bits = name.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag == crate::value::POINTER_TAG && unsafe { crate::symbol::js_is_symbol(name) } != 0 {
        return Some(DiagChannelKey::Symbol(bits));
    }
    None
}

thread_local! {
    /// Side table keyed on an ErrorHeader's `message` string pointer (which
    /// is allocated fresh per throw via `js_string_from_bytes`). The
    /// `.code` getter in `object::field_get_set` consults this map to
    /// recover an `ERR_*` code without resorting to substring matches on
    /// the message text — which would have applied to any user-thrown
    /// error sharing the same message string. Stale entries (after the
    /// referenced StringHeader is GC'd) are harmless: a fresh allocation
    /// will overwrite the slot via `register_error_code` the next time we
    /// register a code, and lookups for unrelated message pointers miss.
    static ERROR_MESSAGE_CODES: RefCell<HashMap<usize, &'static str>> =
        RefCell::new(HashMap::new());
}

fn register_error_code(message_ptr: *const StringHeader, code: &'static str) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_CODES.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, code);
    });
}

/// Returns the explicit `ERR_*` code registered for an Error's `message`
/// pointer, if any. Called from the `.code` property getter.
pub fn error_code_for_message(message_ptr: *const StringHeader) -> Option<&'static str> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_CODES.with(|m| m.borrow().get(&(message_ptr as usize)).copied())
}

fn throw_invalid_arg() -> ! {
    let msg = b"The argument is invalid";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

fn throw_type_error_no_code(message: &[u8]) -> ! {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

fn next_diag_id() -> i64 {
    NEXT_DIAG_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    })
}

fn valid_closure_value(v: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(v) as usize;
    raw >= 0x10000 && crate::closure::is_closure_ptr(raw)
}

fn closure_ptr(v: f64) -> *const ClosureHeader {
    crate::value::js_nanbox_get_pointer(v) as *const ClosureHeader
}

fn set_field_value(obj: *mut ObjectHeader, name: &str, value: f64) {
    unsafe {
        let key = js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
        js_object_set_field_by_name(obj, key, value);
    }
}

fn get_field_value(obj: *mut ObjectHeader, name: &str) -> f64 {
    unsafe {
        let key = js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
        js_object_get_field_by_name_f64(obj, key)
    }
}

#[allow(clippy::missing_transmute_annotations)]
fn cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast3(f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast4(f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast5(f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
fn cast7(
    f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64,
) -> *const u8 {
    f as *const u8
}

fn method_closure(func: *const u8, arity: u32, id: i64) -> f64 {
    let c = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(c, 0, id);
    js_register_closure_arity(func, arity);
    boxed_ptr(c)
}

fn method_id(closure: *const ClosureHeader) -> i64 {
    js_closure_get_capture_ptr(closure, 0)
}

fn catch_js<F: FnOnce() -> f64>(f: F) -> Result<f64, f64> {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
    if jumped == 0 {
        let result = f();
        crate::exception::js_try_end();
        Ok(result)
    } else {
        crate::exception::js_try_end();
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(err)
    }
}

extern "C" fn throw_captured_error(closure: *const ClosureHeader) -> f64 {
    let err = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    crate::exception::js_throw(err)
}

fn schedule_uncaught(err: f64) {
    DIAG_PENDING_UNCAUGHT.with(|q| q.borrow_mut().push(err));
}

pub fn diagnostics_channel_drain_uncaught() {
    if DIAG_SUPPRESS_UNCAUGHT_DRAIN.with(|n| *n.borrow() > 0) {
        return;
    }
    let pending = DIAG_PENDING_UNCAUGHT.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for err in pending {
        crate::os::emit_process_uncaught_exception(err);
    }
}

fn suppress_uncaught_drain<F: FnOnce() -> f64>(f: F) -> f64 {
    DIAG_SUPPRESS_UNCAUGHT_DRAIN.with(|n| *n.borrow_mut() += 1);
    let result = f();
    DIAG_SUPPRESS_UNCAUGHT_DRAIN.with(|n| {
        let mut n = n.borrow_mut();
        *n = n.saturating_sub(1);
    });
    result
}

fn with_implicit_this<F: FnOnce() -> f64>(this_arg: f64, f: F) -> f64 {
    let prev = crate::object::js_implicit_this_set(this_arg);
    let result = f();
    crate::object::js_implicit_this_set(prev);
    result
}

fn update_channel_active(id: i64) {
    DIAG_CHANNELS.with(|channels| {
        if let Some(ch) = channels.borrow_mut().get_mut(&id) {
            let active = !ch.subscribers.is_empty() || !ch.stores.is_empty();
            set_field_value(ch.obj, "hasSubscribers", bool_value(active));
        }
    });
    update_all_tracing_active();
}

fn update_all_tracing_active() {
    let active_for = |id: i64| -> bool {
        DIAG_CHANNELS.with(|channels| {
            channels
                .borrow()
                .get(&id)
                .map(|c| !c.subscribers.is_empty() || !c.stores.is_empty())
                .unwrap_or(false)
        })
    };
    DIAG_TRACES.with(|traces| {
        for trace in traces.borrow_mut().values_mut() {
            let active = trace.events.iter().copied().any(active_for);
            set_field_value(trace.obj, "hasSubscribers", bool_value(active));
        }
    });
}

fn ensure_channel(name: f64) -> i64 {
    let key = channel_key(name).unwrap_or_else(|| throw_invalid_arg());
    if let Some(id) = DIAG_CHANNEL_BY_KEY.with(|m| m.borrow().get(&key).copied()) {
        return id;
    }
    let id = next_diag_id();
    let obj = js_object_alloc(0, 9);
    set_field_value(obj, "name", name);
    set_field_value(obj, "hasSubscribers", bool_value(false));
    set_field_value(
        obj,
        "subscribe",
        method_closure(cast1(diag_channel_subscribe), 1, id),
    );
    set_field_value(
        obj,
        "unsubscribe",
        method_closure(cast1(diag_channel_unsubscribe), 1, id),
    );
    set_field_value(
        obj,
        "publish",
        method_closure(cast1(diag_channel_publish), 1, id),
    );
    set_field_value(
        obj,
        "bindStore",
        method_closure(cast2(diag_channel_bind_store), 2, id),
    );
    set_field_value(
        obj,
        "unbindStore",
        method_closure(cast1(diag_channel_unbind_store), 1, id),
    );
    set_field_value(
        obj,
        "runStores",
        method_closure(cast5(diag_channel_run_stores), 5, id),
    );
    DIAG_CHANNEL_BY_KEY.with(|m| {
        m.borrow_mut().insert(key, id);
    });
    DIAG_CHANNELS.with(|m| {
        m.borrow_mut().insert(
            id,
            DiagChannelState {
                name,
                obj,
                subscribers: Vec::new(),
                stores: Vec::new(),
            },
        );
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    id
}

fn channel_obj(id: i64) -> f64 {
    DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|c| boxed_ptr(c.obj))
            .unwrap_or_else(undefined)
    })
}

fn add_subscriber(id: i64, subscriber: f64) {
    if !valid_closure_value(subscriber) {
        throw_invalid_arg();
    }
    DIAG_CHANNELS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&id) {
            c.subscribers.push(subscriber);
        }
    });
    update_channel_active(id);
}

fn remove_subscriber(id: i64, subscriber: f64) -> bool {
    let removed = DIAG_CHANNELS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&id) {
            let bits = subscriber.to_bits();
            if let Some(pos) = c.subscribers.iter().position(|v| v.to_bits() == bits) {
                c.subscribers.remove(pos);
                return true;
            }
        }
        false
    });
    if removed {
        update_channel_active(id);
    }
    removed
}

fn publish_channel(id: i64, data: f64) {
    // Fast path: no subscribers means publish is a no-op. Avoid the
    // Vec clone entirely. `console.*` hits this path on every call when
    // nobody is subscribed to a `console.{method}` channel.
    let (name, subscribers) = DIAG_CHANNELS.with(|m| {
        let m = m.borrow();
        match m.get(&id) {
            Some(c) if !c.subscribers.is_empty() => (c.name, c.subscribers.clone()),
            Some(c) => (c.name, Vec::new()),
            None => (undefined(), Vec::new()),
        }
    });
    if subscribers.is_empty() {
        return;
    }
    for subscriber in subscribers {
        // Match Node's safe subscriber behavior for the happy path; exceptions
        // propagate through Perry's exception mechanism and are catchable.
        let cb = closure_ptr(subscriber);
        if let Err(err) = catch_js(|| js_closure_call2(cb, data, name)) {
            schedule_uncaught(err);
        }
    }
}

// Cached channel ids for the five `console.*` diagnostics channels.
// Zero means "not yet looked up". The five-element static keeps the
// `console.log` hot path branch-free: load atomic, miss → check by-key
// map, hit → check subscriber count. No string formatting or string
// allocation happens unless a subscriber actually exists.
static CONSOLE_LOG_CHANNEL_ID: AtomicI64 = AtomicI64::new(0);
static CONSOLE_INFO_CHANNEL_ID: AtomicI64 = AtomicI64::new(0);
static CONSOLE_DEBUG_CHANNEL_ID: AtomicI64 = AtomicI64::new(0);
static CONSOLE_ERROR_CHANNEL_ID: AtomicI64 = AtomicI64::new(0);
static CONSOLE_WARN_CHANNEL_ID: AtomicI64 = AtomicI64::new(0);

fn console_channel_slot(method: &str) -> Option<(&'static AtomicI64, &'static str)> {
    match method {
        "log" => Some((&CONSOLE_LOG_CHANNEL_ID, "console.log")),
        "info" => Some((&CONSOLE_INFO_CHANNEL_ID, "console.info")),
        "debug" => Some((&CONSOLE_DEBUG_CHANNEL_ID, "console.debug")),
        "error" => Some((&CONSOLE_ERROR_CHANNEL_ID, "console.error")),
        "warn" => Some((&CONSOLE_WARN_CHANNEL_ID, "console.warn")),
        _ => None,
    }
}

/// Publish the argument array for Node's console diagnostics channels.
/// Called by `builtins::console` before formatting so subscribers can inspect
/// and mutate arguments, matching Node's `console.*` integration.
pub fn diagnostics_channel_publish_console(method: &str, arr: *const crate::array::ArrayHeader) {
    // Hot path: bail out before ANY allocation when no diag channels exist
    // at all. The atomic load is roughly free on x86; `Relaxed` is fine here
    // because we don't synchronize anything beyond the flag itself.
    if ANY_SINGLETON_ALLOCATED.load(Ordering::Relaxed) == 0 {
        return;
    }
    let Some((slot, key)) = console_channel_slot(method) else {
        return;
    };
    // Resolve the channel id without allocating. If nobody has subscribed
    // (or even called `dc.channel("console.<m>")`), the by-key lookup
    // misses and we return without formatting anything.
    let mut id = slot.load(Ordering::Relaxed);
    if id == 0 {
        let lookup = DIAG_CHANNEL_BY_KEY.with(|m| {
            m.borrow()
                .get(&DiagChannelKey::String(key.to_string()))
                .copied()
        });
        match lookup {
            Some(real_id) => {
                slot.store(real_id, Ordering::Relaxed);
                id = real_id;
            }
            None => return,
        }
    }
    // Fast subscriber check: if the channel exists but is empty, skip.
    let has_subs = DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .is_some_and(|c| !c.subscribers.is_empty())
    });
    if !has_subs {
        return;
    }
    let arr_value = if arr.is_null() {
        undefined()
    } else {
        boxed_ptr(arr)
    };
    publish_channel(id, arr_value);
}

fn run_store_wrapped(id: i64, data: f64, fn_value: f64, this_arg: f64, args: &[f64]) -> f64 {
    publish_channel(id, data);
    if !valid_closure_value(fn_value) {
        crate::closure::throw_not_callable();
    }
    let rebound = crate::closure::clone_closure_rebind_this(fn_value.to_bits(), this_arg);
    let cb = (rebound & crate::value::POINTER_MASK) as *const ClosureHeader;
    with_implicit_this(this_arg, || unsafe {
        js_closure_call_array(cb as i64, args.as_ptr(), args.len() as i64)
    })
}

extern "C" fn diag_channel_subscribe(closure: *const ClosureHeader, subscriber: f64) -> f64 {
    add_subscriber(method_id(closure), subscriber);
    undefined()
}

extern "C" fn diag_channel_unsubscribe(closure: *const ClosureHeader, subscriber: f64) -> f64 {
    bool_value(remove_subscriber(method_id(closure), subscriber))
}

extern "C" fn diag_channel_publish(closure: *const ClosureHeader, data: f64) -> f64 {
    publish_channel(method_id(closure), data);
    undefined()
}

extern "C" fn diag_channel_bind_store(
    closure: *const ClosureHeader,
    store: f64,
    transform: f64,
) -> f64 {
    let id = method_id(closure);
    let transform = if valid_closure_value(transform) {
        Some(transform)
    } else {
        None
    };
    DIAG_CHANNELS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&id) {
            if let Some(slot) = c
                .stores
                .iter_mut()
                .find(|(s, _)| s.to_bits() == store.to_bits())
            {
                slot.1 = transform;
            } else {
                c.stores.push((store, transform));
            }
        }
    });
    update_channel_active(id);
    undefined()
}

extern "C" fn diag_channel_unbind_store(closure: *const ClosureHeader, store: f64) -> f64 {
    let id = method_id(closure);
    let removed = DIAG_CHANNELS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&id) {
            if let Some(pos) = c
                .stores
                .iter()
                .position(|(s, _)| s.to_bits() == store.to_bits())
            {
                c.stores.remove(pos);
                return true;
            }
        }
        false
    });
    if removed {
        update_channel_active(id);
    }
    bool_value(removed)
}

fn call_store_run(store: f64, context: f64, next: f64) -> f64 {
    fn run_als(handle: i64, context: f64, next: f64) -> f64 {
        crate::async_context::push_store(handle, context);
        let result = js_closure_call0(closure_ptr(next));
        crate::async_context::pop_store(handle);
        result
    }
    let bits = store.to_bits();
    if bits & crate::value::TAG_MASK == crate::value::INT32_TAG {
        let handle = (bits & crate::value::INT32_MASK) as i32 as i64;
        return run_als(handle, context, next);
    }
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        let raw = (bits & crate::value::POINTER_MASK) as i64;
        if raw > 0 && raw < 0x10000 {
            return run_als(raw, context, next);
        }
    }
    if store.is_finite() {
        return run_als(store as i64, context, next);
    }
    let obj = crate::value::js_nanbox_get_pointer(store) as *mut ObjectHeader;
    let run = get_field_value(obj, "run");
    js_closure_call2(closure_ptr(run), context, next)
}

extern "C" fn store_next_thunk(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_ptr(closure, 0);
    let data = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let fn_value = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    let this_arg = f64::from_bits(js_closure_get_capture_ptr(closure, 3) as u64);
    let a = f64::from_bits(js_closure_get_capture_ptr(closure, 4) as u64);
    let b = f64::from_bits(js_closure_get_capture_ptr(closure, 5) as u64);
    run_store_wrapped(id, data, fn_value, this_arg, &[a, b])
}

extern "C" fn store_chain_thunk(closure: *const ClosureHeader) -> f64 {
    let store = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let context = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let next = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    call_store_run(store, context, next)
}

extern "C" fn diag_channel_run_stores(
    closure: *const ClosureHeader,
    data: f64,
    fn_value: f64,
    this_arg: f64,
    a: f64,
    b: f64,
) -> f64 {
    let id = method_id(closure);
    // Minimal implementation: apply all bound stores in insertion order for
    // the two-argument shape used by the parity suite.
    let stores = DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|c| c.stores.clone())
            .unwrap_or_default()
    });
    if stores.is_empty() {
        return run_store_wrapped(id, data, fn_value, this_arg, &[a, b]);
    }
    let mut next = js_closure_alloc(cast0(store_next_thunk), 6);
    js_register_closure_arity(cast0(store_next_thunk), 0);
    js_closure_set_capture_ptr(next, 0, id);
    js_closure_set_capture_ptr(next, 1, data.to_bits() as i64);
    js_closure_set_capture_ptr(next, 2, fn_value.to_bits() as i64);
    js_closure_set_capture_ptr(next, 3, this_arg.to_bits() as i64);
    js_closure_set_capture_ptr(next, 4, a.to_bits() as i64);
    js_closure_set_capture_ptr(next, 5, b.to_bits() as i64);
    let mut next_value = boxed_ptr(next);
    for (store, transform) in stores.into_iter().rev() {
        let context = if let Some(t) = transform {
            match catch_js(|| js_closure_call1(closure_ptr(t), data)) {
                Ok(context) => context,
                Err(err) => {
                    schedule_uncaught(err);
                    continue;
                }
            }
        } else {
            data
        };
        let chain = js_closure_alloc(cast0(store_chain_thunk), 3);
        js_register_closure_arity(cast0(store_chain_thunk), 0);
        js_closure_set_capture_ptr(chain, 0, store.to_bits() as i64);
        js_closure_set_capture_ptr(chain, 1, context.to_bits() as i64);
        js_closure_set_capture_ptr(chain, 2, next_value.to_bits() as i64);
        next_value = boxed_ptr(chain);
    }
    suppress_uncaught_drain(|| js_closure_call0(closure_ptr(next_value)))
}

extern "C" fn thunk_diag_channel(closure: *const ClosureHeader, name: f64) -> f64 {
    let _ = closure;
    channel_obj(ensure_channel(name))
}

extern "C" fn thunk_diag_subscribe(
    _closure: *const ClosureHeader,
    name: f64,
    subscriber: f64,
) -> f64 {
    let id = ensure_channel(name);
    add_subscriber(id, subscriber);
    undefined()
}

extern "C" fn thunk_diag_unsubscribe(
    _closure: *const ClosureHeader,
    name: f64,
    subscriber: f64,
) -> f64 {
    let id = ensure_channel(name);
    bool_value(remove_subscriber(id, subscriber))
}

extern "C" fn thunk_diag_has_subscribers(_closure: *const ClosureHeader, name: f64) -> f64 {
    let key = match channel_key(name) {
        Some(k) => k,
        None => return bool_value(false),
    };
    let active = DIAG_CHANNEL_BY_KEY.with(|by_key| {
        by_key
            .borrow()
            .get(&key)
            .copied()
            .and_then(|id| {
                DIAG_CHANNELS.with(|channels| {
                    channels
                        .borrow()
                        .get(&id)
                        .map(|c| !c.subscribers.is_empty() || !c.stores.is_empty())
                })
            })
            .unwrap_or(false)
    });
    bool_value(active)
}

fn tracing_event_name(base: f64, event: &str) -> f64 {
    let base = decode_string_value(base).unwrap_or_else(|| "unknown".to_string());
    let s = format!("tracing:{base}:{event}");
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::STRING_TAG | (ptr as u64 & crate::value::POINTER_MASK))
}

fn channel_from_object_property(obj_value: f64, prop: &str) -> i64 {
    let obj = crate::value::js_nanbox_get_pointer(obj_value) as *mut ObjectHeader;
    if obj.is_null() {
        throw_invalid_arg();
    }
    let ch_value = get_field_value(obj, prop);
    if ch_value.to_bits() == crate::value::TAG_UNDEFINED {
        throw_type_error_no_code(b"Invalid channel object");
    }
    let ch_obj = crate::value::js_nanbox_get_pointer(ch_value) as *mut ObjectHeader;
    if ch_obj.is_null() {
        throw_invalid_arg();
    }
    DIAG_CHANNELS.with(|m| {
        for (id, state) in m.borrow().iter() {
            if state.obj == ch_obj {
                return *id;
            }
        }
        throw_invalid_arg()
    })
}

extern "C" fn diag_trace_subscribe(closure: *const ClosureHeader, handlers: f64) -> f64 {
    let id = method_id(closure);
    let events = DIAG_TRACES.with(|m| m.borrow().get(&id).map(|t| t.events).unwrap_or([0; 5]));
    for (idx, name) in ["start", "end", "asyncStart", "asyncEnd", "error"]
        .iter()
        .enumerate()
    {
        let h = get_field_value(
            crate::value::js_nanbox_get_pointer(handlers) as *mut ObjectHeader,
            name,
        );
        // Absent keys (undefined OR null) are silently skipped — Node
        // only requires that *present-and-defined* handler values be
        // callable. Anything else present throws ERR_INVALID_ARG_TYPE.
        let h_bits = h.to_bits();
        if h_bits == TAG_UNDEFINED || h_bits == crate::value::TAG_NULL {
            continue;
        }
        if !valid_closure_value(h) {
            throw_invalid_arg();
        }
        add_subscriber(events[idx], h);
    }
    undefined()
}

extern "C" fn diag_trace_unsubscribe(closure: *const ClosureHeader, handlers: f64) -> f64 {
    let id = method_id(closure);
    let events = DIAG_TRACES.with(|m| m.borrow().get(&id).map(|t| t.events).unwrap_or([0; 5]));
    let mut ok = true;
    for (idx, name) in ["start", "end", "asyncStart", "asyncEnd", "error"]
        .iter()
        .enumerate()
    {
        let h = get_field_value(
            crate::value::js_nanbox_get_pointer(handlers) as *mut ObjectHeader,
            name,
        );
        if valid_closure_value(h) && !remove_subscriber(events[idx], h) {
            ok = false;
        }
    }
    bool_value(ok)
}

fn call_fn_value(fn_value: f64, this_arg: f64, args: &[f64]) -> f64 {
    if !valid_closure_value(fn_value) {
        crate::closure::throw_not_callable();
    }
    let rebound = crate::closure::clone_closure_rebind_this(fn_value.to_bits(), this_arg);
    let cb = (rebound & crate::value::POINTER_MASK) as *const ClosureHeader;
    with_implicit_this(this_arg, || unsafe {
        js_closure_call_array(cb as i64, args.as_ptr(), args.len() as i64)
    })
}

extern "C" fn diag_trace_sync(
    closure: *const ClosureHeader,
    fn_value: f64,
    context: f64,
    this_arg: f64,
    arg: f64,
) -> f64 {
    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    let active = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| get_field_value(t.obj, "hasSubscribers").to_bits() == TAG_TRUE)
            .unwrap_or(false)
    });
    if !active {
        return call_fn_value(fn_value, this_arg, &[arg]);
    }
    publish_channel(events[0], context);
    match catch_js(|| call_fn_value(fn_value, this_arg, &[arg])) {
        Ok(result) => {
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                result,
            );
            publish_channel(events[1], context);
            result
        }
        Err(err) => {
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "error",
                err,
            );
            publish_channel(events[4], context);
            publish_channel(events[1], context);
            crate::exception::js_throw(err)
        }
    }
}

extern "C" fn diag_trace_promise(
    closure: *const ClosureHeader,
    fn_value: f64,
    context: f64,
    this_arg: f64,
) -> f64 {
    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    publish_channel(events[0], context);
    let result = call_fn_value(fn_value, this_arg, &[]);
    let result_ptr = crate::value::js_nanbox_get_pointer(result) as *mut crate::promise::Promise;
    if !result_ptr.is_null()
        && (result.to_bits() & crate::value::TAG_MASK) == crate::value::POINTER_TAG
    {
        let _ = crate::promise::js_promise_run_microtasks();
        let state = crate::promise::js_promise_state(result_ptr);
        if state == 1 {
            publish_channel(events[1], context);
            let value = crate::promise::js_promise_value(result_ptr);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                value,
            );
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            return result;
        } else if state == 2 {
            // Node's TracingChannel#tracePromise rejection order is
            // start, end, error, asyncStart, asyncEnd — confirmed
            // against `node --experimental-strip-types` 24.x. The
            // earlier handwritten ordering shipped here matches that
            // sequence after the `end` publish.
            publish_channel(events[1], context);
            let reason = crate::promise::js_promise_reason(result_ptr);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "error",
                reason,
            );
            publish_channel(events[4], context);
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            return result;
        } else {
            publish_channel(events[1], context);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                result,
            );
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            return result;
        }
    } else {
        publish_channel(events[1], context);
        set_field_value(
            crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
            "result",
            result,
        );
        publish_channel(events[2], context);
        publish_channel(events[3], context);
        return result;
    }
}

extern "C" fn diag_trace_callback(
    closure: *const ClosureHeader,
    fn_value: f64,
    _position: f64,
    context: f64,
    this_arg: f64,
    callback: f64,
    err: f64,
    res: f64,
) -> f64 {
    if !valid_closure_value(callback) {
        throw_invalid_arg();
    }
    let events = DIAG_TRACES.with(|m| {
        m.borrow()
            .get(&method_id(closure))
            .map(|t| t.events)
            .unwrap_or([0; 5])
    });
    publish_channel(events[0], context);
    let ret = call_fn_value(fn_value, this_arg, &[callback, err, res]);
    // Node's traceCallback fires events in this order around the
    // wrapped callback: start, (fn → user callback runs synchronously,
    // possibly producing user-visible "fn" log lines), asyncStart,
    // [error,] asyncEnd, end. End fires *last*, not second — without
    // a real callback wrap boundary we approximate by publishing the
    // async events immediately after `fn` returns. A truthy `err`
    // publishes `error` between asyncStart and asyncEnd. Full
    // async-boundary fidelity is tracked by #788.
    set_field_value(
        crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
        "result",
        res,
    );
    publish_channel(events[2], context);
    if crate::value::js_is_truthy(err) != 0 {
        set_field_value(
            crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
            "error",
            err,
        );
        publish_channel(events[4], context);
    }
    publish_channel(events[3], context);
    publish_channel(events[1], context);
    ret
}

extern "C" fn thunk_diag_tracing_channel(
    _closure: *const ClosureHeader,
    name_or_channels: f64,
) -> f64 {
    let id = next_diag_id();
    let events = if channel_key(name_or_channels).is_some() {
        [
            ensure_channel(tracing_event_name(name_or_channels, "start")),
            ensure_channel(tracing_event_name(name_or_channels, "end")),
            ensure_channel(tracing_event_name(name_or_channels, "asyncStart")),
            ensure_channel(tracing_event_name(name_or_channels, "asyncEnd")),
            ensure_channel(tracing_event_name(name_or_channels, "error")),
        ]
    } else if (name_or_channels.to_bits() & crate::value::POINTER_TAG) == crate::value::POINTER_TAG
    {
        [
            channel_from_object_property(name_or_channels, "start"),
            channel_from_object_property(name_or_channels, "end"),
            channel_from_object_property(name_or_channels, "asyncStart"),
            channel_from_object_property(name_or_channels, "asyncEnd"),
            channel_from_object_property(name_or_channels, "error"),
        ]
    } else {
        throw_invalid_arg();
    };
    let obj = js_object_alloc(0, 12);
    set_field_value(obj, "start", channel_obj(events[0]));
    set_field_value(obj, "end", channel_obj(events[1]));
    set_field_value(obj, "asyncStart", channel_obj(events[2]));
    set_field_value(obj, "asyncEnd", channel_obj(events[3]));
    set_field_value(obj, "error", channel_obj(events[4]));
    set_field_value(obj, "hasSubscribers", bool_value(false));
    set_field_value(
        obj,
        "subscribe",
        method_closure(cast1(diag_trace_subscribe), 1, id),
    );
    set_field_value(
        obj,
        "unsubscribe",
        method_closure(cast1(diag_trace_unsubscribe), 1, id),
    );
    set_field_value(
        obj,
        "traceSync",
        method_closure(cast4(diag_trace_sync), 4, id),
    );
    set_field_value(
        obj,
        "tracePromise",
        method_closure(cast3(diag_trace_promise), 3, id),
    );
    set_field_value(
        obj,
        "traceCallback",
        method_closure(cast7(diag_trace_callback), 7, id),
    );
    DIAG_TRACES.with(|m| {
        m.borrow_mut().insert(id, DiagTracingState { obj, events });
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    boxed_ptr(obj)
}

// One singleton no-op closure shared by every "function" field on the
// tracingChannel / channel stubs. Kept alive for the process's lifetime
// via the same GC root scanner that protects the other submodule
// singletons (see `scan_node_submodule_singleton_roots`).
thread_local! {
    static DIAG_NOOP_CLOSURE: RefCell<Option<*mut ClosureHeader>> = const { RefCell::new(None) };
}

fn ensure_diag_noop_closure() -> *mut ClosureHeader {
    DIAG_NOOP_CLOSURE.with(|slot| {
        if let Some(ptr) = *slot.borrow() {
            return ptr;
        }
        let allocated = js_closure_alloc(thunk_diag_noop as *const u8, 0);
        *slot.borrow_mut() = Some(allocated);
        ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
        allocated
    })
}

// ----- submodule table -----

const SUBMODULES: &[SubmoduleSpec] = &[
    SubmoduleSpec {
        key: "timers_promises",
        exports: &[
            ExportSpec {
                name: "setTimeout",
                thunk: ExportThunk::Fn2(timers_promises_set_timeout),
            },
            ExportSpec {
                name: "setImmediate",
                thunk: ExportThunk::Fn1(timers_promises_set_immediate),
            },
            ExportSpec {
                name: "setInterval",
                thunk: ExportThunk::Fn1(thunk_timers_setInterval),
            },
            ExportSpec {
                name: "scheduler",
                thunk: ExportThunk::Fn1(thunk_timers_scheduler),
            },
        ],
    },
    SubmoduleSpec {
        key: "readline_promises",
        exports: &[
            ExportSpec {
                name: "createInterface",
                thunk: ExportThunk::Fn1(thunk_readline_createInterface),
            },
            ExportSpec {
                name: "Interface",
                thunk: ExportThunk::Fn1(thunk_readline_Interface),
            },
            ExportSpec {
                name: "Readline",
                thunk: ExportThunk::Fn1(thunk_readline_Readline),
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_promises",
        exports: &[
            ExportSpec {
                name: "pipeline",
                thunk: ExportThunk::Fn1(thunk_streamP_pipeline),
            },
            ExportSpec {
                name: "finished",
                thunk: ExportThunk::Fn1(thunk_streamP_finished),
            },
        ],
    },
    SubmoduleSpec {
        key: "stream_consumers",
        exports: &[
            ExportSpec {
                name: "text",
                thunk: ExportThunk::Fn1(thunk_consumers_text),
            },
            ExportSpec {
                name: "json",
                thunk: ExportThunk::Fn1(thunk_consumers_json),
            },
            ExportSpec {
                name: "buffer",
                thunk: ExportThunk::Fn1(thunk_consumers_buffer),
            },
            ExportSpec {
                name: "arrayBuffer",
                thunk: ExportThunk::Fn1(thunk_consumers_arrayBuffer),
            },
            ExportSpec {
                name: "bytes",
                thunk: ExportThunk::Fn1(thunk_consumers_bytes),
            },
            ExportSpec {
                name: "blob",
                thunk: ExportThunk::Fn1(thunk_consumers_blob),
            },
        ],
    },
    SubmoduleSpec {
        key: "sys",
        exports: &[
            ExportSpec {
                name: "format",
                thunk: ExportThunk::Fn1(thunk_sys_format),
            },
            ExportSpec {
                name: "inspect",
                thunk: ExportThunk::Fn1(thunk_sys_inspect),
            },
            ExportSpec {
                name: "debuglog",
                thunk: ExportThunk::Fn1(thunk_sys_debuglog),
            },
            ExportSpec {
                name: "deprecate",
                thunk: ExportThunk::Fn1(thunk_sys_deprecate),
            },
            ExportSpec {
                name: "promisify",
                thunk: ExportThunk::Fn1(thunk_sys_promisify),
            },
            ExportSpec {
                name: "callbackify",
                thunk: ExportThunk::Fn1(thunk_sys_callbackify),
            },
            ExportSpec {
                name: "isArray",
                thunk: ExportThunk::Fn1(thunk_sys_isArray),
            },
        ],
    },
    // #906 follow-up: pino reads `tracingChannel('pino_asJson')` at
    // module init time. The thunks here return useful stub values
    // (an object with `hasSubscribers: false`) instead of throwing,
    // so pino's "no subscribers → fast path" branch is taken and the
    // tracing machinery never enters.
    SubmoduleSpec {
        key: "diagnostics_channel",
        exports: &[
            ExportSpec {
                name: "tracingChannel",
                thunk: ExportThunk::Fn1(thunk_diag_tracing_channel),
            },
            ExportSpec {
                name: "channel",
                thunk: ExportThunk::Fn1(thunk_diag_channel),
            },
            ExportSpec {
                name: "subscribe",
                thunk: ExportThunk::Fn2(thunk_diag_subscribe),
            },
            ExportSpec {
                name: "unsubscribe",
                thunk: ExportThunk::Fn2(thunk_diag_unsubscribe),
            },
            ExportSpec {
                name: "publish",
                thunk: ExportThunk::Fn1(thunk_diag_noop),
            },
            ExportSpec {
                name: "hasSubscribers",
                thunk: ExportThunk::Fn1(thunk_diag_has_subscribers),
            },
            ExportSpec {
                name: "Channel",
                thunk: ExportThunk::Fn1(thunk_diag_noop),
            },
        ],
    },
];

fn find_submodule(key: &str) -> Option<&'static SubmoduleSpec> {
    SUBMODULES.iter().find(|s| s.key == key)
}

fn find_export(submod: &SubmoduleSpec, name: &str) -> Option<&'static ExportSpec> {
    submod.exports.iter().find(|e| e.name == name)
}

// ----- singleton storage -----
//
// One AtomicI64 slot per thunk so concurrent first-use callers don't
// leak a closure. Stored in a thread_local Vec for simplicity — these
// singletons are allocated on first reach and live until process exit
// (they're root-marked by `scan_node_submodule_singleton_roots` below).

thread_local! {
    /// Map from (submod_key_ptr, export_name_ptr) — both `&'static str`,
    /// so pointer-equality is sufficient — to the cached singleton
    /// ClosureHeader pointer for that export's thunk.
    static EXPORT_SINGLETONS: RefCell<std::collections::HashMap<(usize, usize), *mut ClosureHeader>> =
        RefCell::new(std::collections::HashMap::new());

    /// Map from submod_key_ptr to the cached namespace ObjectHeader
    /// pointer — populated once per submodule on first namespace use.
    static NAMESPACE_SINGLETONS: RefCell<std::collections::HashMap<usize, *mut ObjectHeader>> =
        RefCell::new(std::collections::HashMap::new());
}

// We also need a process-wide "any singleton allocated?" flag so the
// GC scanner can early-out without taking the thread_local borrow on
// every cycle. Using `AtomicI64` instead of `AtomicBool` so the scanner
// can also use it as a release fence against the thread_local writes.
static ANY_SINGLETON_ALLOCATED: AtomicI64 = AtomicI64::new(0);

fn ensure_export_singleton(
    submod: &'static SubmoduleSpec,
    export: &'static ExportSpec,
) -> *mut ClosureHeader {
    let key = (submod.key.as_ptr() as usize, export.name.as_ptr() as usize);
    if let Some(cached) = EXPORT_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    let thunk_ptr = export.thunk.as_ptr();
    let allocated = js_closure_alloc(thunk_ptr, 0);
    if submod.key == "diagnostics_channel" {
        let arity = match export.name {
            "subscribe" | "unsubscribe" => 2,
            "channel" | "hasSubscribers" | "tracingChannel" => 1,
            "publish" => 2,
            _ => 1,
        };
        js_register_closure_arity(thunk_ptr, arity);
    }
    // #1213: timers/promises.setTimeout takes (delay, value); registering the
    // arity makes the closure dispatch pad a missing `value` with undefined.
    if submod.key == "timers_promises" {
        let arity = match export.name {
            "setTimeout" => 2,
            "setImmediate" => 1,
            _ => 1,
        };
        js_register_closure_arity(thunk_ptr, arity);
    }
    EXPORT_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, allocated);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    allocated
}

fn ensure_namespace_singleton(submod: &'static SubmoduleSpec) -> *mut ObjectHeader {
    let key = submod.key.as_ptr() as usize;
    if let Some(cached) = NAMESPACE_SINGLETONS.with(|m| m.borrow().get(&key).copied()) {
        return cached;
    }
    // Allocate a fresh object with one inline slot per known export;
    // the dynamic-property path in `js_object_set_field_by_name` will
    // grow it if needed.
    let field_count = submod.exports.len() as u32;
    let obj = js_object_alloc(0, field_count);
    // Populate fields. Each export's value is the singleton closure
    // pointer NaN-boxed as POINTER. We route through
    // `js_object_set_field_by_name` so the keys array gets built up
    // identically to what user code's literal object init would
    // produce — that's what `js_object_keys` / spread / Reflect.ownKeys
    // walks at runtime.
    for spec in submod.exports {
        let closure_ptr = ensure_export_singleton(submod, spec);
        let value_bits = JSValue::pointer(closure_ptr as *const u8).bits();
        let value_f64 = f64::from_bits(value_bits);
        unsafe {
            let name_bytes = spec.name.as_bytes();
            let name_header = js_string_from_bytes(name_bytes.as_ptr(), name_bytes.len() as u32);
            crate::object::js_object_set_field_by_name(obj, name_header, value_f64);
        }
    }
    NAMESPACE_SINGLETONS.with(|m| {
        m.borrow_mut().insert(key, obj);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
    obj
}

/// GC root scanner: pin every (export-singleton, namespace-singleton)
/// allocated by this module against the next sweep. Wired up from
/// `gc::gc_init`.
pub fn scan_node_submodule_singleton_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_node_submodule_singleton_roots_mut(&mut visitor);
}

pub fn scan_node_submodule_singleton_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if ANY_SINGLETON_ALLOCATED.load(Ordering::Acquire) == 0 {
        return;
    }
    EXPORT_SINGLETONS.with(|m| {
        for closure_ptr in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(closure_ptr);
        }
    });
    NAMESPACE_SINGLETONS.with(|m| {
        for obj_ptr in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(obj_ptr);
        }
    });
    // #906 follow-up: the no-op closure shared by every TracingChannel /
    // Channel stub field also needs pinning against the next sweep. The
    // returned stub objects themselves are caller-owned (we don't cache
    // them) so they're traced through normal allocator roots.
    DIAG_NOOP_CLOSURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(ptr) = slot.as_mut() {
            visitor.visit_raw_mut_ptr_slot(ptr);
        }
    });
    DIAG_CHANNELS.with(|m| {
        for state in m.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.name);
            visitor.visit_raw_mut_ptr_slot(&mut state.obj);
            for subscriber in &mut state.subscribers {
                visitor.visit_nanbox_f64_slot(subscriber);
            }
            for (store, transform) in &mut state.stores {
                visitor.visit_nanbox_f64_slot(store);
                if let Some(t) = transform.as_mut() {
                    visitor.visit_nanbox_f64_slot(t);
                }
            }
        }
    });
    DIAG_TRACES.with(|m| {
        for trace in m.borrow_mut().values_mut() {
            visitor.visit_raw_mut_ptr_slot(&mut trace.obj);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_node_submodule_roots(
    closure: *mut ClosureHeader,
    namespace: *mut ObjectHeader,
    diag_noop: *mut ClosureHeader,
) {
    EXPORT_SINGLETONS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert((1, 2), closure);
    });
    NAMESPACE_SINGLETONS.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert(3, namespace);
    });
    DIAG_NOOP_CLOSURE.with(|slot| {
        *slot.borrow_mut() = Some(diag_noop);
    });
    ANY_SINGLETON_ALLOCATED.store(1, Ordering::Release);
}

#[cfg(test)]
pub(crate) fn test_node_submodule_roots() -> (usize, usize, usize) {
    let closure = EXPORT_SINGLETONS.with(|m| {
        m.borrow()
            .get(&(1, 2))
            .map(|ptr| *ptr as usize)
            .unwrap_or(0)
    });
    let namespace =
        NAMESPACE_SINGLETONS.with(|m| m.borrow().get(&3).map(|ptr| *ptr as usize).unwrap_or(0));
    let diag =
        DIAG_NOOP_CLOSURE.with(|slot| slot.borrow().as_ref().map(|ptr| *ptr as usize).unwrap_or(0));
    (closure, namespace, diag)
}

// ----- FFI entry points -----
//
// `submod_key_ptr` / `name_ptr` are `*const u8` pointers + lengths
// rather than NUL-terminated strings so codegen can hand off the raw
// bytes from emitted IR (already produced as `private constant
// [N x i8]` arrays via `emit_string_literal`).

/// Returns a NaN-boxed function singleton for the given
/// `(submodule, export)` pair. Falls back to NaN-boxed `TAG_TRUE`
/// (preserving the pre-#841 sentinel) if no matching entry is found —
/// this keeps any not-yet-listed export's behavior unchanged, so
/// later additions to `SUBMODULES` are strictly additive.
///
/// # Safety
///
/// The `submod_key_ptr` / `name_ptr` arguments must point to valid UTF-8
/// byte sequences of the indicated length, and remain alive for the
/// duration of this call.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_export_as_function(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
    name_ptr: *const u8,
    name_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let name_bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let name = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let export = match find_export(submod, name) {
        Some(e) => e,
        None => return f64::from_bits(JSValue::bool(true).bits()),
    };
    let closure_ptr = ensure_export_singleton(submod, export);
    f64::from_bits(JSValue::pointer(closure_ptr as *const u8).bits())
}

/// Returns a NaN-boxed namespace stub object for the given submodule.
/// Each known named export of that submodule is exposed as an own
/// property on the object whose value is the function singleton
/// produced by `js_node_submodule_export_as_function`. Falls back to
/// `js_unresolved_namespace_stub` (the empty-object stub Perry already
/// hands out for unknown namespace imports) if `submod_key` doesn't
/// match a known submodule.
///
/// # Safety
///
/// Same constraints as `js_node_submodule_export_as_function`.
#[no_mangle]
pub unsafe extern "C" fn js_node_submodule_namespace(
    submod_key_ptr: *const u8,
    submod_key_len: u32,
) -> f64 {
    let submod_bytes = std::slice::from_raw_parts(submod_key_ptr, submod_key_len as usize);
    let submod_key = match std::str::from_utf8(submod_bytes) {
        Ok(s) => s,
        Err(_) => return crate::object::js_unresolved_namespace_stub(),
    };
    let submod = match find_submodule(submod_key) {
        Some(s) => s,
        None => return crate::object::js_unresolved_namespace_stub(),
    };
    let obj = ensure_namespace_singleton(submod);
    f64::from_bits(JSValue::pointer(obj as *const u8).bits())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_submodules_have_at_least_one_export() {
        for s in SUBMODULES {
            assert!(
                !s.exports.is_empty(),
                "submodule {} has zero exports",
                s.key
            );
        }
    }

    #[test]
    fn find_submodule_for_known_keys() {
        for key in [
            "timers_promises",
            "readline_promises",
            "stream_promises",
            "stream_consumers",
            "sys",
            "diagnostics_channel",
        ] {
            assert!(
                find_submodule(key).is_some(),
                "submodule {} missing from SUBMODULES table",
                key
            );
        }
    }

    #[test]
    fn find_submodule_for_unknown_key_returns_none() {
        assert!(find_submodule("not_a_real_submodule").is_none());
    }

    /// #906 follow-up — pino reads `tracingChannel('pino_asJson').hasSubscribers`
    /// before deciding whether to enter the tracing branch. The stub MUST
    /// expose `tracingChannel` as a callable thunk in the SUBMODULES table
    /// so the namespace singleton's field is a function (not TAG_TRUE).
    #[test]
    fn diagnostics_channel_exposes_tracingChannel_export() {
        let submod = find_submodule("diagnostics_channel")
            .expect("diagnostics_channel must be in SUBMODULES");
        let names: Vec<&str> = submod.exports.iter().map(|e| e.name).collect();
        for required in ["tracingChannel", "channel", "subscribe", "unsubscribe"] {
            assert!(
                names.contains(&required),
                "diagnostics_channel must export `{}` for pino's `require('node:diagnostics_channel')` to keep working",
                required
            );
        }
    }
}
