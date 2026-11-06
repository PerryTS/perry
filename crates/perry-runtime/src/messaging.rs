//! Minimal Web messaging globals used by Node-compatible `globalThis` and
//! `node:worker_threads` constructor identity.

use crate::closure::{js_closure_alloc, ClosureHeader};
use crate::object::{self, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::Cell;
use std::collections::HashMap;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Mutex;

mod broadcast_operations;
mod port_operations;
pub mod port_transfer;
#[cfg(test)]
mod port_transfer_tests;
pub mod port_transport;
mod worker_async_dispose;
mod worker_metadata;
pub use broadcast_operations::{
    broadcast_channel_id, broadcast_channel_object_with_id, register_broadcast_channel_dispatch,
    BroadcastChannelOperation,
};
pub use port_operations::install_core_methods as install_message_port_core_methods;
pub use port_operations::js_register_worker_threads_message_port_dispatch;
pub use port_operations::{
    message_port_id, message_port_object_with_id, post_message_arguments, MessagePortOperation,
};
pub(crate) use worker_async_dispose::install as install_worker_async_dispose;
pub(crate) use worker_metadata::install_worker_metadata_getters;
pub use worker_metadata::{
    register_worker_metadata_provider, worker_id, worker_object_with_id, WorkerMetadataField,
};

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

fn closure_value(info: *const crate::closure::JsFunctionInfo, name: &str, arity: u32) -> f64 {
    let closure = js_closure_alloc(info, 0);
    object::set_bound_native_closure_name(closure, name);
    object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn install_method(
    obj: *mut ObjectHeader,
    name: &str,
    info: *const crate::closure::JsFunctionInfo,
    arity: u32,
) {
    set_field(obj, name, closure_value(info, name, arity));
}

// Runtime-only wrappers and listeners belong to one agent. Endpoint ownership,
// queues and peer links live in port_transport and contain no heap references.

#[derive(Default)]
struct PortState {
    lease: port_transport::Lease,
    owner: crate::agent::AgentId,
    /// A port is "started" once `onmessage` is assigned or `.start()` runs.
    started: bool,
    refed: bool,
    /// The current `onmessage` handler value (callable or null/undefined).
    onmessage: f64,
    /// `addEventListener("message", fn)` listeners (closure values).
    listeners: Vec<f64>,
}

/// Consume the runtime transport's existing inbox without scheduling delivery.
pub fn take_runtime_port_message(receiver: f64) -> Option<f64> {
    if message_port_id(receiver) != Some(0) {
        return None;
    }
    let address = crate::value::js_nanbox_get_pointer(receiver) as usize;
    let lease = with_port_states(|states| states.get(&address).map(|state| state.lease))?;
    port_transport::receive(lease).map(|message| {
        f64::from_bits(unsafe { crate::thread::deserialize_nanbox_on_current_thread(&message) })
    })
}

static PORT_STATES: Mutex<Option<HashMap<usize, PortState>>> = Mutex::new(None);

mod root_guard_tls {
    use super::*;
    type Value = Cell<bool>;
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
            "::GC_SCANNER_REGISTERED@",
            line!(),
            ":",
            column!()
        )),
        crate::messaging_hook_slots::resolve_root_guard,
    );
}
use root_guard_tls::KEY as GC_SCANNER_REGISTERED;
pub(crate) fn install_root_tls_resolver() {
    crate::messaging_hook_slots::install_root_resolver(root_guard_tls::resolve);
}

fn ensure_gc_scanner_registered() {
    GC_SCANNER_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        crate::gc::gc_register_mutable_root_scanner_named(
            "messaging_ports",
            port_states_root_scanner_mut,
        );
        registered.set(true);
    });
}

/// Trace only this agent's wrappers and listener values, rewriting moving keys.
pub fn port_states_root_scanner_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut guard = crate::gc::lock_gc_root_registry(&PORT_STATES);
    let Some(map) = guard.as_mut() else {
        return;
    };

    let agent = crate::agent::current_agent();
    for state in map.values_mut().filter(|state| state.owner == agent) {
        visitor.visit_nanbox_f64_slot(&mut state.onmessage);
        for l in state.listeners.iter_mut() {
            visitor.visit_nanbox_f64_slot(l);
        }
    }

    // Rewrite any keys whose port object was evacuated so the entry stays
    // reachable under its new address. Collect first to avoid mutating the map
    // while its key iterator is borrowed.
    let relocations: Vec<(usize, usize)> = map
        .iter()
        .filter_map(|(&address, state)| (state.owner == agent).then_some(address))
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

/// Remove freed wrapper roots. Native ownership retirement is generation checked.
pub(crate) fn release_port_states_in_freed_ranges(freed: &crate::arena::thread_exit::FreedRanges) {
    let removed = {
        let mut guard = PORT_STATES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(map) = guard.as_mut() else {
            return;
        };
        let mut removed = Vec::new();
        map.retain(|&port, state| {
            let keep = !freed.contains(port)
                && !freed.holds_value(state.onmessage)
                && !state
                    .listeners
                    .iter()
                    .any(|&value| freed.holds_value(value));
            if !keep {
                removed.push(state.lease);
            }
            keep
        });
        removed
    };
    for lease in removed {
        port_transport::close(lease);
    }
}

/// Test probe (#11471): does `PORT_STATES` hold an entry for `port`?
#[doc(hidden)]
pub fn port_state_registered_for_test(port: usize) -> bool {
    PORT_STATES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .is_some_and(|map| map.contains_key(&port))
}

fn with_port_states<R>(f: impl FnOnce(&mut HashMap<usize, PortState>) -> R) -> R {
    // #11541: every entry is created through here (the root scanner and the
    // release below lock `PORT_STATES` directly), so the thread-exit hook is
    // registered before the first one instead of being named in
    // `arena::thread_exit`'s dispatcher, which linked it into every binary.
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        crate::arena::thread_exit::register_thread_exit_range_hook(
            release_port_states_in_freed_ranges,
        );
    });
    let mut guard = PORT_STATES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Resolve the current method receiver (`this`) to a port object pointer.
fn this_port_ptr(this: crate::closure::JsThis) -> usize {
    let this = this.as_f64();
    crate::value::js_nanbox_get_pointer(this) as usize
}

/// Register two freshly-created ports as an entangled pair.
fn entangle_ports(port1: *mut ObjectHeader, port2: *mut ObjectHeader) {
    ensure_gc_scanner_registered();
    register_runtime_pump();
    let (a, b) = port_transport::pair();
    with_port_states(|map| {
        for (address, lease) in [(port1 as usize, a), (port2 as usize, b)] {
            map.insert(
                address,
                PortState {
                    lease,
                    owner: crate::agent::current_agent(),
                    ..Default::default()
                },
            );
        }
    });
}

fn runtime_port_lease(value: f64) -> Option<port_transport::Lease> {
    let address = crate::value::js_nanbox_get_pointer(value) as usize;
    with_port_states(|map| {
        map.get(&address)
            .filter(|state| state.owner == crate::agent::current_agent())
            .map(|state| state.lease)
    })
}
fn adopt_runtime_port(lease: port_transport::Lease) -> f64 {
    ensure_gc_scanner_registered();
    register_runtime_pump();
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(message_port_object_with_id(0));
    install_port_methods(object.get_raw_mut_ptr());
    set_object_prototype(
        object.get_raw_mut_ptr(),
        constructor_prototype("MessagePort"),
    );
    with_port_states(|map| {
        map.insert(
            object.get_raw_mut_ptr::<ObjectHeader>() as usize,
            PortState {
                lease,
                owner: crate::agent::current_agent(),
                ..Default::default()
            },
        );
    });
    boxed_object(object.get_raw_mut_ptr())
}
fn register_runtime_pump() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| unsafe {
        extern "C" {
            fn js_register_agent_aux_pump(pump: extern "C" fn() -> i32);
            fn js_register_aux_has_active(active: extern "C" fn() -> i32);
        }
        js_register_agent_aux_pump(pump_runtime_ports);
        js_register_aux_has_active(runtime_ports_active);
    });
    crate::event_pump::claim_agent_loop_route();
}
extern "C" fn runtime_ports_active() -> i32 {
    with_port_states(|map| {
        i32::from(map.values().any(|state| {
            state.owner == crate::agent::current_agent()
                && state.started
                && port_transport::alive(state.lease)
                && !port_transport::closed(state.lease)
                && (state.refed || port_transport::pending(state.lease))
        }))
    })
}
extern "C" fn pump_runtime_ports() -> i32 {
    let addresses = with_port_states(|map| {
        map.iter()
            .filter_map(|(&address, state)| {
                (state.owner == crate::agent::current_agent()
                    && state.started
                    && port_transport::pending(state.lease))
                .then_some(address)
            })
            .collect::<Vec<_>>()
    });
    let count = addresses.len() as i32;
    for address in addresses {
        deliver_one_message(
            std::ptr::null(),
            crate::closure::JsThis::from_f64(js_undefined()),
            crate::value::js_nanbox_pointer(address as i64),
        );
    }
    count
}

fn value_is_callable(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let ptr = crate::value::js_nanbox_get_pointer(value) as usize;
    ptr != 0 && crate::closure::is_closure_ptr(ptr)
}

fn invoke_message_handler(handler: f64, event: f64, port_box: f64) {
    if !value_is_callable(handler) {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handler_h = scope.root_nanbox_f64(handler);
    let event_h = scope.root_nanbox_f64(event);
    let port_h = scope.root_nanbox_f64(port_box);
    crate::event_target_hook_slots::with_message_event_dispatch(
        event_h.get_nanbox_f64(),
        port_h.get_nanbox_f64(),
        || unsafe {
            let args = [event_h.get_nanbox_f64()];
            let _ = crate::closure::native_call_value_this(
                handler_h.get_nanbox_f64(),
                crate::closure::JsThis::from_f64(port_h.get_nanbox_f64()),
                args.as_ptr(),
                args.len(),
            );
        },
    );
}

/// Macrotask body: deliver exactly one queued message to `port_box`'s port.
/// Scheduled via `setImmediate`, so the event loop pumps it; chains naturally
/// because a handler that posts again schedules a fresh macrotask.
extern "C" fn deliver_one_message(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    port_box: f64,
) -> f64 {
    let port_ptr = crate::value::js_nanbox_get_pointer(port_box) as usize;
    if port_ptr == 0 {
        return js_undefined();
    }

    let Some((lease, handler, listeners)) = with_port_states(|map| {
        let state = map.get(&port_ptr)?;
        (state.owner == crate::agent::current_agent() && state.started)
            .then(|| (state.lease, state.onmessage, state.listeners.clone()))
    }) else {
        return js_undefined();
    };
    // Deserialization can collect; root the owner-heap snapshots first.
    let scope = crate::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(port_box);
    let handler = scope.root_nanbox_f64(handler);
    let listeners: Vec<_> = listeners
        .into_iter()
        .map(|v| scope.root_nanbox_f64(v))
        .collect();
    let Some(message) = port_transport::receive(lease) else {
        return js_undefined();
    };
    let (data, ports) = unsafe { crate::thread::deserialize_message(&message, true) };
    let data_h = scope.root_nanbox_u64(data);
    let ports = scope.root_nanbox_u64(ports);
    let event = crate::event_target_hook_slots::node_message_event(
        data_h.get_nanbox_f64(),
        Some(ports.get_nanbox_f64()),
    );
    let event_h = scope.root_nanbox_f64(event);

    invoke_message_handler(
        handler.get_nanbox_f64(),
        event_h.get_nanbox_f64(),
        port.get_nanbox_f64(),
    );
    for listener in listeners {
        invoke_message_handler(
            listener.get_nanbox_f64(),
            event_h.get_nanbox_f64(),
            port.get_nanbox_f64(),
        );
    }
    js_undefined()
}

/// Posting serializes on the source heap; only the owning agent's pump reads it.
extern "C" fn port_post_message(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    data: f64,
    transfer: f64,
) -> f64 {
    let Some(lease) =
        runtime_port_lease(this.as_f64()).filter(|lease| port_transport::alive(*lease))
    else {
        return js_undefined();
    };
    let message = port_transfer::serialize(data, transfer, Some(lease), |transfer| {
        crate::thread::js_worker_transfer_sequence(transfer)
    });
    match message {
        Ok(message) => port_transfer::send(lease, message),
        Err(error) => port_transfer::throw_data_clone_error(&error),
    }
    js_bool(true)
}

fn start_port(self_ptr: usize) {
    with_port_states(|map| {
        if let Some(state) = map.get_mut(&self_ptr) {
            if port_transport::alive(state.lease) {
                state.started = true;
            }
        }
    });
}

extern "C" fn port_start(_closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    start_port(this_port_ptr(this));
    js_undefined()
}

extern "C" fn port_close(_closure: *const ClosureHeader, this: crate::closure::JsThis) -> f64 {
    if let Some(lease) = runtime_port_lease(this.as_f64()) {
        port_transport::close(lease);
        with_port_states(|map| {
            map.retain(|_, state| {
                state.owner != crate::agent::current_agent() || port_transport::alive(state.lease)
            })
        });
    }
    js_undefined()
}

/// `port.onmessage` getter — return the stored handler (null if unset).
extern "C" fn port_onmessage_get(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let self_ptr = this_port_ptr(this);
    if self_ptr == 0 {
        return js_null();
    }
    with_port_states(|map| map.get(&self_ptr).map(|s| s.onmessage).unwrap_or(js_null()))
}

/// `port.onmessage` setter — store the handler and (per HTML) implicitly
/// start the port, flushing any queued messages.
extern "C" fn port_onmessage_set(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    let self_ptr = this_port_ptr(this);
    if self_ptr == 0 {
        return js_undefined();
    }
    with_port_states(|map| {
        if let Some(state) = map.get_mut(&self_ptr) {
            if port_transport::alive(state.lease) {
                state.onmessage = value;
                state.refed = value_is_callable(value);
            }
        }
    });
    start_port(self_ptr);
    js_undefined()
}

extern "C" fn port_add_event_listener(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
    type_value: f64,
    listener: f64,
) -> f64 {
    let self_ptr = this_port_ptr(this);
    if self_ptr == 0 || !is_message_type(type_value) || !value_is_callable(listener) {
        return js_undefined();
    }
    let mut newly_started = false;
    with_port_states(|map| {
        let Some(state) = map.get_mut(&self_ptr) else {
            return;
        };
        if !port_transport::alive(state.lease) {
            return;
        }
        if !state
            .listeners
            .iter()
            .any(|l| l.to_bits() == listener.to_bits())
        {
            state.listeners.push(listener);
        }
        if !state.started && port_transport::alive(state.lease) {
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
    this: crate::closure::JsThis,
    type_value: f64,
    listener: f64,
) -> f64 {
    let self_ptr = this_port_ptr(this);
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
    port_operations::install_core_methods(obj);
    install_method(
        obj,
        "addEventListener",
        crate::fn_info!(port_add_event_listener, 2; with_declared(2)),
        2,
    );
    install_method(
        obj,
        "removeEventListener",
        crate::fn_info!(port_remove_event_listener, 2; with_declared(2)),
        2,
    );
    set_field(obj, "onmessageerror", js_null());

    // `onmessage` as a getter/setter so assignment starts the port + flushes.
    let getter = closure_value(
        crate::fn_info!(port_onmessage_get, 0; with_declared(0)),
        "get onmessage",
        0,
    );
    let setter = closure_value(
        crate::fn_info!(port_onmessage_set, 1; with_declared(1)),
        "set onmessage",
        1,
    );
    let obj_box = boxed_object(obj);
    let key_box = f64::from_bits(JSValue::string_ptr(key("onmessage")).bits());
    object::js_object_define_accessor(obj_box, key_box, getter, setter);
}

/// Install the Node-shaped prototype members for the three messaging
/// constructors. MessagePort core methods share receiver-owned dispatch with
/// both channel implementations.
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
            port_operations::install_core_methods(proto);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
        }
        "MessageChannel" => {}
        "BroadcastChannel" => {
            broadcast_operations::install_core_methods(proto);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
        }
        _ => {}
    }
}

fn message_port_object() -> *mut ObjectHeader {
    let obj = message_port_object_with_id(0);
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
    same_thread_message_channel_new()
}

/// Test probe (#11471): the same-thread `MessageChannel` this crate builds when
/// perry-stdlib's worker_threads factory is not installed (a stdlib test
/// binary installs it, which would bypass `PORT_STATES`).
#[doc(hidden)]
pub fn same_thread_message_channel_new_for_test() -> f64 {
    same_thread_message_channel_new()
}

fn same_thread_message_channel_new() -> f64 {
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
    _this: crate::closure::JsThis,
) -> f64 {
    crate::fs::validate::throw_type_error_with_code(
        "Cannot call constructor without `new`",
        "ERR_CONSTRUCT_CALL_REQUIRED",
    )
}

#[no_mangle]
pub extern "C" fn js_broadcast_channel_new_counted(name: f64, argc: u32) -> f64 {
    if argc == 0 {
        crate::fs::validate::throw_type_error_with_code(
            "The \"name\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }
    js_broadcast_channel_new(name)
}

#[no_mangle]
pub extern "C" fn js_broadcast_channel_new(name: f64) -> f64 {
    if let Some(factory) = worker_threads_broadcast_channel_factory() {
        return factory(name);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(crate::builtins::js_template_string_coerce_box(name));
    let object = scope.root_raw_mut_ptr(broadcast_channel_object_with_id(0, name.get_nanbox_f64()));
    let constructor = scope.root_nanbox_f64(get_global_constructor("BroadcastChannel"));
    for (name, value) in [
        ("constructor", constructor.get_nanbox_f64()),
        ("onmessage", js_null()),
        ("onmessageerror", js_null()),
    ] {
        let value = scope.root_nanbox_f64(value);
        let name = key(name);
        object::js_object_set_field_by_name(object.get_raw_mut_ptr(), name, value.get_nanbox_f64());
    }
    boxed_object(object.get_raw_mut_ptr())
}

pub(crate) extern "C" fn js_broadcast_channel_constructor_call_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    crate::node_submodules::diagnostics::throw_type_error_no_code(
        b"Class constructor BroadcastChannel cannot be invoked without 'new'",
    )
}

pub(crate) extern "C" fn js_message_port_constructor_call_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    throw_message_port_constructor_error()
}

#[no_mangle]
pub extern "C" fn js_message_port_constructor_error() -> f64 {
    throw_message_port_constructor_error()
}

fn throw_message_port_constructor_error() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "Constructor cannot be called",
        "ERR_CONSTRUCT_CALL_INVALID",
    )
}

#[cfg(test)]
#[path = "messaging/constructor_tests.rs"]
mod constructor_tests;
