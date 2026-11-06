//! Event-loop pump for `MessageChannel` / `BroadcastChannel` inboxes.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap. These two `#[no_mangle]` entry points are called from the async bridge
//! pump (`common::async_bridge`) and are re-exported from the parent module so
//! `crate::worker_threads::js_worker_threads_channels_*` keeps resolving.

use super::{
    call_callback1, call_message_callback1, deserialize_nanbox_on_current_thread, event_object,
    object_event_handler, EventListener, SerializedValue, BROADCAST_CHANNELS, MESSAGE_PORTS,
};

/// Drain queued MessageChannel inboxes, dispatching to `message` listeners and
/// firing `close` events for closed ports. Called from the event-loop pump.
/// Returns the number of messages/events dispatched (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_channels_process_pending() -> i32 {
    super::message_port::observe_closed_ports();
    let mut dispatched = 0;

    // Snapshot deliverable (port_id, callback, message) tuples, then invoke the
    // callbacks OUTSIDE the MESSAGE_PORTS borrow — a listener may re-enter
    // postMessage / close, which needs to borrow MESSAGE_PORTS again.
    struct MessageDispatch {
        target_bits: u64,
        raw_cbs: Vec<EventListener>,
        event_cbs: Vec<EventListener>,
        handler_cb: Option<u64>,
        global_handler: Option<u64>,
        msg: SerializedValue,
        async_ids: Option<perry_runtime::async_hook_slots::AsyncResourceIds>,
    }

    loop {
        let candidates: Vec<(u64, u64)> = MESSAGE_PORTS.with(|ports| {
            ports
                .borrow()
                .iter()
                .filter_map(|(port_id, state)| {
                    (!state.closed() && state.pending()).then_some((*port_id, state.object_bits))
                })
                .collect()
        });
        let mut next: Option<MessageDispatch> = None;
        for (port_id, target_bits) in candidates {
            let handler_cb = object_event_handler(target_bits, "onmessage");
            let global_handler = (port_id == super::parent_port::id())
                .then(|| super::worker_surface::web_worker_global_handler("onmessage"))
                .flatten();
            next = MESSAGE_PORTS.with(|ports| {
                let mut ports = ports.borrow_mut();
                let state = ports.get_mut(&port_id)?;
                let has_event_target = state.started
                    && (!state.message_cbs.is_empty() || !state.message_event_cbs.is_empty());
                if state.closed()
                    || (!has_event_target && handler_cb.is_none() && global_handler.is_none())
                {
                    return None;
                }
                super::port_transport::receive(state.lease).map(|msg| {
                    let raw_cbs = state.message_cbs.clone();
                    state.message_cbs.retain(|listener| !listener.once);
                    let event_cbs = state.message_event_cbs.clone();
                    state.message_event_cbs.retain(|listener| !listener.once);
                    if state.message_cbs.is_empty()
                        && state.message_event_cbs.is_empty()
                        && handler_cb.is_none()
                    {
                        state.refed = false;
                    }
                    MessageDispatch {
                        target_bits: state.object_bits,
                        raw_cbs,
                        event_cbs,
                        handler_cb,
                        global_handler,
                        msg,
                        async_ids: state.async_ids,
                    }
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                // The listeners run inside the port's MESSAGEPORT resource.
                let async_ids = dispatch.async_ids;
                if let Some(ids) = async_ids {
                    perry_runtime::async_hook_slots::enter_resource_scope(ids);
                }
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let target = scope.root_nanbox_f64(f64::from_bits(dispatch.target_bits));
                let raw_cbs = dispatch
                    .raw_cbs
                    .into_iter()
                    .map(|listener| scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)))
                    .collect::<Vec<_>>();
                let event_cbs = dispatch
                    .event_cbs
                    .into_iter()
                    .map(|listener| {
                        (
                            scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)),
                            listener.global_event,
                        )
                    })
                    .collect::<Vec<_>>();
                let handler_cb = dispatch
                    .handler_cb
                    .map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
                let global_handler = dispatch
                    .global_handler
                    .map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
                let global = (global_handler.is_some()
                    || event_cbs.iter().any(|(_, global)| *global))
                .then(|| scope.root_nanbox_f64(perry_runtime::object::js_get_global_this()));
                let (value, ports) =
                    unsafe { perry_runtime::thread::deserialize_message(&dispatch.msg, true) };
                let value = scope.root_nanbox_u64(value);
                let ports = scope.root_nanbox_u64(ports);
                for callback in raw_cbs {
                    call_callback1(
                        callback.get_nanbox_f64().to_bits(),
                        target.get_nanbox_f64().to_bits(),
                        value.get_nanbox_f64(),
                    );
                }
                let global_event = global.as_ref().map(|_| {
                    let event = perry_runtime::event_target_hook_slots::message_event_with_ports(
                        value.get_nanbox_f64(),
                        ports.get_nanbox_f64(),
                    );
                    scope.root_nanbox_f64(event)
                });
                if !event_cbs.is_empty() || handler_cb.is_some() {
                    let event = perry_runtime::event_target_hook_slots::node_message_event(
                        value.get_nanbox_f64(),
                        Some(ports.get_nanbox_f64()),
                    );
                    let event = scope.root_nanbox_f64(event);
                    for (callback, is_global) in event_cbs {
                        let (receiver, event) = if is_global {
                            (global.as_ref().unwrap(), global_event.as_ref().unwrap())
                        } else {
                            (&target, &event)
                        };
                        call_message_callback1(
                            callback.get_nanbox_f64().to_bits(),
                            receiver.get_nanbox_f64().to_bits(),
                            event.get_nanbox_f64(),
                        );
                    }
                    if let Some(callback) = handler_cb {
                        call_message_callback1(
                            callback.get_nanbox_f64().to_bits(),
                            target.get_nanbox_f64().to_bits(),
                            event.get_nanbox_f64(),
                        );
                    }
                }
                if let (Some(handler), Some(global), Some(event)) =
                    (global_handler, global, global_event)
                {
                    call_message_callback1(
                        handler.get_nanbox_f64().to_bits(),
                        global.get_nanbox_f64().to_bits(),
                        event.get_nanbox_f64(),
                    );
                }
                if let Some(ids) = async_ids {
                    perry_runtime::async_hook_slots::leave_resource_scope(ids.async_id);
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    loop {
        let next = BROADCAST_CHANNELS.with(|channels| {
            channels
                .borrow_mut()
                .values_mut()
                .find_map(|state| state.inbox.pop_front().map(|msg| (state.object_bits, msg)))
        });
        let Some((target_bits, msg)) = next else {
            break;
        };
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(f64::from_bits(target_bits));
        let value = f64::from_bits(unsafe { deserialize_nanbox_on_current_thread(&msg) });
        let event = perry_runtime::event_target_hook_slots::node_message_event(value, None);
        let event = scope.root_nanbox_f64(event);
        unsafe {
            perry_runtime::event_target_hook_slots::dispatch_event(
                perry_runtime::value::js_nanbox_get_pointer(target.get_nanbox_f64())
                    as *mut perry_runtime::ObjectHeader,
                event.get_nanbox_f64(),
            );
        }
        dispatched += 1;
    }

    // Fire `close` callbacks once for newly-closed ports.
    struct CloseDispatch {
        lease: super::Lease,
        target_bits: u64,
        raw_cbs: Vec<EventListener>,
        event_cbs: Vec<EventListener>,
    }

    let close_events: Vec<CloseDispatch> = MESSAGE_PORTS.with(|ports| {
        let mut events = Vec::new();
        for state in ports.borrow_mut().values_mut() {
            if state.close_pending {
                state.close_pending = false;
                if let Some(ids) = state.async_ids.take() {
                    perry_runtime::async_hook_slots::destroy(ids.async_id);
                }
                let raw_cbs = state.close_cbs.clone();
                state.close_cbs.retain(|listener| !listener.once);
                let event_cbs = state.close_event_cbs.clone();
                state.close_event_cbs.retain(|listener| !listener.once);
                events.push(CloseDispatch {
                    lease: state.lease,
                    target_bits: state.object_bits,
                    raw_cbs,
                    event_cbs,
                });
            }
        }
        events
    });
    for event in close_events {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let target = scope.root_nanbox_f64(f64::from_bits(event.target_bits));
        let raw_cbs = event
            .raw_cbs
            .into_iter()
            .map(|listener| scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)))
            .collect::<Vec<_>>();
        let event_cbs = event
            .event_cbs
            .into_iter()
            .map(|listener| scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)))
            .collect::<Vec<_>>();
        super::port_transport::close(event.lease);
        if !raw_cbs.is_empty() || !event_cbs.is_empty() {
            let close_event = event_object("close", target.get_nanbox_f64().to_bits(), None);
            let close_event = scope.root_nanbox_f64(close_event);
            for callback in raw_cbs {
                call_callback1(
                    callback.get_nanbox_f64().to_bits(),
                    target.get_nanbox_f64().to_bits(),
                    close_event.get_nanbox_f64(),
                );
            }
            for callback in event_cbs {
                call_callback1(
                    callback.get_nanbox_f64().to_bits(),
                    target.get_nanbox_f64().to_bits(),
                    close_event.get_nanbox_f64(),
                );
            }
        }
        dispatched += 1;
    }

    dispatched
}

/// Keep the event loop alive while any MessageChannel port still has a started
/// `message` listener with queued or potentially-incoming messages (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_channels_has_pending() -> i32 {
    super::message_port::observe_closed_ports();
    // Runs after every worker callback (#12448): no allocation for the usual
    // handful of ports. `port_is_refed` may run an `onmessage` getter, so the
    // ids are copied out of the borrow first.
    let mut inline = [0u64; 8];
    let mut spilled = Vec::new();
    let count = MESSAGE_PORTS.with(|ports| {
        let ports = ports.borrow();
        if ports.len() > inline.len() {
            spilled.extend(ports.keys().copied());
        } else {
            for (slot, id) in inline.iter_mut().zip(ports.keys()) {
                *slot = *id;
            }
        }
        ports.len()
    });
    let port_ids = if spilled.is_empty() {
        &inline[..count]
    } else {
        &spilled[..]
    };
    if port_ids
        .iter()
        .copied()
        .any(super::message_port::port_is_refed)
    {
        return 1;
    }
    let pending_without_onmessage = MESSAGE_PORTS.with(|ports| {
        ports.borrow().values().any(|state| {
            let has_event_target = state.started
                && (!state.message_cbs.is_empty() || !state.message_event_cbs.is_empty());
            (!state.closed() && state.pending() && has_event_target) || state.close_pending
        })
    });
    if pending_without_onmessage {
        return 1;
    }

    let onmessage_targets: Vec<u64> = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .values()
            .filter_map(|state| (!state.closed() && state.pending()).then_some(state.object_bits))
            .collect()
    });
    if onmessage_targets
        .into_iter()
        .any(|target_bits| object_event_handler(target_bits, "onmessage").is_some())
    {
        return 1;
    }

    BROADCAST_CHANNELS.with(|channels| {
        channels
            .borrow()
            .values()
            .any(|state| !state.inbox.is_empty()) as i32
    })
}
