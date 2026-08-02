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
        port_bound_closure(port_on as *const u8, 2, port_id),
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
        "addEventListener",
        port_bound_closure(port_add_event_listener as *const u8, 2, port_id),
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
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "unref",
        closure_value(worker_threads_noop0 as *const u8, 0),
    );
    set_object_field(
        obj,
        "hasRef",
        closure_value(worker_threads_has_ref as *const u8, 0),
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

/// port.on(event, callback) / addListener / once (#3157).
extern "C" fn port_on(closure: *const ClosureHeader, event: f64, callback: f64) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    if port_id == PARENT_PORT_HANDLE as u64 && CURRENT_WORKER_ID.with(|id| id.get()) != 0 {
        let callback_ptr = perry_runtime::value::js_nanbox_get_pointer(callback) as i64;
        return js_worker_threads_on(event.to_bits() as i64, callback_ptr);
    }
    let cb_bits = callback.to_bits();
    // A program that only uses MessageChannel never calls spawn_for_promise, so
    // the runtime pump would otherwise never be registered and `main` would
    // return before any queued `message` is delivered. Register it here (mirrors
    // readline #347), so the event loop ticks and drains the inboxes.
    crate::common::async_bridge::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state.message_cb = Some(cb_bits);
                    // Attaching a `message` listener implicitly starts the port.
                    state.started = true;
                }
                "close" => state.close_cb = Some(cb_bits),
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.off(event) / removeListener (#3157).
extern "C" fn port_off(closure: *const ClosureHeader, event: f64, _callback: f64) -> f64 {
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
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => state.message_cb = None,
                "close" => state.close_cb = None,
                _ => {}
            }
        }
    });
    js_undefined()
}

/// port.addEventListener(event, callback) (#3598).
extern "C" fn port_add_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let port_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    crate::common::async_bridge::ensure_pump_registered();
    MESSAGE_PORTS.with(|ports| {
        if let Some(state) = ports.borrow_mut().get_mut(&port_id) {
            match event_name.as_str() {
                "message" => {
                    state.started = true;
                    if !state.message_event_cbs.contains(&cb_bits) {
                        state.message_event_cbs.push(cb_bits);
                    }
                }
                "close" => {
                    if !state.close_event_cbs.contains(&cb_bits) {
                        state.close_event_cbs.push(cb_bits);
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
                "message" => state.message_event_cbs.retain(|cb| *cb != cb_bits),
                "close" => state.close_event_cbs.retain(|cb| *cb != cb_bits),
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
            state.inbox.clear();
        }
        if let Some(peer_id) = peer_id {
            if let Some(peer) = ports.get_mut(&peer_id) {
                if !peer.closed {
                    peer.close_pending = true;
                }
                peer.closed = true;
                peer.inbox.clear();
            }
        }
    });
    js_worker_threads_channels_process_pending();
    js_undefined()
}
