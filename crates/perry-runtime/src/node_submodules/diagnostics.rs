//! node:diagnostics_channel + supporting helpers (channel registry,
//! subscribe/unsubscribe, tracing channels) + global error-code/syscall/path
//! side tables consumed by the OBJECT_TYPE_ERROR getters.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};
use std::thread::ThreadId;

use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::{
    js_closure_get_capture_f64, js_closure_set_capture_f64,
    js_register_closure_synthetic_arguments, ClosureHeader,
};
use crate::object::ObjectHeader;
use crate::string::{js_string_from_bytes, StringHeader};
use crate::thread::SerializedValue;
use crate::value::{js_nanbox_get_pointer, JSValue};

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
// Other entries (`subscribe`, `unsubscribe`, `hasSubscribers`)
// surface as no-op thrower thunks the same way the other submodules do —
// real-tracing semantics are a follow-up under #793.

pub(crate) extern "C" fn thunk_diag_noop(_closure: *const ClosureHeader, _arg: f64) -> f64 {
    f64::from_bits(crate::value::JSValue::undefined().bits())
}

pub(crate) const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
pub(crate) const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;

#[derive(Hash, Eq, PartialEq, Clone)]
pub(crate) enum DiagChannelKey {
    String(String),
    Symbol(u64),
}

/// The `transform` argument remembered by `bindStore(store, transform)`.
///
/// Node distinguishes three cases (#3085): an omitted/`undefined`/`null`
/// transform (the store context is just the published `data`), a callable
/// transform (its return value becomes the context), and a *non-callable,
/// non-nullish* transform value (a number, a string, …). The last case is
/// retained — during `runStores` Node attempts to call it, fails, runs the
/// callback with no store context, and reports an uncaught
/// `TypeError: transform is not a function`. Discarding it (treating it like
/// `undefined`) was the bug.
#[derive(Clone, Copy)]
pub(crate) enum StoreTransform {
    /// No transform: context is the published `data` value.
    None,
    /// A callable transform closure.
    Callable(f64),
    /// A bound but non-callable transform value (triggers the Node TypeError).
    NonCallable,
}

pub(crate) struct DiagChannelState {
    pub(crate) name: f64,
    pub(crate) obj: *mut ObjectHeader,
    pub(crate) subscribers: Vec<f64>,
    pub(crate) stores: Vec<(f64, StoreTransform)>,
}

pub(crate) struct DiagTracingState {
    pub(crate) obj: *mut ObjectHeader,
    pub(crate) events: [i64; 5],
}

/// The event-loop/main thread is the only thread that may register
/// diagnostics subscribers/stores. `perry/thread::spawn` workers are
/// temporary runtimes with independent arenas and no persistent pump after
/// their closure returns, so keeping worker-owned callbacks would leave no
/// safe thread/lifetime for later cross-thread delivery (#1798).
static DIAG_MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

pub fn diagnostics_channel_init_main_thread() {
    let _ = DIAG_MAIN_THREAD.set(std::thread::current().id());
}

fn is_diagnostics_main_thread() -> bool {
    DIAG_MAIN_THREAD
        .get()
        .is_some_and(|main| *main == std::thread::current().id())
}

fn ensure_subscriber_owner_thread() {
    if is_diagnostics_main_thread() {
        return;
    }
    throw_type_error_no_code(
        b"node:diagnostics_channel subscribers are only supported on Perry's main thread",
    );
}

/// Cross-thread activity count keyed by channel name.
///
/// The concrete JS channel objects and subscriber closures remain
/// thread-local because they point into the allocating thread's arena. This
/// map intentionally stores only Rust-owned keys + counts, so workers can
/// observe process-global subscriber state without touching foreign JS
/// pointers.
static DIAG_GLOBAL_ACTIVE_COUNTS: LazyLock<Mutex<HashMap<DiagChannelKey, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Publish events produced by `perry/thread` workers for subscribers owned
/// by the main/event-loop thread. The payload is arena-independent; it is
/// deserialized only when the event-loop pump drains this queue.
static DIAG_PENDING_PUBLISHES: LazyLock<Mutex<Vec<PendingDiagPublish>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

struct PendingDiagPublish {
    key: DiagChannelKey,
    data: SerializedValue,
    origin_thread: ThreadId,
    local_delivered: bool,
}

fn cross_thread_key(key: &DiagChannelKey) -> Option<DiagChannelKey> {
    match key {
        DiagChannelKey::String(_) => Some(key.clone()),
        // Symbols are arena/runtime objects in Perry today. Treat them as
        // thread-local until the runtime has a real cross-thread symbol table.
        DiagChannelKey::Symbol(_) => None,
    }
}

fn global_active_count(key: &DiagChannelKey) -> usize {
    let Some(key) = cross_thread_key(key) else {
        return 0;
    };
    DIAG_GLOBAL_ACTIVE_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&key)
        .copied()
        .unwrap_or(0)
}

fn adjust_global_active_count_for_name(name: f64, delta: isize) {
    let Some(key) = channel_key(name).and_then(|k| cross_thread_key(&k)) else {
        return;
    };
    let mut counts = DIAG_GLOBAL_ACTIVE_COUNTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = counts.entry(key.clone()).or_insert(0);
    if delta >= 0 {
        *entry = entry.saturating_add(delta as usize);
    } else {
        *entry = entry.saturating_sub(delta.unsigned_abs());
    }
    if *entry == 0 {
        counts.remove(&key);
    }
}

fn adjust_global_active_count_for_channel(id: i64, delta: isize) {
    let name = DIAG_CHANNELS.with(|m| m.borrow().get(&id).map(|c| c.name));
    if let Some(name) = name {
        adjust_global_active_count_for_name(name, delta);
    }
}

fn enqueue_cross_thread_publish(key: DiagChannelKey, data: f64, local_delivered: bool) {
    let Some(key) = cross_thread_key(&key) else {
        return;
    };
    let data = unsafe { crate::thread::serialize_nanbox_for_thread(data.to_bits()) };
    let origin_thread = std::thread::current().id();
    {
        let mut pending = DIAG_PENDING_PUBLISHES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.push(PendingDiagPublish {
            key,
            data,
            origin_thread,
            local_delivered,
        });
    }
    crate::event_pump::js_notify_main_thread();
}

pub fn diagnostics_channel_has_pending_publishes() -> bool {
    !DIAG_PENDING_PUBLISHES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .is_empty()
}

/// Drain worker-originated diagnostics publishes on the current event-loop
/// thread. This must be called from a thread whose local diagnostics registry
/// owns the subscriber closures; in the normal executable this is the main
/// thread's microtask/event-loop pump.
pub fn diagnostics_channel_process_pending() -> i32 {
    let current_thread = std::thread::current().id();
    let pending = {
        let mut pending = DIAG_PENDING_PUBLISHES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *pending)
    };
    let mut delivered = 0i32;
    let mut retained = Vec::new();
    for item in pending {
        if item.local_delivered && item.origin_thread == current_thread {
            // The publishing thread already invoked its local subscribers
            // synchronously. Dropping this copy prevents a later same-thread
            // pump from delivering the event twice; any cross-thread delivery
            // remains the responsibility of another thread's pump.
            continue;
        }
        let id = DIAG_CHANNEL_BY_KEY.with(|m| m.borrow().get(&item.key).copied());
        let Some(id) = id else {
            // This thread does not own a channel object for the publish. Keep
            // the item queued so a later main/event-loop-thread drain can
            // deliver it instead of silently swallowing worker-originated
            // publishes.
            retained.push(item);
            continue;
        };
        let data = unsafe { crate::thread::deserialize_nanbox_on_current_thread(&item.data) };
        publish_channel_local(id, f64::from_bits(data));
        delivered += 1;
    }
    if !retained.is_empty() {
        let mut pending = DIAG_PENDING_PUBLISHES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Preserve retained items ahead of publishes that arrived while this
        // drain was running. There is no total ordering guarantee between
        // producer threads; keeping undelivered items at the front avoids
        // starving an older event that was merely observed by the wrong
        // thread first.
        retained.append(&mut *pending);
        *pending = retained;
    }
    delivered
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
// * #1310 — the JS-owning maps remain `thread_local!`, but worker publishes
//   now enqueue arena-independent payloads for the main/event-loop thread
//   when a process-global subscriber count exists for a string channel.
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

/// True when `value` is a JS Symbol. `channel(symbol)` accepts symbols, but
/// `tracingChannel(nameOrChannels)` rejects them (#3084) — Node's validator
/// only allows a string name or a channel-object map there.
fn is_symbol_value(value: f64) -> bool {
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    tag == crate::value::POINTER_TAG && unsafe { crate::symbol::js_is_symbol(value) } != 0
}

/// Throw the Node `ERR_INVALID_ARG_TYPE` that `tracingChannel(symbol)` raises:
/// `The "nameOrChannels" argument must be of type string or an instance of
/// TracingChannel or Object. Received type symbol (Symbol(<desc>))`.
fn throw_tracing_channel_symbol(value: f64) -> ! {
    let desc = unsafe { crate::symbol::js_symbol_description(value) };
    let inner = decode_string_value(desc).unwrap_or_default();
    let msg = format!(
        "The \"nameOrChannels\" argument must be of type string or an instance \
         of TracingChannel or Object. Received type symbol (Symbol({inner}))"
    );
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

pub(crate) fn diagnostics_channel_is_channel_instance_value(value: f64) -> bool {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return false;
    }
    let ptr = js_value.as_pointer::<ObjectHeader>();
    DIAG_CHANNELS.with(|channels| {
        channels
            .borrow()
            .values()
            .any(|channel| std::ptr::eq(channel.obj, ptr))
    })
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
    pub(crate) static ERROR_MESSAGE_ERRNOS: RefCell<HashMap<usize, i32>> =
        RefCell::new(HashMap::new());
    pub(crate) static ERROR_MESSAGE_PATHS: RefCell<HashMap<usize, String>> =
        RefCell::new(HashMap::new());
    pub(crate) static ERROR_MESSAGE_DESTS: RefCell<HashMap<usize, String>> =
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

/// Attach a Node-style negative libuv errno to an Error keyed by its message
/// StringHeader. Read back from the `.errno` getter in `field_get_set`.
pub fn register_error_errno(message_ptr: *const StringHeader, errno: i32) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_ERRNOS.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, errno);
    });
}

pub fn error_errno_for_message(message_ptr: *const StringHeader) -> Option<i32> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_ERRNOS.with(|m| m.borrow().get(&(message_ptr as usize)).copied())
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

thread_local! {
    pub(crate) static ERROR_MESSAGE_HOSTNAMES: RefCell<HashMap<usize, String>> =
        RefCell::new(HashMap::new());
}

/// Attach a Node-style `hostname` string to an Error keyed by its message
/// StringHeader. Node's c-ares `dns.resolve*`/`dns.reverse` failures carry the
/// queried hostname (or address) on `err.hostname`. Read back from the
/// `.hostname` getter in `field_get_set` (mirrors `.path`).
pub fn register_error_hostname(message_ptr: *const StringHeader, hostname: String) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_HOSTNAMES.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, hostname);
    });
}

pub fn error_hostname_for_message(message_ptr: *const StringHeader) -> Option<String> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_HOSTNAMES.with(|m| m.borrow().get(&(message_ptr as usize)).cloned())
}

/// Attach a Node-style `dest` string to an Error keyed by its message
/// StringHeader. Node sets `dest` on two-path fs errors (rename/copyFile/
/// link/symlink) alongside `path`. Read back from the `.dest` getter.
pub fn register_error_dest(message_ptr: *const StringHeader, dest: String) {
    if message_ptr.is_null() {
        return;
    }
    ERROR_MESSAGE_DESTS.with(|m| {
        m.borrow_mut().insert(message_ptr as usize, dest);
    });
}

pub fn error_dest_for_message(message_ptr: *const StringHeader) -> Option<String> {
    if message_ptr.is_null() {
        return None;
    }
    ERROR_MESSAGE_DESTS.with(|m| m.borrow().get(&(message_ptr as usize)).cloned())
}

/// A user-assigned own property value on an `Error` object. String values are
/// stored as an owned `String` (GC-safe — reconstructed into a fresh
/// `StringHeader` on read, exactly like [`ERROR_MESSAGE_PATHS`]); everything
/// else is stored as raw NaN-box bits. Immediates (number/bool/null/undefined)
/// carry no live heap reference so this is fully safe for the common cases
/// (`err.code = "X"`, `err.errno = -2`). Heap-object-valued props are stored
/// by bits as a best-effort and may not survive a GC move of the referent;
/// errors rarely carry object-valued own properties.
#[derive(Clone)]
pub enum ErrUserProp {
    Str(String),
    Bits(u64),
}

thread_local! {
    /// User-assigned own properties on `Error` objects, keyed by the error
    /// object pointer.
    ///
    /// `ErrorHeader` is a fixed `#[repr(C)]` struct with no overflow-field
    /// region, so a plain `err.foo = bar` had nowhere to land: the object
    /// setter dropped it and the getter returned `undefined`. That broke
    /// Node parity — e.g. `assert.throws(fn, { code })` could not read a
    /// user-assigned `.code` (#2014). This side table gives errors arbitrary
    /// string/primitive own properties. Stale entries after a GC move of the
    /// error are harmless (same model as the message-keyed tables above): a
    /// lookup at the new address simply misses.
    pub(crate) static ERROR_USER_PROPS: RefCell<HashMap<usize, HashMap<String, ErrUserProp>>> =
        RefCell::new(HashMap::new());
}

unsafe fn error_user_prop_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value);
    if ptr.is_null() {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
}

/// Record a user-assigned own property on an `Error` object. `error_ptr` is
/// the `ErrorHeader` pointer (the NaN-box pointer payload). Called from the
/// `GC_TYPE_ERROR` branch of `js_object_set_field_by_name`.
pub fn set_error_user_prop(error_ptr: usize, key: &str, value: f64) {
    if error_ptr == 0 {
        return;
    }
    let stored = if JSValue::from_bits(value.to_bits()).is_any_string() {
        ErrUserProp::Str(unsafe { error_user_prop_string(value) })
    } else {
        ErrUserProp::Bits(value.to_bits())
    };
    ERROR_USER_PROPS.with(|m| {
        m.borrow_mut()
            .entry(error_ptr)
            .or_default()
            .insert(key.to_string(), stored);
    });
}

/// Look up a user-assigned own property on an `Error` object, materialising it
/// back into a NaN-boxed `f64`. Returns `None` if no such property was set.
/// Called from the `GC_TYPE_ERROR` branch of `js_object_get_field_by_name`.
pub fn error_user_prop(error_ptr: usize, key: &str) -> Option<f64> {
    if error_ptr == 0 {
        return None;
    }
    ERROR_USER_PROPS.with(|m| {
        m.borrow().get(&error_ptr).and_then(|props| {
            props.get(key).map(|v| match v {
                ErrUserProp::Str(s) => {
                    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    f64::from_bits(crate::js_nanbox_string(ptr as i64).to_bits())
                }
                ErrUserProp::Bits(b) => f64::from_bits(*b),
            })
        })
    })
}

/// Remove a user-assigned own property from an Error object. Returns true
/// when the property existed (used by `delete err.prop` and data↔accessor
/// descriptor conversions).
pub fn remove_error_user_prop(error_ptr: usize, key: &str) -> bool {
    if error_ptr == 0 {
        return false;
    }
    ERROR_USER_PROPS.with(|m| {
        m.borrow_mut()
            .get_mut(&error_ptr)
            .map(|props| props.remove(key).is_some())
            .unwrap_or(false)
    })
}

/// Return user-assigned own properties on an Error object as materialized JS
/// values so util.inspect/console formatting can show them.
pub fn error_user_props(error_ptr: usize) -> Vec<(String, f64)> {
    if error_ptr == 0 {
        return Vec::new();
    }
    let props: Vec<(String, ErrUserProp)> = ERROR_USER_PROPS.with(|m| {
        m.borrow()
            .get(&error_ptr)
            .map(|props| {
                props
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    });
    let mut props: Vec<(String, f64)> = props
        .into_iter()
        .map(|(key, value)| {
            let materialized = match value {
                ErrUserProp::Str(s) => {
                    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
                    f64::from_bits(crate::js_nanbox_string(ptr as i64).to_bits())
                }
                ErrUserProp::Bits(bits) => f64::from_bits(bits),
            };
            (key, materialized)
        })
        .collect();
    props.sort_by(|a, b| a.0.cmp(&b.0));
    props
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

/// Build (but do not throw) the `TypeError: transform is not a function`
/// error that a non-callable `bindStore` transform produces during
/// `runStores`. Returned as a NaN-boxed value so it can be scheduled as an
/// uncaught exception rather than thrown synchronously (#3085).
pub(crate) fn make_transform_not_a_function_error() -> f64 {
    let msg = b"transform is not a function";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    boxed_ptr(err)
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

// #854: diagnostics_channel captured-error helper retained for the subsystem
#[allow(dead_code)]
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
            let active = !ch.subscribers.is_empty()
                || !ch.stores.is_empty()
                || channel_key(ch.name)
                    .as_ref()
                    .is_some_and(|key| global_active_count(key) > 0);
            set_field_value(ch.obj, "hasSubscribers", bool_value(active));
        }
    });
    update_all_tracing_active();
    super::diagnostics_tail::update_all_bounded_active();
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
    let obj = js_object_alloc(0, 11);
    let has_global_subscribers = global_active_count(&key) > 0;
    set_field_value(obj, "name", name);
    set_field_value(obj, "hasSubscribers", bool_value(has_global_subscribers));
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
    set_field_value(obj, "runStores", run_stores_method_closure(id));
    set_field_value(
        obj,
        "withStoreScope",
        method_closure(
            cast1(super::diagnostics_tail::diag_channel_with_store_scope),
            1,
            id,
        ),
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
    ensure_subscriber_owner_thread();
    if !valid_closure_value(subscriber) {
        throw_invalid_arg();
    }
    let mut inserted = false;
    DIAG_CHANNELS.with(|m| {
        if let Some(c) = m.borrow_mut().get_mut(&id) {
            c.subscribers.push(subscriber);
            inserted = true;
        }
    });
    if inserted {
        adjust_global_active_count_for_channel(id, 1);
    }
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
        adjust_global_active_count_for_channel(id, -1);
        update_channel_active(id);
    }
    removed
}

pub(crate) fn publish_channel_local(id: i64, data: f64) {
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

pub(crate) fn publish_channel(id: i64, data: f64) {
    let (name, local_active_count, has_local_subscribers) = DIAG_CHANNELS.with(|m| {
        let m = m.borrow();
        match m.get(&id) {
            Some(c) => (
                c.name,
                c.subscribers.len().saturating_add(c.stores.len()),
                !c.subscribers.is_empty(),
            ),
            None => (undefined(), 0, false),
        }
    });
    if has_local_subscribers {
        publish_channel_local(id, data);
    }
    let Some(key) = channel_key(name) else {
        return;
    };
    if global_active_count(&key) > local_active_count {
        enqueue_cross_thread_publish(key, data, has_local_subscribers);
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
    ensure_subscriber_owner_thread();
    let id = method_id(closure);
    // Omitted, explicit `undefined`, and `null` are all the no-transform
    // case in current Node: the store context is the data value. Other
    // non-callables are retained so `runStores` reports the uncaught
    // `TypeError: transform is not a function` after running the callback
    // with that store unset.
    let transform =
        if transform.to_bits() == TAG_UNDEFINED || transform.to_bits() == crate::value::TAG_NULL {
            StoreTransform::None
        } else if valid_closure_value(transform) {
            StoreTransform::Callable(transform)
        } else {
            StoreTransform::NonCallable
        };
    let mut inserted = false;
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
                inserted = true;
            }
        }
    });
    if inserted {
        adjust_global_active_count_for_channel(id, 1);
    }
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
        adjust_global_active_count_for_channel(id, -1);
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
    // Capture slot 4 holds a NaN-boxed JS array of the trailing callback
    // arguments (everything after `thisArg` in `runStores`), so the full
    // `...args` list is forwarded — not just the first two (#3082).
    let cb_args = f64::from_bits(js_closure_get_capture_ptr(closure, 4) as u64);
    let args = unbox_arg_array(cb_args);
    run_store_wrapped(id, data, fn_value, this_arg, &args)
}

pub(crate) extern "C" fn store_chain_thunk(closure: *const ClosureHeader) -> f64 {
    let store = f64::from_bits(js_closure_get_capture_ptr(closure, 0) as u64);
    let context = f64::from_bits(js_closure_get_capture_ptr(closure, 1) as u64);
    let next = f64::from_bits(js_closure_get_capture_ptr(closure, 2) as u64);
    call_store_run(store, context, next)
}

/// Build the `runStores` method closure. Registered as a synthetic-arguments
/// rest closure (fixed_arity 0) so the dispatcher bundles the FULL argument
/// list — `context`, `callback`, `thisArg`, and every trailing `...args` — into
/// a single JS array, regardless of how many arguments the caller passed
/// (#3082). The fixed five-argument `cast5` entrypoint used previously capped
/// the forwarded callback arguments at two.
pub(crate) fn run_stores_method_closure(id: i64) -> f64 {
    let func = cast1(diag_channel_run_stores);
    let c = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(c, 0, id);
    js_register_closure_synthetic_arguments(func, 0);
    boxed_ptr(c)
}

/// Build the `traceCallback` method closure. Like `runStores`, registered as a
/// synthetic-arguments closure so the FULL argument list — `fn`, `position`,
/// `context`, `thisArg`, and every trailing `...args` — is bundled into one JS
/// array, letting `traceCallback` honor `position` and forward all surrounding
/// arguments (#3086). The fixed seven-argument `cast7` entrypoint used
/// previously ignored `position` and dropped extra arguments.
pub(crate) fn trace_callback_method_closure(id: i64) -> f64 {
    let func = cast1(diag_trace_callback);
    let c = js_closure_alloc(func, 1);
    js_closure_set_capture_ptr(c, 0, id);
    js_register_closure_synthetic_arguments(func, 0);
    boxed_ptr(c)
}

/// Build a NaN-boxed JS array value from a slice of callback arguments.
///
/// # Safety
/// Allocates on the current thread's arena; callers must ensure the runtime
/// is initialized (always true on the diagnostics_channel call path).
unsafe fn build_arg_array(values: &[f64]) -> f64 {
    let mut arr = js_array_alloc(values.len() as u32);
    for &v in values {
        arr = js_array_push_f64(arr, v);
    }
    boxed_ptr(arr)
}

/// Decode a NaN-boxed JS array value into a `Vec<f64>` of its elements.
/// Returns an empty vec for null/non-array inputs.
fn unbox_arg_array(arr_value: f64) -> Vec<f64> {
    let arr = js_nanbox_get_pointer(arr_value) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let len = js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(js_array_get_f64(arr, i));
    }
    out
}

pub(crate) extern "C" fn diag_channel_run_stores(
    closure: *const ClosureHeader,
    all_args: f64,
) -> f64 {
    let id = method_id(closure);
    // `all_args` is the synthetic-arguments rest array: [context, callback,
    // thisArg, ...args]. Split it back into the documented parameters and the
    // trailing callback argument list (#3082).
    let all = unbox_arg_array(all_args);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let data = all.first().copied().unwrap_or(undef);
    let fn_value = all.get(1).copied().unwrap_or(undef);
    let this_arg = all.get(2).copied().unwrap_or(undef);
    let cb_args: Vec<f64> = if all.len() > 3 {
        all[3..].to_vec()
    } else {
        Vec::new()
    };
    let stores = DIAG_CHANNELS.with(|m| {
        m.borrow()
            .get(&id)
            .map(|c| c.stores.clone())
            .unwrap_or_default()
    });
    if stores.is_empty() {
        return run_store_wrapped(id, data, fn_value, this_arg, &cb_args);
    }
    // Re-bundle the trailing callback arguments into a JS array so the chained
    // store-`run` thunk can forward the full list (closure captures are
    // fixed-width scalar slots, so the variadic tail rides in a single array
    // handle).
    let cb_args_arr = unsafe { build_arg_array(&cb_args) };
    let next = js_closure_alloc(cast0(store_next_thunk), 5);
    js_register_closure_arity(cast0(store_next_thunk), 0);
    js_closure_set_capture_ptr(next, 0, id);
    js_closure_set_capture_ptr(next, 1, data.to_bits() as i64);
    js_closure_set_capture_ptr(next, 2, fn_value.to_bits() as i64);
    js_closure_set_capture_ptr(next, 3, this_arg.to_bits() as i64);
    js_closure_set_capture_ptr(next, 4, cb_args_arr.to_bits() as i64);
    let mut next_value = boxed_ptr(next);
    for (store, transform) in stores.into_iter().rev() {
        let context = match transform {
            StoreTransform::Callable(t) => {
                match catch_js(|| js_closure_call1(closure_ptr(t), data)) {
                    Ok(context) => context,
                    Err(err) => {
                        schedule_uncaught(err);
                        continue;
                    }
                }
            }
            StoreTransform::NonCallable => {
                // Node calls the stored transform; a non-callable value
                // throws `TypeError: transform is not a function`, reported
                // uncaught, and the store is not entered (#3085).
                schedule_uncaught(make_transform_not_a_function_error());
                continue;
            }
            StoreTransform::None => data,
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
    if global_active_count(&key) > 0 {
        return bool_value(true);
    }
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
            result
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
            result
        } else {
            publish_channel(events[1], context);
            set_field_value(
                crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
                "result",
                result,
            );
            publish_channel(events[2], context);
            publish_channel(events[3], context);
            result
        }
    } else {
        // Node publishes only start/end for non-thenable returns, sets
        // context.result, returns the plain value, and emits a process warning.
        // The parity runner normalizes the warning away, so preserve the
        // observable channel ordering and return/context shape here.
        set_field_value(
            crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader,
            "result",
            result,
        );
        publish_channel(events[1], context);
        result
    }
}

/// The callback installed in place of the user's callback by
/// `traceCallback`. Registered as a synthetic-arguments closure so it forwards
/// every argument the traced function passes (`cb(null, "a", "b")`) to the
/// user callback, matching Node's `ReflectApply(callback, this, arguments)`
/// (#3086). `arguments[0]` is `err`, `arguments[1]` is `res`.
pub(crate) extern "C" fn diag_trace_wrapped_callback(
    closure: *const ClosureHeader,
    all_args: f64,
) -> f64 {
    let callback = js_closure_get_capture_f64(closure, 0);
    let context = js_closure_get_capture_f64(closure, 1);
    let async_start = js_closure_get_capture_ptr(closure, 2);
    let async_end = js_closure_get_capture_ptr(closure, 3);
    let error = js_closure_get_capture_ptr(closure, 4);
    let context_obj = crate::value::js_nanbox_get_pointer(context) as *mut ObjectHeader;

    let cb_args = unbox_arg_array(all_args);
    let err = cb_args.first().copied().unwrap_or_else(undefined);
    let res = cb_args.get(1).copied().unwrap_or_else(undefined);

    if crate::value::js_is_truthy(err) != 0 {
        set_field_value(context_obj, "error", err);
        publish_channel(error, context);
    } else {
        set_field_value(context_obj, "result", res);
    }

    publish_channel(async_start, context);
    let result = match catch_js(|| call_fn_value(callback, undefined(), &cb_args)) {
        Ok(result) => result,
        Err(callback_err) => {
            publish_channel(async_end, context);
            crate::exception::js_throw(callback_err)
        }
    };
    publish_channel(async_end, context);
    result
}

/// Throw the Node `ERR_INVALID_ARG_TYPE` for a non-function callback at the
/// requested `traceCallback` position (#3086).
fn throw_trace_callback_not_function() -> ! {
    let msg = b"The \"callback\" argument must be of type function.";
    let s = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(boxed_ptr(err))
}

pub(crate) extern "C" fn diag_trace_callback(closure: *const ClosureHeader, all_args: f64) -> f64 {
    // Synthetic-arguments rest array: [fn, position, context, thisArg, ...args]
    // (#3086). `position` defaults to -1 (the last trailing arg), the callback
    // lives at `args[position]`, and the wrapped callback is spliced in at that
    // position while all surrounding args are preserved.
    let all = unbox_arg_array(all_args);
    let undef = undefined();
    let fn_value = all.first().copied().unwrap_or(undef);
    let position_value = all.get(1).copied().unwrap_or(undef);
    let context = all.get(2).copied().unwrap_or(undef);
    let this_arg = all.get(3).copied().unwrap_or(undef);
    let mut args: Vec<f64> = if all.len() > 4 {
        all[4..].to_vec()
    } else {
        Vec::new()
    };

    // `position` defaults to -1. Resolve to a forward index using
    // Array.prototype.at semantics (negative counts from the end). `position`
    // arrives NaN-boxed (int32 or f64), so decode it as a JS number.
    let position = if position_value.to_bits() == TAG_UNDEFINED {
        -1i64
    } else {
        JSValue::from_bits(position_value.to_bits()).to_number() as i64
    };
    let idx = if position < 0 {
        args.len() as i64 + position
    } else {
        position
    };
    let cb_index = if idx >= 0 && (idx as usize) < args.len() {
        Some(idx as usize)
    } else {
        None
    };
    let callback = cb_index.map(|i| args[i]).unwrap_or(undef);

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
        // Inactive fast path: forward every argument verbatim, no callback
        // validation and no wrapping (matches Node's `ReflectApply(fn,
        // thisArg, args)`).
        return call_fn_value(fn_value, this_arg, &args);
    }

    // Active path validates the resolved callback (Node's
    // `validateFunction(callback, 'callback')`).
    if !valid_closure_value(callback) {
        throw_trace_callback_not_function();
    }

    let wrapped = js_closure_alloc(diag_trace_wrapped_callback as *const u8, 5);
    js_closure_set_capture_f64(wrapped, 0, callback);
    js_closure_set_capture_f64(wrapped, 1, context);
    js_closure_set_capture_ptr(wrapped, 2, events[2]);
    js_closure_set_capture_ptr(wrapped, 3, events[3]);
    js_closure_set_capture_ptr(wrapped, 4, events[4]);
    js_register_closure_synthetic_arguments(diag_trace_wrapped_callback as *const u8, 0);
    let wrapped_value = boxed_ptr(wrapped);

    // Splice the wrapped callback in at the resolved position, preserving all
    // surrounding arguments (#3086).
    if let Some(i) = cb_index {
        args[i] = wrapped_value;
    }

    publish_channel(events[0], context);
    match catch_js(|| call_fn_value(fn_value, this_arg, &args)) {
        Ok(ret) => {
            publish_channel(events[1], context);
            ret
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

pub(crate) extern "C" fn thunk_diag_tracing_channel(
    _closure: *const ClosureHeader,
    name_or_channels: f64,
) -> f64 {
    let id = next_diag_id();
    // `tracingChannel` accepts a string name or a channel-object map, but NOT a
    // symbol (unlike plain `channel(symbol)`). Reject symbols with Node's
    // ERR_INVALID_ARG_TYPE before the string-name branch (#3084).
    if is_symbol_value(name_or_channels) {
        throw_tracing_channel_symbol(name_or_channels);
    }
    let events = if decode_string_value(name_or_channels).is_some() {
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
    set_field_value(obj, "traceCallback", trace_callback_method_closure(id));
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

// #854: diagnostics_channel noop-closure helper retained for the subsystem
#[allow(dead_code)]
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
