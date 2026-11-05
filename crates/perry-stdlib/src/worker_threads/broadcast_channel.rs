//! Same-process `BroadcastChannel` surface.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap (`scripts/check_file_size.sh`). Items are moved verbatim; the
//! `#[no_mangle]` constructor is re-exported from the parent module so
//! `crate::worker_threads::js_worker_threads_broadcast_channel_new` (and the
//! `pub use worker_threads::*` glob in `lib.rs`) keeps resolving it.

use super::*;

unsafe extern "C" fn broadcast_post_message(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    args: *const f64,
    len: usize,
) -> f64 {
    if len == 0 {
        perry_runtime::fs::validate::throw_type_error_with_code(
            "The \"message\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }
    let value = unsafe { *args };
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
    let serialized = clone_message(value, js_undefined());
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

extern "C" fn broadcast_close(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
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

extern "C" fn broadcast_ref_or_unref(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    BROADCAST_CHANNELS.with(|channels| match channels.borrow().get(&channel_id) {
        Some(state) => f64::from_bits(state.object_bits),
        None => js_undefined(),
    })
}

extern "C" fn broadcast_add_event_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    event: f64,
    callback: f64,
    options: f64,
) -> f64 {
    let channel_id = port_id_from_closure(closure);
    let event_name = string_value_to_string(event).unwrap_or_default();
    let Some(cb_bits) = callback_bits_from_value(callback) else {
        return js_undefined();
    };
    super::async_shim::ensure_pump_registered();
    BROADCAST_CHANNELS.with(|channels| {
        if let Some(state) = channels.borrow_mut().get_mut(&channel_id) {
            if event_name == "message"
                && !state
                    .message_event_cbs
                    .iter()
                    .any(|listener| listener.callback_bits == cb_bits)
            {
                state.message_event_cbs.push(EventListener {
                    callback_bits: cb_bits,
                    once: listener_once(options),
                    global_event: false,
                });
            }
        }
    });
    js_undefined()
}

extern "C" fn broadcast_remove_event_listener(
    closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
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
                state
                    .message_event_cbs
                    .retain(|listener| listener.callback_bits != cb_bits);
            }
        }
    });
    js_undefined()
}

/// new worker_threads.BroadcastChannel(name)
#[no_mangle]
pub extern "C" fn js_worker_threads_broadcast_channel_new(name: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let name_value = scope.root_nanbox_f64(perry_runtime::builtins::js_template_string_coerce_box(
        name.get_nanbox_f64(),
    ));
    let name_string = string_value_to_string(name_value.get_nanbox_f64()).unwrap_or_default();
    ensure_environment_data_gc_scanner();
    super::async_shim::ensure_pump_registered();
    let id = NEXT_BROADCAST_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    });
    let obj = scope.root_raw_mut_ptr(perry_runtime::object::js_object_alloc(0, 0));
    let prototype = constructor_prototype("BroadcastChannel");
    set_object_prototype(obj.get_raw_mut_ptr(), prototype);
    let set = |name: &str, value: f64| {
        let value = scope.root_nanbox_f64(value);
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_set_field_by_name(
            obj.get_raw_mut_ptr(),
            key,
            value.get_nanbox_f64(),
        );
    };
    set("constructor", get_global_constructor("BroadcastChannel"));
    set(
        "postMessage",
        port_bound_closure(
            perry_runtime::fn_info!(native_args broadcast_post_message, 1),
            id,
        ),
    );
    set(
        "close",
        port_bound_closure(
            perry_runtime::fn_info!(broadcast_close, 0; with_declared(0)),
            id,
        ),
    );
    set(
        "ref",
        port_bound_closure(
            perry_runtime::fn_info!(broadcast_ref_or_unref, 0; with_declared(0)),
            id,
        ),
    );
    set(
        "unref",
        port_bound_closure(
            perry_runtime::fn_info!(broadcast_ref_or_unref, 0; with_declared(0)),
            id,
        ),
    );
    set(
        "addEventListener",
        port_bound_closure(
            perry_runtime::fn_info!(broadcast_add_event_listener, 3; with_declared(3)),
            id,
        ),
    );
    set(
        "removeEventListener",
        port_bound_closure(
            perry_runtime::fn_info!(broadcast_remove_event_listener, 2; with_declared(2)),
            id,
        ),
    );
    set("onmessage", js_null());
    set("onmessageerror", js_null());
    set("name", name_value.get_nanbox_f64());
    let object_bits = object_value(obj.get_raw_mut_ptr()).to_bits();
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
    object_value(obj.get_raw_mut_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_names_leave_broadcast_admission_unchanged() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let symbol = scope.root_nanbox_f64(unsafe { perry_runtime::symbol::js_symbol_new_empty() });
        let before = (
            NEXT_BROADCAST_ID.with(|id| *id.borrow()),
            BROADCAST_CHANNELS.with(|channels| channels.borrow().len()),
        );
        assert!(perry_runtime::exception::catch_js_throw(|| {
            js_worker_threads_broadcast_channel_new(symbol.get_nanbox_f64())
        })
        .is_err());
        assert!(perry_runtime::exception::catch_js_throw(|| {
            perry_runtime::messaging::js_broadcast_channel_new_counted(js_undefined(), 0)
        })
        .is_err());
        assert_eq!(
            before,
            (
                NEXT_BROADCAST_ID.with(|id| *id.borrow()),
                BROADCAST_CHANNELS.with(|channels| channels.borrow().len()),
            )
        );
        let value = scope.root_nanbox_f64(js_worker_threads_broadcast_channel_new(js_undefined()));
        assert_eq!(
            BROADCAST_CHANNELS.with(|channels| channels
                .borrow()
                .get(&before.0)
                .unwrap()
                .name
                .clone()),
            "undefined"
        );
        BROADCAST_CHANNELS.with(|channels| {
            let removed = channels.borrow_mut().remove(&before.0).unwrap();
            assert_eq!(removed.object_bits, value.get_nanbox_f64().to_bits());
        });
    }
}
