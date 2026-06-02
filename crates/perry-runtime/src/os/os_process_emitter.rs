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

pub(crate) fn read_event_name(event_ptr: *const StringHeader) -> Option<String> {
    if event_ptr.is_null() {
        return None;
    }
    unsafe {
        let len = (*event_ptr).byte_len as usize;
        let data = (event_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).ok().map(|s| s.to_string())
    }
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
/// via `js_validate_event_listener`, matching `perry-stdlib::events`.
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
            crate::closure::js_closure_call_array(target_ptr as i64, std::ptr::null(), 0)
        };
    }
    let arr = rest_jv.as_pointer::<ArrayHeader>();
    if arr.is_null() {
        return unsafe {
            crate::closure::js_closure_call_array(target_ptr as i64, std::ptr::null(), 0)
        };
    }
    let len = crate::array::js_array_length(arr) as usize;
    let mut args = Vec::with_capacity(len);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(arr, i as u32));
    }
    unsafe {
        crate::closure::js_closure_call_array(target_ptr as i64, args.as_ptr(), args.len() as i64)
    }
}

fn create_process_once_raw_wrapper(
    callback: *const crate::closure::ClosureHeader,
) -> *const crate::closure::ClosureHeader {
    crate::closure::js_register_closure_rest(process_once_raw_wrapper as *const u8, 0);
    let wrapper = crate::closure::js_closure_alloc(process_once_raw_wrapper as *const u8, 1);
    let callback_value =
        f64::from_bits(crate::value::JSValue::pointer(callback as *const u8).bits());
    crate::closure::js_closure_set_capture_f64(wrapper, 0, callback_value);
    crate::closure::closure_set_dynamic_prop(wrapper as usize, "listener", callback_value);
    let name = "bound onceWrapper";
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let name_value = f64::from_bits(crate::value::JSValue::string_ptr(name_ptr).bits());
    crate::closure::closure_set_dynamic_prop(wrapper as usize, "name", name_value);
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

fn sync_process_signal_listener(event: &str) {
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
    process_namespace_value()
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

fn collect_emit_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args) as usize;
    let mut values = Vec::with_capacity(len);
    for i in 0..len {
        values.push(crate::array::js_array_get_f64(args, i as u32));
    }
    values
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
                args.as_ptr(),
                args.len() as i64,
            );
        }
    }
    true
}

/// Emit Node's special unhandled-`error`-event semantics (#3052).
///
/// `EventEmitter` (and therefore the `process` global) treats `emit("error", …)`
/// with no registered `error` listener specially: if the first argument is an
/// `Error` instance (or a subclass) it is re-thrown *as-is* — the same object,
/// preserving its `code`, prototype, and identity. Otherwise Node constructs a
/// fresh `Error` with `code: "ERR_UNHANDLED_ERROR"` and a message of
/// `Unhandled error. (<util.inspect(arg)>)`, where a missing argument inspects
/// to `undefined`. This replaces the previous behaviour of throwing the raw
/// first argument (so `emit("error", "boom")` now throws an `ERR_UNHANDLED_ERROR`
/// `Error` rather than the bare `"boom"` string).
fn throw_unhandled_error_event(args: &[f64]) -> ! {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let first = args.first().copied().unwrap_or(undefined);

    // `arg instanceof Error` → rethrow the original value untouched.
    let is_error =
        crate::value::js_is_truthy(crate::object::js_util_types_is_native_error(first)) != 0;
    if is_error {
        crate::exception::js_throw(first);
    }

    // Otherwise: `Unhandled error. (<inspected>)` with ERR_UNHANDLED_ERROR.
    let inspected = unsafe { read_inspected(crate::builtins::js_util_inspect(first, undefined)) };
    let message = format!("Unhandled error. ({inspected})");
    let msg_ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg_ptr, "ERR_UNHANDLED_ERROR");
    let err_ptr = crate::error::js_error_new_with_message(msg_ptr);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
}

/// Read the `util.inspect` result (a NaN-boxed string value) back into a Rust
/// `String` for embedding in the unhandled-error message.
unsafe fn read_inspected(value: f64) -> String {
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    read_event_name(ptr).unwrap_or_default()
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

pub extern "C" fn js_process_signal_drain() -> i32 {
    let mut count = 0i32;
    for event in super::signal::take_pending_process_signals() {
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
    emit_process_event("uncaughtException", &[error]);
}

/// process.nextTick(callback, ...args) — schedule callback as a tick,
/// forwarding trailing args (#3046). `callback_bits`/`args` are raw
/// NaN-boxed values. A non-callable callback throws Node's
/// `TypeError [ERR_INVALID_ARG_TYPE]` (`"callback"` message) synchronously.
/// Used by the method-value dispatch path (`const nt = process.nextTick`)
/// and the zero-arg `process.nextTick()` call form; the direct lowered form
/// with trailing args goes through codegen's `js_queue_next_tick_args`.
///
/// # Safety
/// `args` must be a valid NaN-boxed args array pointer, or null.
#[no_mangle]
pub unsafe extern "C" fn js_process_next_tick(callback_bits: i64, args: *const ArrayHeader) {
    // Mirror codegen's setTimeout/queueMicrotask validation: the timer
    // validator always reports the `"callback"` argument name, which is the
    // wording Node uses for `process.nextTick`.
    let callback =
        crate::timer::js_timer_validate_callback(f64::from_bits(callback_bits as u64), 3);
    let values = collect_emit_args(args);
    if values.is_empty() {
        crate::builtins::js_queue_next_tick(callback);
    } else {
        crate::builtins::js_queue_next_tick_args(callback, values.as_ptr(), values.len() as i32);
    }
}
