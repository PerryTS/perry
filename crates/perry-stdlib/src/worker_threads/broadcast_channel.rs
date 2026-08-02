//! Same-process `BroadcastChannel` surface.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim; the
//! `#[no_mangle]` constructor is re-exported from the parent module so
//! `crate::worker_threads::js_worker_threads_broadcast_channel_new` (and the
//! `pub use worker_threads::*` glob in `lib.rs`) keeps resolving it.

use super::*;

extern "C" fn broadcast_post_message(closure: *const ClosureHeader, value: f64) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let channel_name = BROADCAST_CHANNELS.with(|channels| {
        channels
            .borrow()
            .get(&channel_id)
            .and_then(|state| (!state.closed).then(|| state.name.clone()))
    });
    let Some(channel_name) = channel_name else {
        throw_invalid_state_error("BroadcastChannel is closed");
    };
    if message_value_is_uncloneable(value, &mut HashSet::new()) {
        throw_data_clone_error("object could not be cloned.");
    }
    let serialized = serialize_message(value);
    BROADCAST_CHANNELS.with(|channels| {
        for (id, state) in channels.borrow_mut().iter_mut() {
            if *id != channel_id && !state.closed && state.name == channel_name {
                state.inbox.push_back(serialized.clone());
            }
        }
    });
    queue_worker_threads_microtask();
    js_undefined()
}

extern "C" fn broadcast_close(closure: *const ClosureHeader) -> f64 {
    let channel_id = port_id_from_closure(closure);
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            state.closed = true;
            state.inbox.clear();
            state.message_event_cbs.clear();
        }
    });
    js_undefined()
}

extern "C" fn broadcast_ref_or_unref(closure: *const ClosureHeader) -> f64 {
    let channel_id = port_id_from_closure(closure);
    BROADCAST_CHANNELS.with(|channels| match channels.borrow().get(&channel_id) {
        Some(state) => f64::from_bits(state.object_bits),
        None => js_undefined(),
    })
}

extern "C" fn broadcast_add_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    crate::common::async_bridge::ensure_pump_registered();
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            if event_name == "message" && !state.message_event_cbs.contains(&cb_bits) {
                state.message_event_cbs.push(cb_bits);
            }
        }
    });
    js_undefined()
}

extern "C" fn broadcast_remove_event_listener(
    closure: *const ClosureHeader,
    event: f64,
    callback: f64,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            if event_name == "message" {
                state.message_event_cbs.retain(|cb| *cb != cb_bits);
            }
        }
    });
    js_undefined()
}

/// new worker_threads.BroadcastChannel(name)
#[no_mangle]
pub extern "C" fn js_worker_threads_broadcast_channel_new(name: f64) -> f64 {
    ensure_environment_data_gc_scanner();
    crate::common::async_bridge::ensure_pump_registered();
    let id = NEXT_BROADCAST_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    let name_value = string_coerce(name);
    let name_string = string_value_to_string(name_value).unwrap_or_default();
    let obj = perry_runtime::object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("BroadcastChannel"));
    let object_bits = object_value(obj).to_bits();
    set_object_field(
        obj,
        "constructor",
        get_global_constructor("BroadcastChannel"),
    );
    set_object_field(
        obj,
        "postMessage",
        port_bound_closure(broadcast_post_message as *const u8, 1, id),
    );
    set_object_field(
        obj,
        "close",
        port_bound_closure(broadcast_close as *const u8, 0, id),
    );
    set_object_field(
        obj,
        "ref",
        port_bound_closure(broadcast_ref_or_unref as *const u8, 0, id),
    );
    set_object_field(
        obj,
        "unref",
        port_bound_closure(broadcast_ref_or_unref as *const u8, 0, id),
    );
    set_object_field(
        obj,
        "addEventListener",
        port_bound_closure(broadcast_add_event_listener as *const u8, 2, id),
    );
    set_object_field(
        obj,
        "removeEventListener",
        port_bound_closure(broadcast_remove_event_listener as *const u8, 2, id),
    );
    set_object_field(obj, "onmessage", js_null());
    set_object_field(obj, "onmessageerror", js_null());
    set_object_field(obj, "name", name_value);
    set_object_field(obj, "__perryBroadcastChannelId", f64::from_bits(id));
    BROADCAST_CHANNELS.with(|channels| {
        channels.borrow_mut().insert(
            id,
            BroadcastChannelState {
                name: name_string,
                object_bits,
                ..Default::default()
            },
        );
    });
    object_value(obj)
}
