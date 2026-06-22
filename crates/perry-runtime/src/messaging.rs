//! Minimal Web messaging globals used by Node-compatible `globalThis` and
//! `node:worker_threads` constructor identity.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{self, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::collections::{HashMap, VecDeque};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;

type MessageChannelFactory = extern "C" fn() -> f64;
type BroadcastChannelFactory = extern "C" fn(f64) -> f64;

static WORKER_THREADS_MESSAGE_CHANNEL_FACTORY: AtomicPtr<()> = AtomicPtr::new(null_mut());
static WORKER_THREADS_BROADCAST_CHANNEL_FACTORY: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_worker_threads_messaging_constructors(
    message_channel: MessageChannelFactory,
    broadcast_channel: BroadcastChannelFactory,
) {
    WORKER_THREADS_MESSAGE_CHANNEL_FACTORY.store(message_channel as *mut (), Ordering::Release);
    WORKER_THREADS_BROADCAST_CHANNEL_FACTORY.store(broadcast_channel as *mut (), Ordering::Release);
}

fn worker_threads_message_channel_factory() -> Option<MessageChannelFactory> {
    let ptr = WORKER_THREADS_MESSAGE_CHANNEL_FACTORY.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

fn worker_threads_broadcast_channel_factory() -> Option<BroadcastChannelFactory> {
    let ptr = WORKER_THREADS_BROADCAST_CHANNEL_FACTORY.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

fn js_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn js_null() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn boxed_object(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn key(name: &str) -> *mut StringHeader {
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    object::js_object_set_field_by_name(obj, key(name), value);
}

fn get_global_constructor(name: &str) -> f64 {
    let global = object::js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global) as *const ObjectHeader;
    if global_obj.is_null() {
        return js_undefined();
    }
    object::js_object_get_field_by_name_f64(global_obj, key(name))
}

fn constructor_prototype(name: &str) -> f64 {
    let ctor = get_global_constructor(name);
    let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
    if ctor_ptr == 0 {
        return js_undefined();
    }
    crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
}

fn set_object_prototype(obj: *mut ObjectHeader, prototype: f64) {
    if obj.is_null() {
        return;
    }
    if crate::value::js_nanbox_get_pointer(prototype) != 0 {
        object::prototype_chain::object_set_static_prototype(obj as usize, prototype.to_bits());
    }
}

fn closure_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    js_register_closure_arity(func_ptr, arity);
    let closure = js_closure_alloc(func_ptr, 0);
    object::set_bound_native_closure_name(closure, name);
    object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn noop0(_closure: *const ClosureHeader) -> f64 {
    js_undefined()
}

extern "C" fn noop1(_closure: *const ClosureHeader, _arg0: f64) -> f64 {
    js_undefined()
}

extern "C" fn noop2(_closure: *const ClosureHeader, _arg0: f64, _arg1: f64) -> f64 {
    js_undefined()
}

extern "C" fn has_ref(_closure: *const ClosureHeader) -> f64 {
    js_bool(false)
}

fn install_method(obj: *mut ObjectHeader, name: &str, func_ptr: *const u8, arity: u32) {
    set_field(obj, name, closure_value(func_ptr, name, arity));
}

// ============================================================================
// Same-thread MessageChannel / MessagePort delivery.
//
// Each `MessagePort` is a plain JS object. We keep its delivery state in a
// process-global side table keyed by the port object pointer: the entangled
// (paired) port, an incoming FIFO queue of cloned message values, a "started"
// flag, a closed flag, and the list of `addEventListener("message", fn)`
// listeners. Delivery follows HTML/Node single-thread semantics: each queued
// message is dispatched on its own macrotask, reusing the runtime's existing
// `setImmediate` callback-timer queue (so the event loop already keeps itself
// alive and pumps the delivery).
// ============================================================================

#[derive(Default)]
struct PortState {
    /// Heap address of the entangled (paired) port object. 0 until linked.
    /// Used only as a `PORT_STATES` key (never dereferenced), but it is a live
    /// reference to the partner port: the GC scanner visits it with
    /// `visit_usize_slot` so the partner survives event-loop turns and so the
    /// address is rewritten if the partner is evacuated.
    entangled: usize,
    /// FIFO of cloned message values waiting to be dispatched to THIS port.
    queue: VecDeque<f64>,
    /// A port is "started" once `onmessage` is assigned or `.start()` runs.
    started: bool,
    /// `.close()` stops further delivery.
    closed: bool,
    /// The current `onmessage` handler value (callable or null/undefined).
    onmessage: f64,
    /// `addEventListener("message", fn)` listeners (closure values).
    listeners: Vec<f64>,
}

static PORT_STATES: Mutex<Option<HashMap<usize, PortState>>> = Mutex::new(None);
static GC_SCANNER_REGISTERED: AtomicBool = AtomicBool::new(false);

fn ensure_gc_scanner_registered() {
    if !GC_SCANNER_REGISTERED.swap(true, Ordering::AcqRel) {
        crate::gc::gc_register_mutable_root_scanner_named(
            "messaging_ports",
            port_states_root_scanner_mut,
        );
    }
}

/// Keep queued message values, `onmessage` handlers, listener closures, and the
/// entangled partner port alive across event-loop turns — they are NaN-boxed JS
/// values / heap pointers held only by this side table while a macrotask is
/// pending. The table is keyed by each port's heap address, so the keys are
/// rewritten too: if the GC evacuates a port, a stale key would make later
/// `this`-pointer lookups miss and silently drop messages.
pub fn port_states_root_scanner_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut guard = crate::gc::lock_gc_root_registry(&PORT_STATES);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for state in map.values_mut() {
        for v in state.queue.iter_mut() {
            visitor.visit_nanbox_f64_slot(v);
        }
        visitor.visit_nanbox_f64_slot(&mut state.onmessage);
        // Root + rewrite the entangled-partner address (raw heap pointer).
        visitor.visit_usize_slot(&mut state.entangled);
        for l in state.listeners.iter_mut() {
            visitor.visit_nanbox_f64_slot(l);
        }
    }

    // Rewrite any keys whose port object was evacuated so the entry stays
    // reachable under its new address. Collect first to avoid mutating the map
    // while its key iterator is borrowed.
    let relocations: Vec<(usize, usize)> = map
        .keys()
        .copied()
        .filter_map(|old| {
            let mut new = old;
            visitor.visit_usize_slot(&mut new);
            (new != old).then_some((old, new))
        })
        .collect();
    for (old, new) in relocations {
        if let Some(state) = map.remove(&old) {
            map.insert(new, state);
        }
    }
}

fn with_port_states<R>(f: impl FnOnce(&mut HashMap<usize, PortState>) -> R) -> R {
    let mut guard = PORT_STATES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Resolve the current method receiver (`this`) to a port object pointer.
fn this_port_ptr() -> usize {
    let this = object::js_implicit_this_get();
    crate::value::js_nanbox_get_pointer(this) as usize
}

/// Register two freshly-created ports as an entangled pair.
fn entangle_ports(port1: *mut ObjectHeader, port2: *mut ObjectHeader) {
    ensure_gc_scanner_registered();
    let p1 = port1 as usize;
    let p2 = port2 as usize;
    with_port_states(|map| {
        map.entry(p1).or_default().entangled = p2;
        map.entry(p2).or_default().entangled = p1;
    });
}

/// Best-effort structured clone. Full structured-clone / transfer-list support
/// is out of scope; passing the value through preserves identity-free data and
/// never crashes on a transfer array argument.
fn structured_clone(value: f64) -> f64 {
    value
}

fn value_is_callable(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    ptr != 0 && crate::closure::is_closure_ptr(ptr)
}

/// Build a `MessageEvent`-shaped object: `{ data, type: "message" }`.
fn make_message_event(data: f64) -> f64 {
    let event = object::js_object_alloc(0, 0);
    set_field(event, "data", data);
    let type_ptr = js_string_from_bytes(b"message".as_ptr(), 7);
    set_field(
        event,
        "type",
        f64::from_bits(JSValue::string_ptr(type_ptr).bits()),
    );
    boxed_object(event)
}

fn invoke_message_handler(handler: f64, event: f64, port_box: f64) {
    if !value_is_callable(handler) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handler_h = scope.root_nanbox_f64(handler);
    let event_h = scope.root_nanbox_f64(event);
    let port_h = scope.root_nanbox_f64(port_box);
    let prev_this = object::js_implicit_this_set(port_h.get_nanbox_f64());
    let args = [event_h.get_nanbox_f64()];
    unsafe {
        let _ = crate::closure::js_native_call_value(
            handler_h.get_nanbox_f64(),
            args.as_ptr(),
            args.len(),
        );
    }
    object::js_implicit_this_set(prev_this);
}

/// Macrotask body: deliver exactly one queued message to `port_box`'s port.
/// Scheduled via `setImmediate`, so the event loop pumps it; chains naturally
/// because a handler that posts again schedules a fresh macrotask.
extern "C" fn deliver_one_message(_closure: *const ClosureHeader, port_box: f64) -> f64 {
    let port_ptr = crate::value::js_nanbox_get_pointer(port_box) as usize;
    if port_ptr == 0 {
        return js_undefined();
    }

    let (data, handler, listeners, deliverable) = with_port_states(|map| {
        let Some(state) = map.get_mut(&port_ptr) else {
            return (js_undefined(), js_null(), Vec::new(), false);
        };
        if state.closed || !state.started {
            return (js_undefined(), js_null(), Vec::new(), false);
        }
        match state.queue.pop_front() {
            Some(data) => (data, state.onmessage, state.listeners.clone(), true),
            None => (js_undefined(), js_null(), Vec::new(), false),
        }
    });

    if !deliverable {
        return js_undefined();
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let data_h = scope.root_nanbox_f64(data);
    let event = make_message_event(data_h.get_nanbox_f64());
    let event_h = scope.root_nanbox_f64(event);

    invoke_message_handler(handler, event_h.get_nanbox_f64(), port_box);
    for listener in listeners {
        invoke_message_handler(listener, event_h.get_nanbox_f64(), port_box);
    }
    js_undefined()
}

/// Schedule one delivery macrotask for `port_ptr` via the existing
/// `setImmediate` callback-timer queue, passing the port object pointer as a
/// trailing argument so the native thunk knows which port to flush.
fn schedule_delivery(port_ptr: usize) {
    let func = deliver_one_message as *const u8;
    js_register_closure_arity(func, 1);
    let closure = js_closure_alloc(func, 0);
    let closure_ptr = closure as i64;
    let port_box = crate::value::js_nanbox_pointer(port_ptr as i64);
    let args = [port_box];
    unsafe {
        crate::timer::js_set_immediate_callback_args(closure_ptr, args.as_ptr(), args.len() as i32);
    }
}

/// `port.postMessage(data[, transferList])`.
extern "C" fn port_post_message(_closure: *const ClosureHeader, data: f64, _transfer: f64) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr == 0 {
        return js_undefined();
    }
    let cloned = structured_clone(data);
    let target = with_port_states(|map| map.get(&self_ptr).map(|s| s.entangled).unwrap_or(0));
    if target == 0 {
        return js_undefined();
    }
    let should_schedule = with_port_states(|map| {
        let state = map.entry(target).or_default();
        if state.closed {
            return false;
        }
        state.queue.push_back(cloned);
        // Only schedule a delivery task once the target has been started.
        // Otherwise the message stays queued and is flushed when the port
        // starts (via `onmessage` assignment or `.start()`).
        state.started
    });
    if should_schedule {
        schedule_delivery(target);
    }
    js_undefined()
}

/// Mark a port started and schedule one delivery task per queued message.
fn start_port(self_ptr: usize) {
    if self_ptr == 0 {
        return;
    }
    let pending = with_port_states(|map| {
        let state = map.entry(self_ptr).or_default();
        if state.closed {
            return 0;
        }
        state.started = true;
        state.queue.len()
    });
    for _ in 0..pending {
        schedule_delivery(self_ptr);
    }
}

extern "C" fn port_start(_closure: *const ClosureHeader) -> f64 {
    start_port(this_port_ptr());
    js_undefined()
}

extern "C" fn port_close(_closure: *const ClosureHeader) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr != 0 {
        with_port_states(|map| {
            let Some(state) = map.get_mut(&self_ptr) else {
                return;
            };
            state.closed = true;
            state.queue.clear();
            let entangled = state.entangled;
            // Reclaim side-table entries once the channel is dead: a port with
            // no partner, or whose partner is already closed (or gone). This
            // keeps long-running apps that churn through short-lived
            // MessageChannels (e.g. the React scheduler pattern) from
            // accumulating stale port state without bound.
            let partner_done =
                entangled == 0 || map.get(&entangled).map(|s| s.closed).unwrap_or(true);
            if partner_done {
                map.remove(&self_ptr);
                if entangled != 0 {
                    map.remove(&entangled);
                }
            }
        });
    }
    js_undefined()
}

/// `port.onmessage` getter — return the stored handler (null if unset).
extern "C" fn port_onmessage_get(_closure: *const ClosureHeader) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr == 0 {
        return js_null();
    }
    with_port_states(|map| map.get(&self_ptr).map(|s| s.onmessage).unwrap_or(js_null()))
}

/// `port.onmessage` setter — store the handler and (per HTML) implicitly
/// start the port, flushing any queued messages.
extern "C" fn port_onmessage_set(_closure: *const ClosureHeader, value: f64) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr == 0 {
        return js_undefined();
    }
    with_port_states(|map| {
        map.entry(self_ptr).or_default().onmessage = value;
    });
    start_port(self_ptr);
    js_undefined()
}

extern "C" fn port_add_event_listener(
    _closure: *const ClosureHeader,
    type_value: f64,
    listener: f64,
) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr == 0 || !is_message_type(type_value) || !value_is_callable(listener) {
        return js_undefined();
    }
    let mut newly_started = false;
    with_port_states(|map| {
        let state = map.entry(self_ptr).or_default();
        if !state
            .listeners
            .iter()
            .any(|l| l.to_bits() == listener.to_bits())
        {
            state.listeners.push(listener);
        }
        if !state.started && !state.closed {
            newly_started = true;
        }
    });
    // Adding a "message" listener also starts the port (Node/HTML: a port is
    // implicitly started once it has a message consumer).
    if newly_started {
        start_port(self_ptr);
    }
    js_undefined()
}

extern "C" fn port_remove_event_listener(
    _closure: *const ClosureHeader,
    type_value: f64,
    listener: f64,
) -> f64 {
    let self_ptr = this_port_ptr();
    if self_ptr == 0 || !is_message_type(type_value) {
        return js_undefined();
    }
    with_port_states(|map| {
        if let Some(state) = map.get_mut(&self_ptr) {
            state
                .listeners
                .retain(|l| l.to_bits() != listener.to_bits());
        }
    });
    js_undefined()
}

fn is_message_type(type_value: f64) -> bool {
    crate::node_submodules::diagnostics::decode_string_value(type_value)
        .map(|s| s == "message")
        .unwrap_or(false)
}

/// Install the delivery-capable method set + onmessage accessor on a port.
fn install_port_methods(obj: *mut ObjectHeader) {
    install_method(obj, "postMessage", port_post_message as *const u8, 2);
    install_method(obj, "start", port_start as *const u8, 0);
    install_method(obj, "close", port_close as *const u8, 0);
    install_method(obj, "ref", noop0 as *const u8, 0);
    install_method(obj, "unref", noop0 as *const u8, 0);
    install_method(obj, "hasRef", has_ref as *const u8, 0);
    install_method(
        obj,
        "addEventListener",
        port_add_event_listener as *const u8,
        2,
    );
    install_method(
        obj,
        "removeEventListener",
        port_remove_event_listener as *const u8,
        2,
    );
    set_field(obj, "onmessageerror", js_null());

    // `onmessage` as a getter/setter so assignment starts the port + flushes.
    let getter = closure_value(port_onmessage_get as *const u8, "get onmessage", 0);
    let setter = closure_value(port_onmessage_set as *const u8, "set onmessage", 1);
    let obj_box = boxed_object(obj);
    let key_box = f64::from_bits(JSValue::string_ptr(key("onmessage")).bits());
    object::js_object_define_accessor(obj_box, key_box, getter, setter);
}

/// Install the Node-shaped prototype members for the three messaging
/// constructors. The method bodies are intentionally small no-ops here; this
/// slice is about constructor identity and shape, while full message delivery
/// remains worker_threads parity follow-up work.
pub fn populate_messaging_prototype(builtin_name: &str, proto: *mut ObjectHeader, ctor: f64) {
    if proto.is_null() {
        return;
    }
    set_field(proto, "constructor", ctor);
    object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        object::PropertyAttrs::new(true, false, true),
    );

    match builtin_name {
        "MessagePort" => {
            install_method(proto, "postMessage", noop2 as *const u8, 2);
            install_method(proto, "start", noop0 as *const u8, 0);
            install_method(proto, "ref", noop0 as *const u8, 0);
            install_method(proto, "unref", noop0 as *const u8, 0);
            install_method(proto, "hasRef", has_ref as *const u8, 0);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
            install_method(proto, "close", noop0 as *const u8, 0);
        }
        "MessageChannel" => {}
        "BroadcastChannel" => {
            set_field(proto, "name", js_undefined());
            install_method(proto, "close", noop0 as *const u8, 0);
            install_method(proto, "postMessage", noop1 as *const u8, 1);
            install_method(proto, "ref", noop0 as *const u8, 0);
            install_method(proto, "unref", noop0 as *const u8, 0);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
        }
        _ => {}
    }
}

fn message_port_object() -> *mut ObjectHeader {
    let obj = object::js_object_alloc(0, 0);
    let proto = constructor_prototype("MessagePort");
    set_object_prototype(obj, proto);
    set_field(obj, "constructor", get_global_constructor("MessagePort"));
    install_port_methods(obj);
    obj
}

#[no_mangle]
pub extern "C" fn js_message_channel_new() -> f64 {
    if let Some(factory) = worker_threads_message_channel_factory() {
        return factory();
    }
    let obj = object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("MessageChannel"));
    set_field(obj, "constructor", get_global_constructor("MessageChannel"));
    let port1 = message_port_object();
    let port2 = message_port_object();
    entangle_ports(port1, port2);
    set_field(obj, "port1", boxed_object(port1));
    set_field(obj, "port2", boxed_object(port2));
    boxed_object(obj)
}

pub(crate) extern "C" fn js_message_channel_constructor_call_error(
    _closure: *const ClosureHeader,
) -> f64 {
    throw_constructor_call_error()
}

#[no_mangle]
pub extern "C" fn js_broadcast_channel_new(name: f64) -> f64 {
    if let Some(factory) = worker_threads_broadcast_channel_factory() {
        return factory(name);
    }
    let obj = object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("BroadcastChannel"));
    set_field(
        obj,
        "constructor",
        get_global_constructor("BroadcastChannel"),
    );
    let name_ptr = crate::builtins::js_string_coerce(name);
    let name_value = f64::from_bits(JSValue::string_ptr(name_ptr).bits());
    set_field(obj, "name", name_value);
    install_method(obj, "close", noop0 as *const u8, 0);
    install_method(obj, "postMessage", noop1 as *const u8, 1);
    install_method(obj, "ref", noop0 as *const u8, 0);
    install_method(obj, "unref", noop0 as *const u8, 0);
    set_field(obj, "onmessage", js_null());
    set_field(obj, "onmessageerror", js_null());
    boxed_object(obj)
}

pub(crate) extern "C" fn js_broadcast_channel_constructor_call_error(
    _closure: *const ClosureHeader,
    _arg: f64,
) -> f64 {
    throw_constructor_call_error()
}

pub(crate) extern "C" fn js_message_port_constructor_call_error(
    _closure: *const ClosureHeader,
) -> f64 {
    throw_constructor_call_error()
}

#[no_mangle]
pub extern "C" fn js_message_port_constructor_error() -> f64 {
    throw_constructor_call_error()
}

fn throw_constructor_call_error() -> f64 {
    let msg = js_string_from_bytes(b"Constructor cannot be called".as_ptr(), 28);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
