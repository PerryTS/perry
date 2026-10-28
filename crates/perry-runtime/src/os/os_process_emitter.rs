//! `process` EventEmitter surface (listeners, emit, nextTick, uncaught-error
//! semantics) extracted from `os.rs` to keep it under the 2000-line cap.
//! `use super::*` preserves parent visibility; os.rs re-exports via glob.
use super::*;

#[derive(Clone, Copy)]
struct ProcessListener {
    callback: *const crate::closure::ClosureHeader,
    raw_wrapper: *const crate::closure::ClosureHeader,
    once: bool,
}

struct ProcessEmitter {
    events: HashMap<String, Vec<ProcessListener>>,
    event_order: Vec<String>,
    max_listeners: f64,
}

impl ProcessEmitter {
    fn new() -> Self {
        Self {
            events: HashMap::new(),
            event_order: Vec::new(),
            max_listeners: 10.0,
        }
    }

    fn ensure_event_order(&mut self, event: &str) {
        if !self.event_order.iter().any(|name| name == event) {
            self.event_order.push(event.to_string());
        }
    }

    fn prune_event_if_empty(&mut self, event: &str) {
        if self
            .events
            .get(event)
            .map(|listeners| listeners.is_empty())
            .unwrap_or(false)
        {
            self.events.remove(event);
            self.event_order.retain(|name| name != event);
        }
    }
}

thread_local! {
    static PROCESS_EMITTER: RefCell<ProcessEmitter> = RefCell::new(ProcessEmitter::new());
}

/// #8220: GC root scanner for `PROCESS_EMITTER`. Each `ProcessListener` holds
/// raw `*const ClosureHeader` pointers (`callback`, `raw_wrapper`) that the
/// precise root map cannot see — they live in a TLS `HashMap`, not in a
/// codegen root slot or a registered global. Without this scanner, a copying
/// minor that evacuates a listener closure leaves the raw pointer pointing at
/// retired from-space, and the next `emit` dereferences stale memory.
pub(crate) fn process_emitter_root_scanner(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        for listeners in emitter.events.values_mut() {
            for listener in listeners.iter_mut() {
                visitor.visit_raw_const_ptr_slot(&mut listener.callback);
                visitor.visit_raw_const_ptr_slot(&mut listener.raw_wrapper);
            }
        }
    });
}

fn process_namespace_value() -> f64 {
    crate::object::js_create_native_module_namespace(b"process".as_ptr(), "process".len())
}

/// Coerce a raw NaN-boxed JS value into Node's EventEmitter event-name key
/// (#3047). The `process` global is an EventEmitter, so non-symbol event
/// names follow `ToString` semantics: `123` → `"123"`, `null` → `"null"`,
/// `undefined` → `"undefined"`, `{}` → `"[object Object]"`. Strings pass
/// through unchanged.
///
/// Returns `None` only when the value carries no string representation we
/// can read back (it should not happen for the supported primitive/object
/// inputs). Symbol event names are not yet keyed by identity — they coerce
/// to their `String(sym)` form here, which is sufficient for the
/// string-keyed emitter but does not preserve per-symbol identity.
fn coerce_event_name(event_bits: i64) -> Option<String> {
    let value = f64::from_bits(event_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    // Fast path: already a heap/SSO string — read it directly so we never
    // round-trip a perfectly good string through ToString.
    if jv.is_string() || jv.is_short_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
        return read_event_name(ptr);
    }
    let coerced = crate::value::js_jsvalue_to_string(value);
    read_event_name(coerced)
}

/// Validate an EventEmitter listener argument supplied as raw NaN-box bits
/// and return its closure pointer (#3047). Non-callable values throw Node's
/// `TypeError [ERR_INVALID_ARG_TYPE]` with the shared `"listener"` message
/// via `js_validate_event_listener`, matching `EventEmitter.prototype`.
fn validate_listener(listener_bits: i64) -> *const crate::closure::ClosureHeader {
    let name = "listener";
    let ptr = unsafe {
        crate::fs::validate::js_validate_event_listener(
            listener_bits,
            name.as_ptr(),
            name.len() as u32,
        )
    };
    ptr as *const crate::closure::ClosureHeader
}

/// Extract a closure pointer from raw NaN-box bits *without* throwing, for
/// the lookup-style methods (`removeListener`/`off`/`listenerCount`) where
/// Node simply finds no match for a non-callable rather than raising. A
/// non-closure value yields a null pointer that matches no stored listener.
fn listener_lookup_ptr(listener_bits: i64) -> *const crate::closure::ClosureHeader {
    let value = f64::from_bits(listener_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>() as usize;
        if crate::closure::is_closure_ptr(ptr) {
            return ptr as *const crate::closure::ClosureHeader;
        }
    }
    std::ptr::null()
}

extern "C" fn process_once_raw_wrapper(
    closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    rest_args: f64,
) -> f64 {
    if closure.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let target = crate::closure::js_closure_get_capture_f64(closure, 0);
    let target_jv = crate::value::JSValue::from_bits(target.to_bits());
    if !target_jv.is_pointer() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let target_ptr = target_jv.as_pointer::<crate::closure::ClosureHeader>();
    if target_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let rest_jv = crate::value::JSValue::from_bits(rest_args.to_bits());
    if !rest_jv.is_pointer() {
        return unsafe {
            crate::closure::js_closure_call_array(
                target_ptr as i64,
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            )
        };
    }
    let arr = rest_jv.as_pointer::<ArrayHeader>();
    if arr.is_null() {
        return unsafe {
            crate::closure::js_closure_call_array(
                target_ptr as i64,
                crate::closure::plain_call_receiver(),
                std::ptr::null(),
                0,
            )
        };
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut args = Vec::with_capacity(len);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(arr, i as u32));
    }
    unsafe {
        crate::closure::js_closure_call_array(
            target_ptr as i64,
            crate::closure::plain_call_receiver(),
            args.as_ptr(),
            args.len() as i64,
        )
    }
}

fn create_process_once_raw_wrapper(
    callback: *const crate::closure::ClosureHeader,
) -> *const crate::closure::ClosureHeader {
    let wrapper = crate::closure::js_closure_alloc(
        crate::fn_info!(process_once_raw_wrapper, 1; with_rest(0)),
        1,
    );
    let callback_value =
        f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits());
    crate::closure::js_closure_set_capture_f64(wrapper, 0, callback_value);
    crate::closure::closure_set_dynamic_prop(wrapper as usize, "listener", callback_value);
    let name = "bound onceWrapper";
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_value = f64::from_bits(crate::value::JSValue::string_ptr(name_ptr).bits());
    crate::closure::closure_define_dynamic_prop(wrapper as usize, "name", name_value);
    wrapper
}

fn process_listener_matches(
    listener: &ProcessListener,
    handler: *const crate::closure::ClosureHeader,
) -> bool {
    !handler.is_null() && (listener.callback == handler || listener.raw_wrapper == handler)
}

fn process_listener_count(event: &str) -> usize {
    PROCESS_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get(event)
            .map(|listeners| listeners.len())
            .unwrap_or(0)
    })
}

static PROCESS_SIGNAL_SYNC: crate::feature_hooks::Hook<fn(&str)> =
    crate::feature_hooks::Hook::empty();

pub(crate) fn install_process_signal_sync() {
    PROCESS_SIGNAL_SYNC.set(sync_process_signal_listener_impl);
}

fn sync_process_signal_listener(event: &str) {
    if let Some(sync) = PROCESS_SIGNAL_SYNC.get() {
        sync(event);
    }
}

fn sync_process_signal_listener_impl(event: &str) {
    if super::signal::is_process_signal_name(event) {
        super::signal::set_process_signal_listener_count(event, process_listener_count(event));
    }
}

fn register_process_listener(
    event_bits: i64,
    listener_bits: i64,
    once: bool,
    prepend: bool,
) -> f64 {
    // Node validates the listener *before* coercing the event name, so an
    // invalid listener throws even for an odd event value.
    let callback = validate_listener(listener_bits);
    let Some(event) = coerce_event_name(event_bits) else {
        return process_namespace_value();
    };

    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        emitter.ensure_event_order(&event);
        let raw_wrapper = if once {
            create_process_once_raw_wrapper(callback)
        } else {
            std::ptr::null()
        };
        let listener = ProcessListener {
            callback,
            raw_wrapper,
            once,
        };
        let listeners = emitter.events.entry(event.clone()).or_default();
        if prepend {
            listeners.insert(0, listener);
        } else {
            listeners.push(listener);
        }
    });
    sync_process_signal_listener(&event);
    crate::process::process_ipc_note_listener(&event);
    process_namespace_value()
}

pub(crate) fn add_internal_process_listener(
    event: &str,
    callback: *const crate::closure::ClosureHeader,
) {
    if callback.is_null() {
        return;
    }
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        emitter.ensure_event_order(event);
        emitter
            .events
            .entry(event.to_string())
            .or_default()
            .push(ProcessListener {
                callback,
                raw_wrapper: std::ptr::null(),
                once: false,
            });
    });
    sync_process_signal_listener(event);
}

pub(crate) fn remove_internal_process_listener(
    event: &str,
    callback: *const crate::closure::ClosureHeader,
) {
    if callback.is_null() {
        return;
    }
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        if let Some(listeners) = emitter.events.get_mut(event) {
            listeners.retain(|listener| listener.callback != callback);
        }
        emitter.prune_event_if_empty(event);
    });
    sync_process_signal_listener(event);
}

fn boxed_bool(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn listener_array(event_bits: i64, raw: bool) -> *mut ArrayHeader {
    let Some(event) = coerce_event_name(event_bits) else {
        return crate::array::js_array_alloc(0);
    };
    let callbacks = PROCESS_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get(&event)
            .map(|listeners| {
                listeners
                    .iter()
                    .map(|listener| {
                        if raw && listener.once && !listener.raw_wrapper.is_null() {
                            listener.raw_wrapper
                        } else {
                            listener.callback
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let mut arr = crate::array::js_array_alloc(callbacks.len() as u32);
    for callback in callbacks {
        arr =
            crate::array::js_array_push(arr, crate::value::JSValue::pointer(callback as *const u8));
    }
    arr
}

pub(crate) fn emit_process_event(event: &str, args: &[f64]) -> bool {
    let listeners = PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        let Some(listeners) = emitter.events.get_mut(event) else {
            return Vec::new();
        };
        let snapshot = listeners.clone();
        if snapshot.iter().any(|listener| listener.once) {
            listeners.retain(|listener| !listener.once);
        }
        emitter.prune_event_if_empty(event);
        snapshot
    });

    if listeners.is_empty() {
        if event == "error" {
            throw_unhandled_error_event(args);
        }
        return false;
    }

    for listener in listeners {
        unsafe {
            crate::closure::js_closure_call_array(
                listener.callback as i64,
                crate::closure::plain_call_receiver(),
                args.as_ptr(),
                args.len() as i64,
            );
        }
    }
    true
}

/// process.on(event, listener) — register an event listener.
///
/// `event_bits`/`listener_bits` are the raw NaN-boxed JS values (codegen
/// routes both through `NA_JSV`) so the event name is coerced with
/// `ToString` and a non-callable listener throws `ERR_INVALID_ARG_TYPE`
/// (#3047), matching Node's EventEmitter argument handling.
#[no_mangle]
pub extern "C" fn js_process_on(event_bits: i64, listener_bits: i64) -> f64 {
    register_process_listener(event_bits, listener_bits, false, false)
}

/// process.addListener(event, listener) — alias for on().
#[no_mangle]
pub extern "C" fn js_process_add_listener(event_bits: i64, listener_bits: i64) -> f64 {
    register_process_listener(event_bits, listener_bits, false, false)
}

/// process.once(event, listener) — one-shot listener (Node parity).
#[no_mangle]
pub extern "C" fn js_process_once(event_bits: i64, listener_bits: i64) -> f64 {
    register_process_listener(event_bits, listener_bits, true, false)
}

#[no_mangle]
pub extern "C" fn js_process_prepend_listener(event_bits: i64, listener_bits: i64) -> f64 {
    register_process_listener(event_bits, listener_bits, false, true)
}

#[no_mangle]
pub extern "C" fn js_process_prepend_once_listener(event_bits: i64, listener_bits: i64) -> f64 {
    register_process_listener(event_bits, listener_bits, true, true)
}

/// Emit the synthetic `beforeExit` event with the would-be exit code as a
/// numeric argument. Called from the codegen-emitted event-loop epilogue
/// once the loop has drained all real work (#2135). Node's semantics fire
/// this hook *only* when the event loop is exiting on its own; an explicit
/// `process.exit()` skips it. Perry's `js_process_exit` calls `libc::_exit`
/// directly without going through this hook, so that contract is preserved.
#[no_mangle]
pub extern "C" fn js_process_emit_before_exit(code: f64) {
    let _ = emit_process_event("beforeExit", &[code]);
}

// Codegen-only symbol: nothing inside the runtime calls it, so anchor it
// against the auto-optimize whole-program dead-strip (#4876), like the other
// epilogue hooks.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PROCESS_EMIT_BEFORE_EXIT_PENDING: extern "C" fn() = js_process_emit_before_exit_pending;

mod process_exit_tls {
    type Value = std::cell::Cell<bool>;
    type Storage = crate::tls_hot::HotCell<Value, { std::mem::needs_drop::<Value>() as usize }>;
    crate::__perry_thread_local_storage!(Storage, const { Value::new(false) });
    pub(super) fn resolve(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
        STORAGE.try_with(|cell| {
            cell.arm_guard(idx);
            cell.value_addr()
        })
    }
    pub(super) static KEY: crate::tls_hot::HotKey<Value> = crate::tls_hot::HotKey::new(
        crate::tls_hot::SlotId::named(concat!(
            module_path!(),
            "::PROCESS_EXIT_EVENT_EMITTED@",
            line!(),
            ":",
            column!()
        )),
        crate::os_hook_slots::resolve_process_exit_guard,
    );
}
use process_exit_tls::KEY as PROCESS_EXIT_EVENT_EMITTED;
pub(crate) fn install_exit_tls_resolver() {
    crate::os_hook_slots::install_exit_resolver(process_exit_tls::resolve);
}

/// Emit the `exit` event with `code`, at most once per process.
///
/// Node runs `exit` listeners synchronously at the moment the process leaves:
/// after the loop drained (`beforeExit` first), from `process.exit()`, and
/// from the fatal uncaught-exception / unhandled-rejection paths. Only
/// SYNCHRONOUS work inside a listener is honoured, and that falls out of where
/// this is called rather than from any suppression machinery: every caller
/// terminates — or returns out of generated `main` — as soon as this returns,
/// so a `setTimeout`, `setImmediate` or `nextTick` queued by a listener is
/// never given a turn, while a `writeFileSync` in the same listener has
/// already landed.
///
/// Prefer `crate::process::run_process_exit_sequence`, which also publishes
/// the code as `process.exitCode` and reports the — possibly reassigned —
/// final status. This entry point exists for it and for tests.
#[no_mangle]
pub extern "C" fn js_process_emit_exit(code: f64) {
    if PROCESS_EXIT_EVENT_EMITTED.with(|emitted| emitted.replace(true)) {
        return;
    }
    let _ = emit_process_event("exit", &[code]);
}

/// Whether the one-shot `exit` emit has already happened on this thread.
pub(crate) fn process_exit_event_emitted() -> bool {
    PROCESS_EXIT_EVENT_EMITTED.with(|emitted| emitted.get())
}

#[cfg(test)]
pub(crate) fn reset_process_exit_event_emitted_for_test() {
    PROCESS_EXIT_EVENT_EMITTED.with(|emitted| emitted.set(false));
}

pub extern "C" fn js_process_signal_drain() -> i32 {
    let mut count = 0i32;
    for event in super::signal::take_pending_process_signals() {
        if event == "SIGWINCH" {
            // Node refreshes the TTY streams' size before user listeners run.
            crate::os::refresh_tty_stream_sizes();
        }
        if emit_process_event(event, &[]) {
            count += 1;
        }
        sync_process_signal_listener(event);
    }
    count
}

pub extern "C" fn js_process_signal_has_active() -> i32 {
    if super::signal::has_active_process_signal_listeners() {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn js_process_emit(event_bits: i64, args: *const ArrayHeader) -> f64 {
    let Some(event) = coerce_event_name(event_bits) else {
        return boxed_bool(false);
    };
    let values = collect_emit_args(args);
    boxed_bool(emit_process_event(&event, &values))
}

#[no_mangle]
pub extern "C" fn js_process_remove_listener(event_bits: i64, listener_bits: i64) -> f64 {
    let handler = listener_lookup_ptr(listener_bits);
    if let Some(event) = coerce_event_name(event_bits) {
        PROCESS_EMITTER.with(|emitter| {
            let mut emitter = emitter.borrow_mut();
            if let Some(listeners) = emitter.events.get_mut(&event) {
                if let Some(pos) = listeners
                    .iter()
                    .rposition(|listener| process_listener_matches(listener, handler))
                {
                    listeners.remove(pos);
                }
            }
            emitter.prune_event_if_empty(&event);
        });
        sync_process_signal_listener(&event);
    }
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_off(event_bits: i64, listener_bits: i64) -> f64 {
    js_process_remove_listener(event_bits, listener_bits)
}

#[no_mangle]
pub extern "C" fn js_process_remove_all_listeners(event_bits: i64) -> f64 {
    // `removeAllListeners()` / `removeAllListeners(undefined)` clears every
    // event. Node treats a missing or `undefined`/`null` argument as
    // "no specific event" rather than coercing it to the literal string
    // `"undefined"`/`"null"`, so guard those tags before coercing.
    let value = f64::from_bits(event_bits as u64);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let target = if jv.is_undefined() || jv.is_null() {
        None
    } else {
        coerce_event_name(event_bits)
    };
    let signal_events = PROCESS_EMITTER.with(|emitter| {
        let emitter = emitter.borrow();
        match target.as_ref() {
            Some(event) if super::signal::is_process_signal_name(event) => vec![event.clone()],
            Some(_) => Vec::new(),
            None => emitter
                .event_order
                .iter()
                .filter(|event| super::signal::is_process_signal_name(event))
                .cloned()
                .collect::<Vec<_>>(),
        }
    });
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        if let Some(event) = target.as_ref() {
            emitter.events.remove(event);
            emitter.event_order.retain(|name| name != event);
        } else {
            emitter.events.clear();
            emitter.event_order.clear();
        }
    });
    for event in signal_events {
        sync_process_signal_listener(&event);
    }
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_listener_count(event_bits: i64, listener_bits: i64) -> f64 {
    let handler = listener_lookup_ptr(listener_bits);
    let Some(event) = coerce_event_name(event_bits) else {
        return 0.0;
    };
    PROCESS_EMITTER.with(|emitter| {
        let emitter = emitter.borrow();
        let Some(listeners) = emitter.events.get(&event) else {
            return 0.0;
        };
        if handler.is_null() {
            listeners.len() as f64
        } else {
            listeners
                .iter()
                .filter(|listener| process_listener_matches(listener, handler))
                .count() as f64
        }
    })
}

#[no_mangle]
pub extern "C" fn js_process_listeners(event_bits: i64) -> *mut ArrayHeader {
    listener_array(event_bits, false)
}

#[no_mangle]
pub extern "C" fn js_process_raw_listeners(event_bits: i64) -> *mut ArrayHeader {
    listener_array(event_bits, true)
}

#[no_mangle]
pub extern "C" fn js_process_event_names() -> *mut ArrayHeader {
    let names = PROCESS_EMITTER.with(|emitter| emitter.borrow().event_order.clone());
    let mut arr = crate::array::js_array_alloc(names.len() as u32);
    for name in names {
        let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        arr = crate::array::js_array_push(arr, crate::value::JSValue::string_ptr(s));
    }
    arr
}

#[no_mangle]
pub extern "C" fn js_process_set_max_listeners(value: f64) -> f64 {
    // #3049 — share the EventEmitter setter validation: non-numbers throw
    // TypeError [ERR_INVALID_ARG_TYPE]; NaN/negative throw RangeError
    // [ERR_OUT_OF_RANGE]; finite non-negative (incl. fractional and
    // Infinity) are stored verbatim and read back exactly by
    // getMaxListeners(). Returns `process` so it chains like Node.
    let validated = crate::node_stream::validate_max_listeners(value);
    PROCESS_EMITTER.with(|emitter| {
        emitter.borrow_mut().max_listeners = validated;
    });
    process_namespace_value()
}

#[no_mangle]
pub extern "C" fn js_process_get_max_listeners() -> f64 {
    PROCESS_EMITTER.with(|emitter| emitter.borrow().max_listeners)
}

pub fn scan_process_event_listener_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        for listeners in emitter.events.values_mut() {
            for listener in listeners {
                visitor.visit_raw_const_ptr_slot(&mut listener.callback);
                if !listener.raw_wrapper.is_null() {
                    visitor.visit_raw_const_ptr_slot(&mut listener.raw_wrapper);
                }
            }
        }
    });
}

#[cfg(test)]
pub(crate) fn test_clear_process_event_listeners() {
    PROCESS_EMITTER.with(|emitter| {
        *emitter.borrow_mut() = ProcessEmitter::new();
    });
}

#[cfg(test)]
pub(crate) fn test_seed_process_event_listener_root(
    callback: *const crate::closure::ClosureHeader,
) {
    PROCESS_EMITTER.with(|emitter| {
        let mut emitter = emitter.borrow_mut();
        emitter.ensure_event_order("__test__");
        emitter.events.insert(
            "__test__".to_string(),
            vec![ProcessListener {
                callback,
                raw_wrapper: std::ptr::null(),
                once: false,
            }],
        );
    });
}

#[cfg(test)]
pub(crate) fn test_process_event_listener_root_snapshot() -> usize {
    PROCESS_EMITTER.with(|emitter| {
        emitter
            .borrow()
            .events
            .get("__test__")
            .and_then(|listeners| listeners.first())
            .map(|listener| listener.callback as usize)
            .unwrap_or(0)
    })
}

pub fn emit_process_uncaught_exception(error: f64) {
    if !emit_process_event("uncaughtException", &[error]) {
        crate::exception::exit_on_uncaught(error);
    }
}

use crate::process::tick_args::collect_emit_args;
pub use crate::process::tick_args::js_process_next_tick;

pub(crate) use crate::event_emitter_error::{read_event_name, throw_unhandled_error_event};

pub use crate::os_hook_slots::js_process_emit_before_exit_pending;
