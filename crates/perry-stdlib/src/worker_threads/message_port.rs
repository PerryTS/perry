//! Same-process `MessagePort` object surface (#3157 / #3598).
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim;
//! `message_port_object` is widened to `pub(super)` so the parent module's
//! `MessageChannel` / `moveMessagePortToContext` call sites keep resolving.

use super::*;

/// Build a MessagePort around its existing state ID.
pub(super) fn message_port_object(port_id: u64) -> *mut perry_runtime::object::ObjectHeader {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(perry_runtime::messaging::message_port_object_with_id(
        port_id,
    ));
    let prototype = constructor_prototype("MessagePort");
    set_object_prototype(object.get_raw_mut_ptr(), prototype);
    let set = |name: &str, value: f64| {
        let value = scope.root_nanbox_f64(value);
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_set_field_by_name(
            object.get_raw_mut_ptr(),
            key,
            value.get_nanbox_f64(),
        );
    };
    set("constructor", get_global_constructor("MessagePort"));

    set(
        "on",
        port_bound_closure(
            perry_runtime::fn_info!(port_on, 2; with_declared(2)),
            port_id,
        ),
    );
    set(
        "addListener",
        port_bound_closure(
            perry_runtime::fn_info!(port_on, 2; with_declared(2)),
            port_id,
        ),
    );
    set(
        "once",
        port_bound_closure(
            perry_runtime::fn_info!(port_once, 2; with_declared(2)),
            port_id,
        ),
    );
    set(
        "off",
        port_bound_closure(
            perry_runtime::fn_info!(port_off, 2; with_declared(2)),
            port_id,
        ),
    );
    set(
        "removeListener",
        port_bound_closure(
            perry_runtime::fn_info!(port_off, 2; with_declared(2)),
            port_id,
        ),
    );
    set(
        "listenerCount",
        port_bound_closure(
            perry_runtime::fn_info!(port_listener_count, 1; with_declared(1)),
            port_id,
        ),
    );
    set(
        "addEventListener",
        port_bound_closure(
            perry_runtime::fn_info!(port_add_event_listener, 3; with_declared(3)),
            port_id,
        ),
    );
    set(
        "removeEventListener",
        port_bound_closure(
            perry_runtime::fn_info!(port_remove_event_listener, 2; with_declared(2)),
            port_id,
        ),
    );

    perry_runtime::messaging::install_message_port_core_methods(object.get_raw_mut_ptr());
    set("onmessage", js_null());
    set("onmessageerror", js_null());
    let object_bits = object_value(object.get_raw_mut_ptr()).to_bits();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.object_bits = object_bits;
        }
    });
    object.get_raw_mut_ptr()
}

/// port.postMessage(value) — deliver to the peer port's inbox (#3157).
pub(crate) fn register_codec() {
    perry_runtime::messaging::port_transfer::register(lookup_lease, adopt_port);
    port_transport::register_worker_notice(|agent, worker_id| {
        push_parent_event(agent, WorkerEvent::DataReady(worker_id))
    });
}
fn lookup_lease(value: f64) -> Option<Lease> {
    let id = port_id_from_object(value)?;
    MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&id)
            .filter(|state| state.object_bits == value.to_bits())
            .map(|state| state.lease)
    })
}
fn adopt_port(lease: Lease) -> f64 {
    ensure_environment_data_gc_scanner();
    super::async_shim::ensure_pump_registered();
    let id = next_port_id();
    MESSAGE_PORTS.with(|ports| {
        ports.borrow_mut().insert(
            id,
            MessagePortState {
                lease,
                ..Default::default()
            },
        );
    });
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let port = scope.root_nanbox_f64(object_value(message_port_object(id)));
    let ids =
        perry_runtime::async_hook_slots::init_resource("MESSAGEPORT", port.get_nanbox_f64(), true);
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&id) {
            state.async_ids = Some(ids);
        }
    });
    port.get_nanbox_f64()
}
pub(super) fn new_owned_port(lease: Lease) -> f64 {
    adopt_port(lease)
}
pub(super) fn port_post_message(port_id: u64, value: f64, transfer: f64) -> f64 {
    let lease = MESSAGE_PORTS.with(|ports| ports.borrow().get(&port_id).map(|state| state.lease));
    let Some(lease) = lease.filter(|lease| port_transport::alive(*lease)) else {
        return js_undefined();
    };
    let message = super::clone::clone_message_from_port(value, transfer, lease);
    perry_runtime::messaging::port_transfer::send(lease, message);
    js_bool(true)
}

/// port.on(event, callback) / addListener (#3157).
extern "C" fn port_on(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    port_add_node_listener(closure, event, callback, false)
}

/// port.once(event, callback) (#6763).
extern "C" fn port_once(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    port_add_node_listener(closure, event, callback, true)
}

fn port_add_node_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
    once: bool,
) -> f64 {
    add_node_listener(port_id_from_closure(closure), event, callback, once)
}

fn listener_bits(value: f64) -> Option<u64> {
    let listener = JSValue::from_bits(value.to_bits());
    let object =
        listener.is_pointer() && unsafe { perry_runtime::symbol::js_is_symbol(value) == 0 };
    if !listener.is_undefined() && !listener.is_null() && !object {
        perry_runtime::validators::throw_invalid_arg_type(
            "listener",
            "an instance of EventListener",
            value,
        );
    }
    callback_bits_from_value(value)
}

fn added_listener_bits(value: f64) -> Option<u64> {
    let callback = listener_bits(value);
    let listener = JSValue::from_bits(value.to_bits());
    if listener.is_null() || listener.is_undefined() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let kind = if listener.is_null() {
            "null"
        } else {
            "undefined"
        };
        let message = scope.root_nanbox_f64(string_value(&format!(
            "addEventListener called with {kind} which has no effect."
        )));
        let name = scope.root_nanbox_f64(string_value("AddEventListenerArgumentTypeWarning"));
        perry_runtime::process::js_process_emit_warning(
            message.get_nanbox_f64(),
            name.get_nanbox_f64(),
            js_undefined(),
        );
    }
    callback
}

pub(super) fn add_node_listener(port_id: u64, event: f64, callback: f64, once: bool) -> f64 {
    let Some(cb_bits) = added_listener_bits(callback) else {
        return js_undefined();
    };
    let event_name = string_value_to_string(event).unwrap_or_default();
    // A program that only uses MessageChannel never calls spawn_for_promise, so
    // the runtime pump would otherwise never be registered and `main` would
    // return before any queued `message` is delivered. Register it here (mirrors
    // readline #347), so the event loop ticks and drains the inboxes.
    super::async_shim::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    let first_listener = state.message_cbs.is_empty()
                        && state.message_event_cbs.is_empty()
                        && object_event_handler(state.object_bits, "onmessage").is_none();
                    if !state
                        .message_cbs
                        .iter()
                        .any(|listener| listener.callback_bits == cb_bits)
                    {
                        state.message_cbs.push(EventListener {
                            callback_bits: cb_bits,
                            once,
                            global_event: false,
                        });
                        if first_listener {
                            state.refed = true;
                        }
                    }
                    // Attaching a `message` listener implicitly starts the port.
                    state.started = true;
                }
                "close" => {
                    if !state
                        .close_cbs
                        .iter()
                        .any(|listener| listener.callback_bits == cb_bits)
                    {
                        state.close_cbs.push(EventListener {
                            callback_bits: cb_bits,
                            once,
                            global_event: false,
                        });
                    }
                }
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.off(event) / removeListener (#3157).
extern "C" fn port_off(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let Some(cb_bits) = listener_bits(callback) else {
        return js_undefined();
    };
    let event_name = string_value_to_string(event).unwrap_or_default();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state
                        .message_cbs
                        .retain(|listener| listener.callback_bits != cb_bits);
                    if state.message_cbs.is_empty()
                        && state.message_event_cbs.is_empty()
                        && object_event_handler(state.object_bits, "onmessage").is_none()
                    {
                        state.refed = false;
                    }
                }
                "close" => state
                    .close_cbs
                    .retain(|listener| listener.callback_bits != cb_bits),
                _ => {}
            }
        }
    });
    js_undefined()
}

extern "C" fn port_listener_count(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    MESSAGE_PORTS.with(|ports| {
        let ports = ports.borrow();
        let Some(state) = ports.get(&port_id) else {
            return 0.0;
        };
        match event_name.as_str() {
            "message" => {
                (state.message_cbs.len()
                    + state.message_event_cbs.len()
                    + if object_event_handler(state.object_bits, "onmessage").is_some() {
                        1
                    } else {
                        0
                    }) as f64
            }
            "close" => (state.close_cbs.len() + state.close_event_cbs.len()) as f64,
            _ => 0.0,
        }
    })
}

/// port.addEventListener(event, callback) (#3598).
extern "C" fn port_add_event_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
    options: f64,
) -> f64 {
    add_event_listener(
        port_id_from_closure(closure),
        event,
        callback,
        options,
        false,
    )
}

pub(super) fn add_event_listener(
    port_id: u64,
    event: f64,
    callback: f64,
    options: f64,
    global_event: bool,
) -> f64 {
    let Some(cb_bits) = added_listener_bits(callback) else {
        return js_undefined();
    };
    let event_name = string_value_to_string(event).unwrap_or_default();
    super::async_shim::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    let first_listener = state.message_cbs.is_empty()
                        && state.message_event_cbs.is_empty()
                        && object_event_handler(state.object_bits, "onmessage").is_none();
                    state.started = true;
                    if !state
                        .message_event_cbs
                        .iter()
                        .any(|listener| listener.callback_bits == cb_bits)
                    {
                        state.message_event_cbs.push(EventListener {
                            callback_bits: cb_bits,
                            once: listener_once(options),
                            global_event,
                        });
                        if first_listener {
                            state.refed = true;
                        }
                    }
                }
                "close" => {
                    if !state
                        .close_event_cbs
                        .iter()
                        .any(|listener| listener.callback_bits == cb_bits)
                    {
                        state.close_event_cbs.push(EventListener {
                            callback_bits: cb_bits,
                            once: listener_once(options),
                            global_event,
                        });
                    }
                }
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.removeEventListener(event, callback) (#3598).
extern "C" fn port_remove_event_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
) -> f64 {
    remove_event_listener(port_id_from_closure(closure), event, callback)
}

pub(super) fn remove_event_listener(port_id: u64, event: f64, callback: f64) -> f64 {
    let Some(cb_bits) = listener_bits(callback) else {
        return js_undefined();
    };
    let event_name = string_value_to_string(event).unwrap_or_default();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state
                        .message_event_cbs
                        .retain(|listener| listener.callback_bits != cb_bits);
                    if state.message_cbs.is_empty()
                        && state.message_event_cbs.is_empty()
                        && object_event_handler(state.object_bits, "onmessage").is_none()
                    {
                        state.refed = false;
                    }
                }
                "close" => state
                    .close_event_cbs
                    .retain(|listener| listener.callback_bits != cb_bits),
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.start() — enable delivery of queued messages to the listener (#3157).
fn port_start(port_id: u64) -> f64 {
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.started = true;
        }
    });
    js_undefined()
}

fn set_port_ref(port_id: u64, refed: bool) -> f64 {
    let has_handler = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&port_id)
            .is_some_and(|state| object_event_handler(state.object_bits, "onmessage").is_some())
    });
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            if !state.closed() {
                state.refed = refed;
                state.onmessage_callable = has_handler;
            }
        }
    });
    js_undefined()
}

pub(super) fn port_is_refed(port_id: u64) -> bool {
    let has_handler = MESSAGE_PORTS.with(|ports| {
        let ports = ports.borrow();
        ports
            .get(&port_id)
            .is_some_and(|state| object_event_handler(state.object_bits, "onmessage").is_some())
    });
    let refed = MESSAGE_PORTS.with(|ports| {
        let mut ports = ports.borrow_mut();
        let Some(state) = ports.get_mut(&port_id) else {
            return false;
        };
        if has_handler != state.onmessage_callable {
            state.onmessage_callable = has_handler;
            state.refed = has_handler;
        }
        !state.closed() && state.refed
    });
    refed
}

pub(super) fn observe_closed_ports() {
    MESSAGE_PORTS.with(|ports| {
        for state in ports.borrow_mut().values_mut() {
            if !state.close_seen && port_transport::closed(state.lease) {
                state.close_seen = true;
                state.close_pending = true;
                state.refed = false;
            }
        }
    });
}

/// port.close() — mark closed and queue `close` events on both ends (#3157).
pub(super) fn close_port(port_id: u64) -> f64 {
    let lease = MESSAGE_PORTS.with(|ports| ports.borrow().get(&port_id).map(|state| state.lease));
    if let Some(lease) = lease.filter(|lease| port_transport::alive(*lease)) {
        port_transport::close(lease);
        observe_closed_ports();
    }
    perry_runtime::event_pump::js_notify_main_thread();
    js_undefined()
}

pub(crate) unsafe extern "C" fn dispatch_core_operation(
    operation: perry_runtime::messaging::MessagePortOperation,
    receiver: f64,
    value: f64,
    transfer: f64,
    result: *mut f64,
) -> bool {
    let Some(id) = port_id_from_object(receiver) else {
        return false;
    };
    let recognized = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&id)
            .is_some_and(|state| state.object_bits == receiver.to_bits())
    });
    if !recognized {
        return false;
    }
    use perry_runtime::messaging::MessagePortOperation;
    *result = match operation {
        MessagePortOperation::PostMessage => port_post_message(id, value, transfer),
        MessagePortOperation::Start => port_start(id),
        MessagePortOperation::Close => {
            if callback_bits_from_value(value).is_some() {
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let callback = scope.root_nanbox_f64(value);
                let event = js_string_from_bytes(b"close".as_ptr(), 5);
                add_node_listener(
                    id,
                    f64::from_bits(JSValue::string_ptr(event).bits()),
                    callback.get_nanbox_f64(),
                    true,
                );
            }
            close_port(id)
        }
        MessagePortOperation::Ref => set_port_ref(id, true),
        MessagePortOperation::Unref => set_port_ref(id, false),
        MessagePortOperation::HasRef => js_bool(port_is_refed(id)),
    };
    true
}
