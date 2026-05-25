//! node:diagnostics_channel + supporting helpers (channel registry,
//! subscribe/unsubscribe, tracing channels) + global error-code/syscall/path
//! side tables consumed by the OBJECT_TYPE_ERROR getters.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::closure::ClosureHeader;
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

use super::*;

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

pub(crate) extern "C" fn thunk_diag_noop(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

pub(crate) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(crate) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[derive(Hash, Eq, PartialEq, Clone)]
enum DiagChannelKey {
    String(String),
    Symbol(u64),
}

pub(crate) struct DiagChannelState {
    pub(crate) name: f64,
    pub(crate) obj: *mut ObjectHeader,
    pub(crate) subscribers: Vec<f64>,
    pub(crate) stores: Vec<(f64, Option<f64>)>,
}

pub(crate) struct DiagTracingState {
    pub(crate) obj: *mut ObjectHeader,
    pub(crate) events: [i64; 5],
}

// Known follow-ups for the thread-local state below:
//
// * #1309 — channels used to be pinned for the process lifetime. A soft
//   cap now evicts the oldest *inactive* channels (no subscribers, no
//   stores) once the map crosses `DIAG_CHANNEL_SOFT_CAP`, so a long-running
//   service that mints per-request channel names stays bounded (see
//   `evict_inactive_diag_channels_if_needed`). Full weak-collection (drop a
//   channel the moment no user reference and no subscriber remain) still
//   needs a GC post-sweep hook or a weak-ref primitive.
//
// * #1310 — these maps are `thread_local!`, so `parallelMap`/`spawn`
//   workers see an empty world. A `publish` from a worker thread
//   silently no-ops against subscribers registered on the main
//   thread, diverging from Node's process-global model.
thread_local! {
    pub(crate) static DIAG_CHANNEL_BY_KEY: RefCell<HashMap<DiagChannelKey, i64>> = RefCell::new(HashMap::new());
    pub(crate) static DIAG_CHANNELS: RefCell<HashMap<i64, DiagChannelState>> = RefCell::new(HashMap::new());
    pub(crate) static DIAG_TRACES: RefCell<HashMap<i64, DiagTracingState>> = RefCell::new(HashMap::new());
    pub(crate) static DIAG_PENDING_UNCAUGHT: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
    pub(crate) static DIAG_SUPPRESS_UNCAUGHT_DRAIN: RefCell<usize> = const { RefCell::new(0) };
    pub(crate) static NEXT_DIAG_ID: RefCell<i64> = const { RefCell::new(1) };
}

pub(crate) fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
pub(crate) fn bool_value(v: bool) -> f64 {
    f64::from_bits(if v { TAG_TRUE } else { TAG_FALSE })
}

pub(crate) fn boxed_ptr<T>(p: *const T) -> f64 {
    f64::from_bits(JSValue::pointer(p as *const u8).bits())
}

pub(crate) fn decode_string_value(value: f64) -> Option<String> {
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

pub(crate) fn channel_key(name: f64) -> Option<DiagChannelKey> {
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
    pub(crate) static ERROR_MESSAGE_CODES: RefCell<HashMap<usize, &'static str>> =
        RefCell::new(HashMap::new());
}

pub(crate) fn register_error_code(message_ptr: *const StringHeader, code: &'static str) {
    register_error_code_pub(message_ptr, code);
}

/// `register_error_code` for crate-external callers (e.g. `fs.rs` decorates
/// io::Error values with their POSIX `ERR_*` code so `err.code === "ENOENT"`
/// works on caught errors).
pub fn register_error_code_pub(message_ptr: *const StringHeader, code: &'static str) {
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

thread_local! {
    pub(crate) static ERROR_MESSAGE_SYSCALLS: RefCell<HashMap<usize, &'static str>> =
        RefCell::new(HashMap::new());
    pub(crate) static ERROR_MESSAGE_PATHS: RefCell<HashMap<usize, String>> =
        RefCell::new(HashMap::new());
}

/// Attach a Node-style `syscall` string to an Error keyed by its message
/// StringHeader, mirroring [`register_error_code_pub`]. Read back from the
/// `.syscall` getter in `field_get_set`.
pub fn register_error_syscall(message_ptr: *const StringHeader, syscall: &'static str) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_SYSCALLS.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, syscall);
    });
}

pub fn error_syscall_for_message(message_ptr: *const StringHeader) -> Option<&'static str> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_SYSCALLS.with(|m| m.borrow().get(&(message_ptr as usize)).copied())
}

/// Attach a Node-style `path` string to an Error keyed by its message
/// StringHeader. The value is owned (paths are runtime data, not static).
pub fn register_error_path(message_ptr: *const StringHeader, path: String) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_PATHS.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, path);
    });
}

pub fn error_path_for_message(message_ptr: *const StringHeader) -> Option<String> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_PATHS.with(|m| m.borrow().get(&(message_ptr as usize)).cloned())
}

pub(crate) fn throw_invalid_arg() -> ! {
    let msg = b"The argument is invalid";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

pub(crate) fn throw_type_error_no_code(message: &[u8]) -> ! {
    let s = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

pub(crate) fn next_diag_id() -> i64 {
    NEXT_DIAG_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    })
}

pub(crate) fn valid_closure_value(v: f64) -> bool {
    let raw = crate::value::js_nanbox_get_pointer(v) as usize;
    raw >= 0x10000 && crate::closure::is_closure_ptr(raw)
}

pub(crate) fn closure_ptr(v: f64) -> *const ClosureHeader {
    crate::value::js_nanbox_get_pointer(v) as *const ClosureHeader
}

pub(crate) fn set_field_value(obj: *mut ObjectHeader, name: &str, value: f64) {
    unsafe {
        let key = js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
        js_object_set_field_by_name(obj, key, value);
    }
}

pub(crate) fn get_field_value(obj: *mut ObjectHeader, name: &str) -> f64 {
    unsafe {
        let key = js_string_from_bytes(name.as_bytes().as_ptr(), name.len() as u32);
        js_object_get_field_by_name_f64(obj, key)
    }
}

#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast0(f: extern "C" fn(*const ClosureHeader) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast1(f: extern "C" fn(*const ClosureHeader, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast2(f: extern "C" fn(*const ClosureHeader, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast3(f: extern "C" fn(*const ClosureHeader, f64, f64, f64) -> f64) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast4(
    f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64) -> f64,
) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast5(
    f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64) -> f64,
) -> *const u8 {
    f as *const u8
}
#[allow(clippy::missing_transmute_annotations)]
pub(crate) fn cast7(
    f: extern "C" fn(*const ClosureHeader, f64, f64, f64, f64, f64, f64, f64) -> f64,
) -> *const u8 {
    f as *const u8
}

pub(crate) fn method_closure(func: *const u8, arity: u32, id: i64) -> f64 {
    let c = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(c, 0, id);
    js_register_closure_arity(func, arity);
    boxed_ptr(c)
}

pub(crate) fn method_id(closure: *const ClosureHeader) -> i64 {
    js_closure_get_capture_ptr(closure, 0)
}

pub(crate) fn catch_js<F: FnOnce() -> f64>(f: F) -> Result<f64, f64> {
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

pub(crate) extern "C" fn throw_captured_error(closure: *const ClosureHeader) -> f64 {
    let err = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    crate::exception::js_throw(err)
}

pub(crate) fn schedule_uncaught(err: f64) {
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

pub(crate) fn suppress_uncaught_drain<F: FnOnce() -> f64>(f: F) -> f64 {
    DIAG_SUPPRESS_UNCAUGHT_DRAIN.with(|n| *n.borrow_mut() += 1);
    let result = f();
    DIAG_SUPPRESS_UNCAUGHT_DRAIN.with(|n| {
        let mut n = n.borrow_mut();
        *n = n.saturating_sub(1);
    });
    result
}

pub(crate) fn with_implicit_this<F: FnOnce() -> f64>(this_arg: f64, f: F) -> f64 {
    let prev = crate::object::js_implicit_this_set(this_arg);
    let result = f();
    crate::object::js_implicit_this_set(prev);
    result
}

pub(crate) fn update_channel_active(id: i64) {
    DIAG_CHANNELS.with(|channels| {
        if let Some(ch) = channels.borrow_mut().get_mut(&id) {
            let active = !ch.subscribers.is_empty() || !ch.stores.is_empty();
            set_field_value(ch.obj, "hasSubscribers", bool_value(active));
        }
    });
    update_all_tracing_active();
}

pub(crate) fn update_all_tracing_active() {
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

// #1309: soft cap on the number of live diagnostics channels. Node holds
// channels weakly — a channel with no subscribers, no bound stores, and no
// user reference is collectible. Perry can't observe "no user reference"
// without GC integration, but the unbounded-growth case the issue describes
// is a long-running service minting per-request channel names that nobody
// subscribes to (`dc.channel(`req-${id}`)`). When the map crosses the cap we
// drop the oldest *inactive* channels (no subscribers, no stores) so memory
// stays bounded. The cap is generous enough that normal programs (a handful
// of stable channel names) never trigger eviction; active channels are never
// evicted, so subscribed/published flows are unaffected.
const DIAG_CHANNEL_SOFT_CAP: usize = 8192;
const DIAG_CHANNEL_EVICT_BATCH: usize = 2048;

/// Drop the oldest inactive channels when the live-channel map crosses the
/// soft cap (#1309). Eviction happens in batches so the O(n) scan amortizes
/// across many `channel(name)` calls. Ids are monotonic, so the lowest ids
/// are the oldest. An evicted name simply re-allocates a fresh channel on the
/// next `channel(name)` — correct for the unsubscribed per-request pattern;
/// a still-held channel object whose name was evicted would dispatch against
/// a dropped id (a rare divergence accepted for the leak fix).
fn evict_inactive_diag_channels_if_needed() {
    let len = DIAG_CHANNELS.with(|m| m.borrow().len());
    if len < DIAG_CHANNEL_SOFT_CAP {
        return;
    }
    let mut victims: Vec<(i64, f64)> = DIAG_CHANNELS.with(|m| {
        let m = m.borrow();
        let mut v: Vec<(i64, f64)> = m
            .iter()
            .filter(|(_, s)| s.subscribers.is_empty() && s.stores.is_empty())
            .map(|(id, s)| (*id, s.name))
            .collect();
        v.sort_unstable_by_key(|(id, _)| *id);
        v
    });
    victims.truncate(DIAG_CHANNEL_EVICT_BATCH);
    for (id, name) in victims {
        DIAG_CHANNELS.with(|m| {
            m.borrow_mut().remove(&id);
        });
        if let Some(key) = channel_key(name) {
            DIAG_CHANNEL_BY_KEY.with(|m| {
                let mut map = m.borrow_mut();
                // Only drop the name→id mapping if it still points at the
                // channel we're evicting (a re-created channel would have
                // overwritten it with a new id).
                if map.get(&key).copied() == Some(id) {
                    map.remove(&key);
                }
            });
        }
    }
}

pub(crate) fn ensure_channel(name: f64) -> i64 {
    let key = channel_key(name).unwrap_or_else(|| throw_invalid_arg());
    if let Some(id) = DIAG_CHANNEL_BY_KEY.with(|m| m.borrow().get(&key).copied()) {
        return id;
    }
    evict_inactive_diag_channels_if_needed();
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

pub(crate) fn channel_obj(id: i64) -> f64 {
    DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|c| boxed_ptr(c.obj))
            .unwrap_or_else(undefined)
    })
}

pub(crate) fn add_subscriber(id: i64, subscriber: f64) {
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

pub(crate) fn remove_subscriber(id: i64, subscriber: f64) -> bool {
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

pub(crate) fn publish_channel(id: i64, data: f64) {
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

pub(crate) fn console_channel_slot(method: &str) -> Option<(&'static AtomicI64, &'static str)> {
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

pub(crate) fn run_store_wrapped(
    id: i64,
    data: f64,
    fn_value: f64,
    this_arg: f64,
    args: &[f64],
) -> f64 {
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

pub(crate) extern "C" fn diag_channel_subscribe(
    closure: *const ClosureHeader,
    subscriber: f64,
) -> f64 {
    add_subscriber(method_id(closure), subscriber);
    undefined()
}

pub(crate) extern "C" fn diag_channel_unsubscribe(
    closure: *const ClosureHeader,
    subscriber: f64,
) -> f64 {
    bool_value(remove_subscriber(method_id(closure), subscriber))
}

pub(crate) extern "C" fn diag_channel_publish(closure: *const ClosureHeader, data: f64) -> f64 {
    publish_channel(method_id(closure), data);
    undefined()
}

pub(crate) extern "C" fn diag_channel_bind_store(
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

pub(crate) extern "C" fn diag_channel_unbind_store(
    closure: *const ClosureHeader,
    store: f64,
) -> f64 {
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

pub(crate) fn call_store_run(store: f64, context: f64, next: f64) -> f64 {
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

pub(crate) extern "C" fn store_next_thunk(closure: *const ClosureHeader) -> f64 {
    let id = js_closure_get_capture_ptr(closure, 0);
    let data = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let fn_value = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    let this_arg = f64::from_bits(js_closure_get_capture_ptr(closure, 3) as u64);
    let a = f64::from_bits(js_closure_get_capture_ptr(closure, 4) as u64);
    let b = f64::from_bits(js_closure_get_capture_ptr(closure, 5) as u64);
    run_store_wrapped(id, data, fn_value, this_arg, &[a, b])
}

pub(crate) extern "C" fn store_chain_thunk(closure: *const ClosureHeader) -> f64 {
    let store = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let context = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let next = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    call_store_run(store, context, next)
}

pub(crate) extern "C" fn diag_channel_run_stores(
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

pub(crate) extern "C" fn thunk_diag_channel(closure: *const ClosureHeader, name: f64) -> f64 {
    let _ = closure;
    channel_obj(ensure_channel(name))
}

pub(crate) extern "C" fn thunk_diag_subscribe(
    _closure: *const ClosureHeader,
    name: f64,
    subscriber: f64,
) -> f64 {
    let id = ensure_channel(name);
    add_subscriber(id, subscriber);
    undefined()
}

pub(crate) extern "C" fn thunk_diag_unsubscribe(
    _closure: *const ClosureHeader,
    name: f64,
    subscriber: f64,
) -> f64 {
    let id = ensure_channel(name);
    bool_value(remove_subscriber(id, subscriber))
}

pub(crate) extern "C" fn thunk_diag_has_subscribers(
    _closure: *const ClosureHeader,
    name: f64,
) -> f64 {
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

pub(crate) fn tracing_event_name(base: f64, event: &str) -> f64 {
    let base = decode_string_value(base).unwrap_or_else(|| "unknown".to_string());
    let s = format!("tracing:{base}:{event}");
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::STRING_TAG | (ptr as u64 & crate::value::POINTER_MASK))
}

pub(crate) fn channel_from_object_property(obj_value: f64, prop: &str) -> i64 {
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

pub(crate) extern "C" fn diag_trace_subscribe(closure: *const ClosureHeader, handlers: f64) -> f64 {
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

pub(crate) extern "C" fn diag_trace_unsubscribe(
    closure: *const ClosureHeader,
    handlers: f64,
) -> f64 {
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

pub(crate) fn call_fn_value(fn_value: f64, this_arg: f64, args: &[f64]) -> f64 {
    if !valid_closure_value(fn_value) {
        crate::closure::throw_not_callable();
    }
    let rebound = crate::closure::clone_closure_rebind_this(fn_value.to_bits(), this_arg);
    let cb = (rebound & crate::value::POINTER_MASK) as *const ClosureHeader;
    with_implicit_this(this_arg, || unsafe {
        js_closure_call_array(cb as i64, args.as_ptr(), args.len() as i64)
    })
}

pub(crate) extern "C" fn diag_trace_sync(
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

pub(crate) extern "C" fn diag_trace_promise(
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

pub(crate) extern "C" fn diag_trace_callback(
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

pub(crate) extern "C" fn thunk_diag_tracing_channel(
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
    pub(crate) static DIAG_NOOP_CLOSURE: RefCell<Option<*mut ClosureHeader>> = const { RefCell::new(None) };
}

pub(crate) fn ensure_diag_noop_closure() -> *mut ClosureHeader {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn inactive_state() -> DiagChannelState {
        DiagChannelState {
            name: 0.0,
            obj: std::ptr::null_mut(),
            subscribers: Vec::new(),
            stores: Vec::new(),
        }
    }

    // #1309: crossing the soft cap evicts a batch of the oldest inactive
    // channels so the live-channel map stays bounded.
    #[test]
    fn diag_channels_capped_by_evicting_inactive() {
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
        DIAG_CHANNEL_BY_KEY.with(|m| m.borrow_mut().clear());
        for _ in 0..DIAG_CHANNEL_SOFT_CAP + 100 {
            let id = next_diag_id();
            DIAG_CHANNELS.with(|m| {
                m.borrow_mut().insert(id, inactive_state());
            });
        }
        evict_inactive_diag_channels_if_needed();
        let len = DIAG_CHANNELS.with(|m| m.borrow().len());
        assert!(len <= DIAG_CHANNEL_SOFT_CAP, "expected <= cap, got {len}");
        assert!(
            len >= DIAG_CHANNEL_SOFT_CAP - DIAG_CHANNEL_EVICT_BATCH,
            "should evict at most one batch, got {len}"
        );
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
    }

    // #1309: a subscribed (active) channel is never evicted, even when the
    // map is over the cap.
    #[test]
    fn active_diag_channel_survives_eviction() {
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
        DIAG_CHANNEL_BY_KEY.with(|m| m.borrow_mut().clear());
        let active_id = next_diag_id();
        DIAG_CHANNELS.with(|m| {
            let mut s = inactive_state();
            s.subscribers.push(1.0);
            m.borrow_mut().insert(active_id, s);
        });
        for _ in 0..DIAG_CHANNEL_SOFT_CAP + 100 {
            let id = next_diag_id();
            DIAG_CHANNELS.with(|m| {
                m.borrow_mut().insert(id, inactive_state());
            });
        }
        evict_inactive_diag_channels_if_needed();
        assert!(
            DIAG_CHANNELS.with(|m| m.borrow().contains_key(&active_id)),
            "subscribed channel must not be evicted"
        );
        DIAG_CHANNELS.with(|m| m.borrow_mut().clear());
    }
}
