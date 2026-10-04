use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::value::JSValue;

pub(super) extern "C" fn ns_set_max_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    set_stream_max_listeners(super::this_value(closure, this), value)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_set_max_listeners(stream_handle: i64, value: f64) -> f64 {
    set_stream_max_listeners(super::stream_value_from_handle(stream_handle), value)
}

fn set_stream_max_listeners(stream: f64, value: f64) -> f64 {
    let value = validate_max_listeners(value);
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    set_named(stream.get_nanbox_f64(), MAX_LISTENERS_KEY, value);
    stream.get_nanbox_f64()
}

fn format_max_listeners_received(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn throw_max_listeners_invalid_type(value: f64) -> ! {
    let message = format!(
        "The \"setMaxListeners\" argument must be of type number. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_max_listeners_out_of_range(n: f64) -> ! {
    let message = format!(
        "The value of \"setMaxListeners\" is out of range. It must be >= 0. Received {}",
        format_max_listeners_received(n)
    );
    crate::fs::validate::throw_range_error_with_code(&message)
}

pub(crate) fn validate_max_listeners(value: f64) -> f64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if !crate::fs::validate::is_numeric(js_value) {
        throw_max_listeners_invalid_type(value);
    }
    let n = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else {
        js_value.as_number()
    };
    if n.is_nan() || n < 0.0 {
        throw_max_listeners_out_of_range(n);
    }
    n
}

pub(super) extern "C" fn ns_get_max_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    stream_max_listeners(super::this_value(closure, this))
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_get_max_listeners(stream_handle: i64) -> f64 {
    stream_max_listeners(super::stream_value_from_handle(stream_handle))
}

fn stream_max_listeners(stream: f64) -> f64 {
    // node: `_maxListeners === undefined ? defaultMaxListeners : _maxListeners`.
    let max = get_named(stream, MAX_LISTENERS_KEY);
    if is_undefined(max) {
        DEFAULT_MAX_LISTENERS
    } else {
        max
    }
}

pub(super) extern "C" fn ns_on2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_once2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

pub(super) extern "C" fn ns_prepend_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

pub(super) extern "C" fn ns_prepend_once_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

pub(super) extern "C" fn ns_remove_listener2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

pub(super) extern "C" fn ns_off2(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
    cb: f64,
) -> f64 {
    ns_remove_listener2(closure, this, event, cb)
}

pub(super) extern "C" fn ns_remove_all_listeners1(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_on(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_on_value(stream, event, cb, false) {
        return result;
    }
    add_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_once(stream_handle: i64, event: f64, cb: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_on_value(stream, event, cb, true) {
        return result;
    }
    add_stream_listener_for_event_with_options(stream, event, cb, true, false);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, false, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_prepend_once_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    add_stream_listener_for_event_with_options(stream, event, cb, true, true);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_listener(
    stream_handle: i64,
    event: f64,
    cb: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_off_value(stream, event, cb) {
        return result;
    }
    remove_stream_listener_for_event(stream, event, cb);
    stream
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_off(stream_handle: i64, event: f64, cb: f64) -> f64 {
    js_node_stream_method_remove_listener(stream_handle, event, cb)
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_remove_all_listeners(
    stream_handle: i64,
    event: f64,
) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_remove_all_value(stream, event) {
        return result;
    }
    remove_all_stream_listeners_for_event(stream, event);
    stream
}

pub(super) extern "C" fn ns_listener_count(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    stream_listener_count_for_event(super::this_value(closure, this), event) as f64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listener_count(stream_handle: i64, event: f64) -> f64 {
    let stream = super::stream_value_from_handle(stream_handle);
    if let Some(result) = crate::fs::utf8_stream_listener_count_value(stream, event) {
        return result;
    }
    stream_listener_count_for_event(stream, event) as f64
}

pub(super) extern "C" fn ns_event_names(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(JSValue::pointer(stream_event_names_array(stream) as *const u8).bits())
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_event_names(stream_handle: i64) -> i64 {
    stream_event_names_array(super::stream_value_from_handle(stream_handle)) as i64
}

pub(super) extern "C" fn ns_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, false) as *const u8)
            .bits(),
    )
}

pub(super) extern "C" fn ns_raw_listeners(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    let stream = super::this_value(closure, this);
    f64::from_bits(
        JSValue::pointer(stream_listeners_array_for_event(stream, event, true) as *const u8).bits(),
    )
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, false)
        as i64
}

#[no_mangle]
pub extern "C" fn js_node_stream_method_raw_listeners(stream_handle: i64, event: f64) -> i64 {
    stream_listeners_array_for_event(super::stream_value_from_handle(stream_handle), event, true)
        as i64
}

pub(super) fn is_callable_value(value: f64) -> bool {
    let raw = super::raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

pub(super) fn add_stream_listener_for_event(stream: f64, event: f64, cb: f64) {
    add_stream_listener_for_event_with_options(stream, event, cb, false, false);
}

// ─────────────────────────────────────────────────────────────────
// Listener state is node's own (`lib/events.js`): `this._events` is a
// null-prototype object mapping each event key to ONE listener function or an
// array of them, `this._eventsCount` counts its keys, and a `once` listener is
// stored as a wrapper function whose `.listener` is the original. Every read
// and write goes through ordinary property access on the receiver, so code
// that inspects or edits `_events` directly (readable-stream's
// `prependListener`, ee-first, user code) sees and changes the same state the
// methods use, and the methods themselves live on one shared prototype
// rather than on every instance.
// ─────────────────────────────────────────────────────────────────

const EVENTS_KEY: &[u8] = b"_events";
const EVENTS_COUNT_KEY: &[u8] = b"_eventsCount";
const MAX_LISTENERS_KEY: &[u8] = b"_maxListeners";
const DEFAULT_MAX_LISTENERS: f64 = 10.0;

fn undefined_value() -> f64 {
    f64::from_bits(super::TAG_UNDEFINED)
}

fn is_undefined(value: f64) -> bool {
    value.to_bits() == super::TAG_UNDEFINED
}

/// `target[key]`: the full [[Get]] (prototype chain, accessors, proxies).
fn get_key(target: f64, key: f64) -> f64 {
    unsafe { crate::object::js_object_get_property_key(target, key) }
}

/// `target[key] = value`: the full [[Set]].
fn set_key(target: f64, key: f64, value: f64) {
    unsafe {
        crate::object::js_object_set_property_key(target, key, value);
    }
}

/// A runtime-owned property name, interned: allocates only on the first use
/// per thread, then a hash probe returning the canonical (rooted) string.
fn name_key(name: &[u8]) -> f64 {
    f64::from_bits(JSValue::string_ptr(crate::string::intern_ascii_literal(name) as *mut _).bits())
}

fn get_named(target: f64, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let key = name_key(name);
    get_key(target.get_nanbox_f64(), key)
}

fn set_named(target: f64, name: &[u8], value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let value = scope.root_nanbox_f64(value);
    let key = name_key(name);
    set_key(target.get_nanbox_f64(), key, value.get_nanbox_f64());
}

fn is_object_value(value: f64) -> bool {
    super::object_ptr_from_value(value).is_some()
}

fn is_array_value(value: f64) -> bool {
    crate::array::js_array_is_array(value).to_bits() == super::TAG_TRUE
}

fn number_of(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        js.as_int32() as f64
    } else if js.is_number() {
        value
    } else {
        f64::NAN
    }
}

/// `{ __proto__: null }`, node's empty `_events`.
fn new_events_object() -> f64 {
    crate::object::js_object_create(f64::from_bits(crate::value::TAG_NULL))
}

fn reset_events(target: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let events = new_events_object();
    set_named(target.get_nanbox_f64(), EVENTS_KEY, events);
    set_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY, 0.0);
}

fn adjust_events_count(target: f64, delta: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let count = number_of(get_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY)) + delta;
    set_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY, count);
    count
}

/// The receiver's `_events` when it is an object.
fn events_of(target: f64) -> Option<f64> {
    let events = get_named(target, EVENTS_KEY);
    is_object_value(events).then_some(events)
}

/// node's `EventEmitter.init`: give the receiver its own `_events` (unless it
/// already has one that is not merely inherited), `_eventsCount` and
/// `_maxListeners`. Run by `super()` of a class extending EventEmitter and by
/// `EventEmitter.call(this)`.
pub(crate) fn init_event_emitter_state(target: f64) {
    if !is_object_value(target) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    // node: reset when `this._events === undefined || this._events ===
    // ObjectGetPrototypeOf(this)._events`. Without an own `_events` the read
    // IS the prototype's, so both arms hold and the (slow, inherited) reads
    // are skipped; only an own `_events` needs comparing.
    let has_own = crate::object::js_object_has_own(target.get_nanbox_f64(), name_key(EVENTS_KEY))
        .to_bits()
        == super::TAG_TRUE;
    let reset = !has_own || {
        let events = scope.root_nanbox_f64(get_named(target.get_nanbox_f64(), EVENTS_KEY));
        is_undefined(events.get_nanbox_f64()) || {
            let proto = crate::object::js_object_get_prototype_of(target.get_nanbox_f64());
            is_object_value(proto)
                && get_named(proto, EVENTS_KEY).to_bits() == events.get_nanbox_f64().to_bits()
        }
    };
    if reset {
        reset_events(target.get_nanbox_f64());
    }
    let max = get_named(target.get_nanbox_f64(), MAX_LISTENERS_KEY);
    let max = if crate::value::js_is_truthy(max) != 0 {
        max
    } else {
        undefined_value()
    };
    set_named(target.get_nanbox_f64(), MAX_LISTENERS_KEY, max);
}

/// The data properties node's `EventEmitter.prototype` carries ahead of its
/// methods: `_events: undefined`, `_eventsCount: 0`, `_maxListeners: undefined`.
pub(crate) fn install_event_emitter_prototype_state(proto: f64) {
    set_named(proto, EVENTS_KEY, undefined_value());
    set_named(proto, EVENTS_COUNT_KEY, 0.0);
    set_named(proto, MAX_LISTENERS_KEY, undefined_value());
}

/// Call `target[name](...args)` with `this = target`, as node's emitter does
/// for `this.emit('newListener', …)`, `this.removeListener(…)` and
/// `this.removeAllListeners(…)`, so a subclass override sees those calls.
/// `None` when the receiver has no callable `name`.
fn call_method(target: f64, name: &[u8], args: &[f64]) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let method = get_named(target.get_nanbox_f64(), name);
    if !is_callable_value(method) {
        return None;
    }
    let live_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    Some(unsafe {
        crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(target.get_nanbox_f64()),
            live_args.as_ptr(),
            live_args.len(),
        )
    })
}

fn emit_via_method(target: f64, args: &[f64]) {
    if call_method(target, b"emit", args).is_none() {
        if let Some((event, rest)) = args.split_first() {
            let _ = emit_stream_event(target, *event, rest);
        }
    }
}

/// `listener.listener ?? listener` — a once wrapper's original.
fn unwrap_listener(listener: f64) -> f64 {
    if !is_callable_value(listener) {
        return listener;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let listener = scope.root_nanbox_f64(listener);
    let inner = get_named(listener.get_nanbox_f64(), b"listener");
    if is_undefined(inner) || inner.to_bits() == crate::value::TAG_NULL {
        listener.get_nanbox_f64()
    } else {
        inner
    }
}

/// The body of node's `onceWrapper`: captures `[target, type, listener,
/// fired]`; the first call removes the wrapper and forwards to the listener.
extern "C" fn ns_once_wrapper(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    rest: f64,
) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    if crate::value::js_is_truthy(js_closure_get_capture_f64(closure, 3)) != 0 {
        return undefined_value();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let wrapper = scope.root_nanbox_f64(super::box_pointer(closure as *const u8));
    let target = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let event = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let listener = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let args = {
        let arr = super::raw_ptr_from_value(rest) as *const crate::array::ArrayHeader;
        let len = if arr.is_null() || !is_array_value(rest) {
            0
        } else {
            crate::array::js_array_length(arr)
        };
        (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect::<Vec<f64>>()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let removal = [event.get_nanbox_f64(), wrapper.get_nanbox_f64()];
    if call_method(target.get_nanbox_f64(), b"removeListener", &removal).is_none() {
        remove_stream_listener_for_event(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            wrapper.get_nanbox_f64(),
        );
    }
    let wrapper_ptr = super::raw_ptr_from_value(wrapper.get_nanbox_f64()) as *mut ClosureHeader;
    js_closure_set_capture_f64(wrapper_ptr, 3, f64::from_bits(super::TAG_TRUE));
    let live_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    if !is_callable_value(listener.get_nanbox_f64()) {
        return undefined_value();
    }
    unsafe {
        crate::closure::native_call_value_this(
            listener.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(target.get_nanbox_f64()),
            live_args.as_ptr(),
            live_args.len(),
        )
    }
}

/// node's `_onceWrap(target, type, listener)`.
fn once_wrap(target: f64, event: f64, listener: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let event = scope.root_nanbox_f64(event);
    let listener = scope.root_nanbox_f64(listener);
    let wrapper = js_closure_alloc(crate::fn_info!(ns_once_wrapper, 1; with_rest(0)), 4);
    js_closure_set_capture_f64(wrapper, 0, target.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 1, event.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 2, listener.get_nanbox_f64());
    js_closure_set_capture_f64(wrapper, 3, f64::from_bits(super::TAG_FALSE));
    let wrapper = scope.root_nanbox_f64(super::box_pointer(wrapper as *const u8));
    set_named(
        wrapper.get_nanbox_f64(),
        b"listener",
        listener.get_nanbox_f64(),
    );
    wrapper.get_nanbox_f64()
}

fn add_stream_listener_for_event_with_options(
    stream: f64,
    event: f64,
    cb: f64,
    once: bool,
    prepend: bool,
) {
    if !is_callable_value(cb) {
        throw_invalid_listener_type();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let cb = scope.root_nanbox_f64(cb);
    let stored = if once {
        once_wrap(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            cb.get_nanbox_f64(),
        )
    } else {
        cb.get_nanbox_f64()
    };
    let stored = scope.root_nanbox_f64(stored);

    let events = match events_of(target.get_nanbox_f64()) {
        None => {
            reset_events(target.get_nanbox_f64());
            events_of(target.get_nanbox_f64())
        }
        Some(events) => {
            if !is_undefined(get_named(events, b"newListener")) {
                let announced = if once {
                    cb.get_nanbox_f64()
                } else {
                    unwrap_listener(cb.get_nanbox_f64())
                };
                let meta = name_key(b"newListener");
                emit_via_method(
                    target.get_nanbox_f64(),
                    &[meta, event.get_nanbox_f64(), announced],
                );
                // A `newListener` listener may have replaced `_events`.
                events_of(target.get_nanbox_f64())
            } else {
                Some(events)
            }
        }
    };
    let Some(events) = events else {
        return;
    };
    let events = scope.root_nanbox_f64(events);
    let existing = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_undefined(existing.get_nanbox_f64()) {
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            stored.get_nanbox_f64(),
        );
        adjust_events_count(target.get_nanbox_f64(), 1.0);
    } else if is_array_value(existing.get_nanbox_f64()) {
        let arr =
            super::raw_ptr_from_value(existing.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
        let grown = if prepend {
            crate::array::js_array_unshift_f64(arr, stored.get_nanbox_f64())
        } else {
            crate::array::js_array_push_f64(arr, stored.get_nanbox_f64())
        };
        if grown as usize != arr as usize {
            set_key(
                events.get_nanbox_f64(),
                event.get_nanbox_f64(),
                super::box_pointer(grown as *const u8),
            );
        }
    } else {
        let mut pair = crate::array::js_array_alloc(2);
        let (first, second) = if prepend {
            (stored.get_nanbox_f64(), existing.get_nanbox_f64())
        } else {
            (existing.get_nanbox_f64(), stored.get_nanbox_f64())
        };
        pair = crate::array::js_array_push_f64(pair, first);
        pair = crate::array::js_array_push_f64(pair, second);
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            super::box_pointer(pair as *const u8),
        );
    }

    if super::string_value_eq(event.get_nanbox_f64(), b"data") {
        super::readable_data_listener_added(target.get_nanbox_f64());
    } else if super::string_value_eq(event.get_nanbox_f64(), b"readable") {
        super::readable_listener_added(target.get_nanbox_f64());
    }
}

#[cold]
fn throw_invalid_listener_type() -> ! {
    let msg = b"The \"listener\" argument must be of type function";
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    crate::node_submodules::register_error_code(s, "ERR_INVALID_ARG_TYPE");
    let err = crate::error::js_typeerror_new(s);
    let bits = JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

fn emit_remove_listener_if_watched(target: f64, events: f64, event: f64, listener: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let event = scope.root_nanbox_f64(event);
    let listener = scope.root_nanbox_f64(listener);
    if is_undefined(get_named(events, b"removeListener")) {
        return;
    }
    let meta = name_key(b"removeListener");
    emit_via_method(
        target.get_nanbox_f64(),
        &[meta, event.get_nanbox_f64(), listener.get_nanbox_f64()],
    );
}

/// node's `removeListener(type, listener)`. True when a listener was removed.
pub(super) fn remove_stream_listener_for_event(stream: f64, event: f64, cb: f64) -> bool {
    if !is_callable_value(cb) {
        throw_invalid_listener_type();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let cb = scope.root_nanbox_f64(cb);
    let Some(events) = events_of(target.get_nanbox_f64()) else {
        return false;
    };
    let events = scope.root_nanbox_f64(events);
    let list = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_undefined(list.get_nanbox_f64()) {
        return false;
    }
    let cb_bits = cb.get_nanbox_f64().to_bits();
    if is_callable_value(list.get_nanbox_f64()) {
        let inner = unwrap_listener(list.get_nanbox_f64());
        if list.get_nanbox_f64().to_bits() != cb_bits && inner.to_bits() != cb_bits {
            return false;
        }
        let announced = scope.root_nanbox_f64(inner);
        if adjust_events_count(target.get_nanbox_f64(), -1.0) == 0.0 {
            reset_events(target.get_nanbox_f64());
        } else {
            let _ = crate::object::js_object_delete_dynamic_value(
                events.get_nanbox_f64(),
                event.get_nanbox_f64(),
            );
            emit_remove_listener_if_watched(
                target.get_nanbox_f64(),
                events.get_nanbox_f64(),
                event.get_nanbox_f64(),
                announced.get_nanbox_f64(),
            );
        }
        return true;
    }
    if !is_array_value(list.get_nanbox_f64()) {
        return false;
    }
    let arr = super::raw_ptr_from_value(list.get_nanbox_f64()) as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut position = None;
    for i in (0..len).rev() {
        let arr =
            super::raw_ptr_from_value(list.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let item = crate::array::js_array_get_f64(arr, i);
        if item.to_bits() == cb_bits || unwrap_listener(item).to_bits() == cb_bits {
            position = Some(i);
            break;
        }
    }
    let Some(position) = position else {
        return false;
    };
    let arr = super::raw_ptr_from_value(list.get_nanbox_f64()) as *mut crate::array::ArrayHeader;
    // `js_array_splice` returns the DELETED elements and reports the edited
    // (possibly reallocated) receiver through its out-parameter.
    let mut kept: *mut crate::array::ArrayHeader = std::ptr::null_mut();
    let _deleted =
        crate::array::js_array_splice(arr, position as i32, 1, std::ptr::null(), 0, &mut kept);
    let kept_value = scope.root_nanbox_f64(super::box_pointer(kept as *const u8));
    let kept =
        super::raw_ptr_from_value(kept_value.get_nanbox_f64()) as *const crate::array::ArrayHeader;
    if crate::array::js_array_length(kept) == 1 {
        let only = crate::array::js_array_get_f64(kept, 0);
        set_key(events.get_nanbox_f64(), event.get_nanbox_f64(), only);
    } else if kept as usize != arr as usize {
        set_key(
            events.get_nanbox_f64(),
            event.get_nanbox_f64(),
            kept_value.get_nanbox_f64(),
        );
    }
    emit_remove_listener_if_watched(
        target.get_nanbox_f64(),
        events.get_nanbox_f64(),
        event.get_nanbox_f64(),
        cb.get_nanbox_f64(),
    );
    true
}

/// node's `removeAllListeners([type])`; `undefined` stands for "no argument".
fn remove_all_stream_listeners_for_event(stream: f64, event: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(target.get_nanbox_f64()) else {
        return;
    };
    let events = scope.root_nanbox_f64(events);
    let all = is_undefined(event.get_nanbox_f64());
    if is_undefined(get_named(events.get_nanbox_f64(), b"removeListener")) {
        if all {
            reset_events(target.get_nanbox_f64());
        } else if !is_undefined(get_key(events.get_nanbox_f64(), event.get_nanbox_f64())) {
            if adjust_events_count(target.get_nanbox_f64(), -1.0) == 0.0 {
                reset_events(target.get_nanbox_f64());
            } else {
                let _ = crate::object::js_object_delete_dynamic_value(
                    events.get_nanbox_f64(),
                    event.get_nanbox_f64(),
                );
            }
        }
        return;
    }
    if all {
        let keys = crate::proxy::js_reflect_own_keys(events.get_nanbox_f64());
        let keys = scope.root_nanbox_f64(keys);
        let keys_arr =
            super::raw_ptr_from_value(keys.get_nanbox_f64()) as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(keys_arr);
        for i in 0..len {
            let keys_arr = super::raw_ptr_from_value(keys.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            let key = crate::array::js_array_get_f64(keys_arr, i);
            if super::string_value_eq(key, b"removeListener") {
                continue;
            }
            remove_all_via_method(target.get_nanbox_f64(), key);
        }
        let meta = name_key(b"removeListener");
        remove_all_via_method(target.get_nanbox_f64(), meta);
        reset_events(target.get_nanbox_f64());
        return;
    }
    let listeners = scope.root_nanbox_f64(get_key(events.get_nanbox_f64(), event.get_nanbox_f64()));
    if is_callable_value(listeners.get_nanbox_f64()) {
        remove_via_method(
            target.get_nanbox_f64(),
            event.get_nanbox_f64(),
            listeners.get_nanbox_f64(),
        );
    } else if is_array_value(listeners.get_nanbox_f64()) {
        let arr = super::raw_ptr_from_value(listeners.get_nanbox_f64())
            as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        for i in (0..len).rev() {
            let arr = super::raw_ptr_from_value(listeners.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            if i >= crate::array::js_array_length(arr) {
                continue;
            }
            let item = crate::array::js_array_get_f64(arr, i);
            remove_via_method(target.get_nanbox_f64(), event.get_nanbox_f64(), item);
        }
    }
}

fn remove_via_method(target: f64, event: f64, listener: f64) {
    if call_method(target, b"removeListener", &[event, listener]).is_none() {
        remove_stream_listener_for_event(target, event, listener);
    }
}

fn remove_all_via_method(target: f64, event: f64) {
    if call_method(target, b"removeAllListeners", &[event]).is_none() {
        remove_all_stream_listeners_for_event(target, event);
    }
}

/// The listener (functions or once wrappers) stored for `event`, in order.
fn stored_listeners(target: f64, event: f64) -> Vec<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(target) else {
        return Vec::new();
    };
    let list = get_key(events, event.get_nanbox_f64());
    if is_undefined(list) {
        return Vec::new();
    }
    if is_array_value(list) {
        let arr = super::raw_ptr_from_value(list) as *const crate::array::ArrayHeader;
        let len = crate::array::js_array_length(arr);
        return (0..len)
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect();
    }
    if is_callable_value(list) {
        return vec![list];
    }
    Vec::new()
}

pub(super) fn stream_listener_count_for_event(stream: f64, event: f64) -> usize {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_nanbox_f64(event);
    let Some(events) = events_of(stream) else {
        return 0;
    };
    let list = get_key(events, event.get_nanbox_f64());
    if is_callable_value(list) {
        1
    } else if is_array_value(list) {
        crate::array::js_array_length(super::raw_ptr_from_value(list) as *const _) as usize
    } else {
        0
    }
}

/// node's `eventNames()`: `Reflect.ownKeys(this._events)` while any remain.
fn stream_event_names_array(stream: f64) -> *mut crate::array::ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(stream);
    let count = number_of(get_named(target.get_nanbox_f64(), EVENTS_COUNT_KEY));
    match events_of(target.get_nanbox_f64()) {
        Some(events) if count > 0.0 => {
            let keys = crate::proxy::js_reflect_own_keys(events);
            super::raw_ptr_from_value(keys) as *mut crate::array::ArrayHeader
        }
        _ => crate::array::js_array_alloc(0),
    }
}

/// `listeners(type)` (unwrapped) or `rawListeners(type)` (once wrappers kept).
fn stream_listeners_array_for_event(
    stream: f64,
    event: f64,
    raw: bool,
) -> *mut crate::array::ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stored = stored_listeners(stream, event);
    let stored = scope.root_nanbox_f64_slice(&stored);
    let mut values = Vec::with_capacity(stored.len());
    for handle in &stored {
        let listener = handle.get_nanbox_f64();
        values.push(if raw {
            listener
        } else {
            unwrap_listener(listener)
        });
    }
    let values = scope.root_nanbox_f64_slice(&values);
    // `js_array_alloc` reserves the capacity, so no push below allocates and
    // the rooted values stay current across the loop.
    let mut out = crate::array::js_array_alloc(values.len() as u32);
    for handle in &values {
        out = crate::array::js_array_push_f64(out, handle.get_nanbox_f64());
    }
    out
}

pub(super) fn call_listener_args(stream: f64, listener: f64, args: &[f64]) -> f64 {
    if !is_callable_value(listener) {
        return f64::from_bits(super::TAG_UNDEFINED);
    }
    unsafe {
        crate::closure::native_call_value_this(
            listener,
            crate::closure::JsThis::from_f64(stream),
            args.as_ptr(),
            args.len(),
        )
    }
}

/// node's `emitUnhandledRejectionOrErr`: a captured listener rejection goes to
/// the emitter's `[Symbol.for('nodejs.rejection')](err, type, ...args)` when
/// it has one, else to `emit('error', err)` with capture switched off for that
/// emit. Captures `[emitter, type, argsArray]`.
pub(super) extern "C" fn ns_capture_rejection(
    closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    reason: f64,
) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 0));
    let event = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 1));
    let args = scope.root_nanbox_f64(js_closure_get_capture_f64(closure, 2));
    let reason = scope.root_nanbox_f64(reason);
    let hook_key = unsafe { crate::symbol::js_symbol_for(name_key(b"nodejs.rejection")) };
    let hook = scope.root_nanbox_f64(get_key(stream.get_nanbox_f64(), hook_key));
    if is_callable_value(hook.get_nanbox_f64()) {
        let mut call_args = vec![reason.get_nanbox_f64(), event.get_nanbox_f64()];
        if is_array_value(args.get_nanbox_f64()) {
            let arr = super::raw_ptr_from_value(args.get_nanbox_f64())
                as *const crate::array::ArrayHeader;
            for i in 0..crate::array::js_array_length(arr) {
                call_args.push(crate::array::js_array_get_f64(arr, i));
            }
        }
        unsafe {
            crate::closure::native_call_value_this(
                hook.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(stream.get_nanbox_f64()),
                call_args.as_ptr(),
                call_args.len(),
            );
        }
        return undefined_value();
    }
    set_capture_rejections(stream.get_nanbox_f64(), false);
    let error = name_key(b"error");
    emit_via_method(stream.get_nanbox_f64(), &[error, reason.get_nanbox_f64()]);
    set_capture_rejections(stream.get_nanbox_f64(), true);
    undefined_value()
}

fn set_capture_rejections(stream: f64, enabled: bool) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let key = super::hidden_capture_rejections_key();
    super::set_hidden_value(stream.get_nanbox_f64(), key, bool_value(enabled));
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        super::TAG_TRUE
    } else {
        super::TAG_FALSE
    })
}

/// node's `EventEmitter.init(opts)` capture step: a truthy
/// `opts.captureRejections` must be a boolean, and turns capture on.
pub(crate) fn init_event_emitter_capture(target: f64, options: f64) {
    if !is_object_value(target) || !is_object_value(options) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let value = get_named(options, b"captureRejections");
    if crate::value::js_is_truthy(value) == 0 {
        return;
    }
    if value.to_bits() != super::TAG_TRUE {
        let message = format!(
            "The \"options.captureRejections\" property must be of type boolean. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    set_capture_rejections(target.get_nanbox_f64(), true);
}

fn capture_rejections_enabled(stream: f64) -> bool {
    super::has_truthy_hidden(stream, super::hidden_capture_rejections_key())
}

/// Mark an async listener's returned promise handled when its rejection is to
/// be swallowed (Node's Readable `data` path), so it is not reported as an
/// unhandled rejection at program end (#1545).
fn swallow_listener_rejection(result: f64) {
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    let promise = crate::value::js_nanbox_get_pointer(result) as *mut crate::promise::Promise;
    if !promise.is_null() {
        crate::promise::mark_rejection_handled(promise);
    }
}

/// node's `addCatch`: route a listener's rejected promise to
/// [`ns_capture_rejection`] with the emit's type and arguments.
fn capture_listener_rejection(stream: f64, event: f64, args: &[f64], result: f64) {
    if crate::promise::js_value_is_promise(result) == 0 {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream = scope.root_nanbox_f64(stream);
    let event = scope.root_nanbox_f64(event);
    let result = scope.root_nanbox_f64(result);
    let arg_handles = scope.root_nanbox_f64_slice(args);
    let mut arr = crate::array::js_array_alloc(arg_handles.len() as u32);
    for handle in &arg_handles {
        arr = crate::array::js_array_push_f64(arr, handle.get_nanbox_f64());
    }
    let arr = scope.root_nanbox_f64(super::box_pointer(arr as *const u8));
    let on_rejected = js_closure_alloc(
        crate::fn_info!(ns_capture_rejection, 1; with_declared(1)),
        3,
    );
    js_closure_set_capture_f64(on_rejected, 0, stream.get_nanbox_f64());
    js_closure_set_capture_f64(on_rejected, 1, event.get_nanbox_f64());
    js_closure_set_capture_f64(on_rejected, 2, arr.get_nanbox_f64());
    let promise = crate::value::js_nanbox_get_pointer(result.get_nanbox_f64())
        as *mut crate::promise::Promise;
    if promise.is_null() {
        return;
    }
    crate::promise::js_promise_then(promise, std::ptr::null(), on_rejected);
}

pub(super) fn emit_stream_event_from_array(
    stream: f64,
    event: f64,
    args_arr: *const crate::array::ArrayHeader,
) -> f64 {
    let len = if args_arr.is_null() {
        0
    } else {
        crate::array::js_array_length(args_arr)
    };
    let mut args = Vec::with_capacity(len as usize);
    for i in 0..len {
        args.push(crate::array::js_array_get_f64(args_arr, i));
    }
    emit_stream_event(stream, event, &args)
}

/// #9493: whether `stream` has a listener for `event` in THIS registry. The
/// fs-stream bridge asks before forwarding an `'error'` its own registry has
/// already delivered, so the unhandled-error throw below fires only when
/// neither registry had a listener.
pub(super) fn has_stream_listeners(stream: f64, event: f64) -> bool {
    stream_listener_count_for_event(stream, event) > 0
}

/// node's `emit(type, ...args)`.
pub(super) fn emit_stream_event(stream: f64, event: f64, args: &[f64]) -> f64 {
    // #10600: a listener can allocate enough to trigger a moving collection,
    // so the receiver, the event, the arguments and the listener snapshot are
    // all rooted for the whole dispatch window and re-read before each call.
    let scope = crate::gc::RuntimeHandleScope::new();
    let stream_h = scope.root_nanbox_f64(stream);
    let event_h = scope.root_nanbox_f64(event);
    let arg_handles = scope.root_nanbox_f64_slice(args);

    let is_error = super::string_value_eq(event, b"error");
    if is_error {
        if let Some(first) = args.first() {
            let first = scope.root_nanbox_f64(*first);
            let key = super::hidden_error_key();
            super::set_hidden_value(stream_h.get_nanbox_f64(), key, first.get_nanbox_f64());
            super::refresh_readable_aborted_flag(stream_h.get_nanbox_f64());
        }
    }
    let events = events_of(stream_h.get_nanbox_f64());
    let mut unhandled_error = is_error;
    match events {
        Some(events) => {
            if is_error {
                let events = scope.root_nanbox_f64(events);
                let monitor = error_monitor_event();
                if !is_undefined(get_key(events.get_nanbox_f64(), monitor)) {
                    let live =
                        crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
                    let mut monitor_args = Vec::with_capacity(live.len() + 1);
                    monitor_args.push(monitor);
                    monitor_args.extend_from_slice(&live);
                    emit_via_method(stream_h.get_nanbox_f64(), &monitor_args);
                }
                unhandled_error = is_undefined(get_named(events.get_nanbox_f64(), b"error"));
            }
        }
        None if !is_error => return f64::from_bits(super::TAG_FALSE),
        None => {}
    }
    if unhandled_error {
        let err = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles)
            .first()
            .copied()
            .unwrap_or_else(undefined_value);
        crate::exception::js_throw(err);
    }

    let Some(events) = events_of(stream_h.get_nanbox_f64()) else {
        return f64::from_bits(super::TAG_FALSE);
    };
    let handler = get_key(events, event_h.get_nanbox_f64());
    if is_undefined(handler) {
        return f64::from_bits(super::TAG_FALSE);
    }
    // node clones the array before dispatch, so listeners added or removed
    // by a listener take effect from the next emit.
    let listener_values: Vec<f64> = if is_callable_value(handler) {
        vec![handler]
    } else if is_array_value(handler) {
        let arr = super::raw_ptr_from_value(handler) as *const crate::array::ArrayHeader;
        (0..crate::array::js_array_length(arr))
            .map(|i| crate::array::js_array_get_f64(arr, i))
            .collect()
    } else {
        Vec::new()
    };
    let listener_handles = scope.root_nanbox_f64_slice(&listener_values);
    // Node's Readable data delivery path does not route async `data` listener
    // rejections through captureRejections; custom EventEmitter-style events do.
    // As in node's `addCatch`, the capture flag is consulted only for a
    // listener that returned a promise.
    let is_data = super::string_value_eq(event_h.get_nanbox_f64(), b"data");
    for handle in &listener_handles {
        let live_args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
        let result = call_listener_args(
            stream_h.get_nanbox_f64(),
            handle.get_nanbox_f64(),
            &live_args,
        );
        if crate::promise::js_value_is_promise(result) == 0 {
            continue;
        }
        if !is_error && !is_data && capture_rejections_enabled(stream_h.get_nanbox_f64()) {
            capture_listener_rejection(
                stream_h.get_nanbox_f64(),
                event_h.get_nanbox_f64(),
                &crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles),
                result,
            );
        } else if is_data {
            // Node's Readable swallows a rejection returned by an async `data`
            // listener — it is neither captured to `error` nor surfaced as an
            // unhandled rejection. Mark it handled so it stays silent (#1545).
            swallow_listener_rejection(result);
        }
    }
    f64::from_bits(super::TAG_TRUE)
}

fn error_monitor_event() -> f64 {
    unsafe { crate::symbol::js_symbol_for(super::literal_string_value(b"events.errorMonitor")) }
}
