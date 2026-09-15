//! Same-process `MessagePort` object surface (#3157 / #3598).
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim;
//! `message_port_object` is widened to `pub(super)` so the parent module's
//! `MessageChannel` / `moveMessagePortToContext` call sites keep resolving.

use super::*;

/// Build a MessagePort JS object for a same-process channel. The id is also
/// stored on the object (hidden `__perryPortId` field) so `receiveMessageOnPort`
/// can recover it from the object reference.
pub(super) fn message_port_object(port_id: u64) -> *mut perry_runtime::object::ObjectHeader {
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("MessagePort"));
    let object_bits = object_value(obj).to_bits();
    set_object_field(obj, "constructor", get_global_constructor("MessagePort"));
    set_object_field(
        obj,
        "postMessage",
        port_bound_closure(port_post_message as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "on",
        port_bound_closure(port_on as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "addListener",
        port_bound_closure(port_on as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "once",
        port_bound_closure(port_once as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "off",
        port_bound_closure(port_off as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "removeListener",
        port_bound_closure(port_off as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "listenerCount",
        port_bound_closure(port_listener_count as *const u8, 1, port_id),
    );
    set_object_field(
        obj,
        "addEventListener",
        port_bound_closure(port_add_event_listener as *const u8, 3, port_id),
    );
    set_object_field(
        obj,
        "removeEventListener",
        port_bound_closure(port_remove_event_listener as *const u8, 2, port_id),
    );
    set_object_field(
        obj,
        "close",
        port_bound_closure(port_close as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "start",
        port_bound_closure(port_start as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "ref",
        port_bound_closure(port_ref as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "unref",
        port_bound_closure(port_unref as *const u8, 0, port_id),
    );
    set_object_field(
        obj,
        "hasRef",
        port_bound_closure(port_has_ref as *const u8, 0, port_id),
    );
    set_object_field(obj, "__perryPortId", f64::from_bits(port_id));
    set_object_field(obj, "onmessage", js_null());
    set_object_field(obj, "onmessageerror", js_null());
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.object_bits = object_bits;
        }
    });
    obj
}

/// port.postMessage(value) — deliver to the peer port's inbox (#3157).
extern "C" fn port_post_message(closure: *const ClosureHeader, value: f64, _transfer: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    if port_id == PARENT_PORT_HANDLE as u64 && CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        return js_worker_threads_post_message(value);
    }
    // Validate the full submitted graph: a marked object nested inside an
    // otherwise cloneable container rejects the whole message.
    if message_value_is_uncloneable(value, &mut HashSet::new()) {
        throw_data_clone_error("object could not be cloned.");
    }
    let serialized = serialize_message(value);
    MESSAGE_PORTS.with(|ports| {
        let peer = {
            let ports = ports.borrow();
            match ports.get(&port_id) {
                Some(state) if !state.closed => state.peer,
                _ => return,
            }
        };
        if let Some(peer_state) = ports.borrow_mut().get_mut(&peer) {
            if !peer_state.closed {
                peer_state.inbox.push_back(serialized);
            }
        }
    });
    perry_runtime::event_pump::js_notify_main_thread();
    js_undefined()
}

/// port.on(event, callback) / addListener (#3157).
extern "C" fn port_on(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    port_add_node_listener(closure, event, callback, false)
}

/// port.once(event, callback) (#6763).
extern "C" fn port_once(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    port_add_node_listener(closure, event, callback, true)
}

fn port_add_node_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
    once: bool,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    if port_id == PARENT_PORT_HANDLE as u64 && CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        let callback_ptr = perry_runtime::value::js_nanbox_get_pointer(callback) as i64;
        return js_worker_threads_on(event.to_bits() as i64, callback_ptr);
    }
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
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
extern "C" fn port_off(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    if port_id == PARENT_PORT_HANDLE as u64 && CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        match event_name.as_str() {
            "message" => MESSAGE_CALLBACK.with(|cb| *cb.borrow_mut() = None),
            "close" => CLOSE_CALLBACK.with(|cb| *cb.borrow_mut() = None),
            _ => {}
        }
        return js_undefined();
    }
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
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

extern "C" fn port_listener_count(closure: *const ClosureHeader, event: f64) -> f64 {
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
    event: f64,
    callback: f64,
    options: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
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
    event: f64,
    callback: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
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
extern "C" fn port_start(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            state.started = true;
        }
    });
    js_undefined()
}

extern "C" fn port_ref(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    let has_handler = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&port_id)
            .is_some_and(|state| object_event_handler(state.object_bits, "onmessage").is_some())
    });
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            if !state.closed {
                state.refed = true;
                state.onmessage_callable = has_handler;
            }
        }
    });
    js_undefined()
}

extern "C" fn port_unref(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    let has_handler = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .get(&port_id)
            .is_some_and(|state| object_event_handler(state.object_bits, "onmessage").is_some())
    });
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            if !state.closed {
                state.refed = false;
                state.onmessage_callable = has_handler;
            }
        }
    });
    js_undefined()
}

extern "C" fn port_has_ref(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
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
        !state.closed && state.refed
    });
    js_bool(refed)
}

/// port.close() — mark closed and queue `close` events on both ends (#3157).
extern "C" fn port_close(closure: *const ClosureHeader) -> f64 {
    let port_id = port_id_from_closure(closure);
    let peer_id = MESSAGE_PORTS.with(|ports| ports.borrow().get(&port_id).map(|state| state.peer));
    MESSAGE_PORTS.with(|ports| {
        let mut ports = ports.borrow_mut();
        if let Some(state) = ports.get_mut(&port_id) {
            if !state.closed {
                state.close_pending = true;
            }
            state.closed = true;
            state.refed = false;
            state.inbox.clear();
        }
        if let Some(peer_id) = peer_id {
            if let Some(peer) = ports.get_mut(&peer_id) {
                if !peer.closed {
                    peer.close_pending = true;
                }
                peer.closed = true;
                peer.refed = false;
                peer.inbox.clear();
            }
        }
    });
    js_worker_threads_channels_process_pending();
    js_undefined()
}
