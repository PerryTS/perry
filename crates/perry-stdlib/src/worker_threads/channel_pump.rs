//! Event-loop pump for `MessageChannel` / `BroadcastChannel` inboxes.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap. These two `#[no_mangle]` entry points are called from the async bridge
//! pump (`common::async_bridge`) and are re-exported from the parent module so
//! `crate::worker_threads::js_worker_threads_channels_*` keeps resolving.

use super::{
    call_callback1, deserialize_message, event_object, object_event_handler, EventListener,
    SerializedMessage, BROADCAST_CHANNELS, MESSAGE_PORTS,
};

/// Drain queued MessageChannel inboxes, dispatching to `message` listeners and
/// firing `close` events for closed ports. Called from the event-loop pump.
/// Returns the number of messages/events dispatched (#3157).
#[no_mangle]
pub extern "C" fn js_worker_threads_channels_process_pending() -> i32 {
    let mut dispatched = 0;

    // Snapshot deliverable (port_id, callback, message) tuples, then invoke the
    // callbacks OUTSIDE the MESSAGE_PORTS borrow — a listener may re-enter
    // postMessage / close, which needs to borrow MESSAGE_PORTS again.
    struct MessageDispatch {
        target_bits: u64,
        raw_cbs: Vec<EventListener>,
        event_cbs: Vec<EventListener>,
        handler_cb: Option<u64>,
        msg: SerializedMessage,
    }

    loop {
        let candidates: Vec<(u64, u64)> = MESSAGE_PORTS.with(|ports| {
            ports
                .borrow()
                .iter()
                .filter_map(|(port_id, state)| {
                    (!state.closed && !state.inbox.is_empty())
                        .then_some((*port_id, state.object_bits))
                })
                .collect()
        });
        let mut next: Option<MessageDispatch> = None;
        for (port_id, target_bits) in candidates {
            let handler_cb = object_event_handler(target_bits, "onmessage");
            next = MESSAGE_PORTS.with(|ports| {
                let mut ports = ports.borrow_mut();
                let state = ports.get_mut(&port_id)?;
                let has_event_target = state.started
                    && (!state.message_cbs.is_empty() || !state.message_event_cbs.is_empty());
                if state.closed || (!has_event_target && handler_cb.is_none()) {
                    return None;
                }
                state.inbox.pop_front().map(|msg| {
                    let raw_cbs = state.message_cbs.clone();
                    state.message_cbs.retain(|listener| !listener.once);
                    let event_cbs = state.message_event_cbs.clone();
                    state.message_event_cbs.retain(|listener| !listener.once);
                    MessageDispatch {
                        target_bits: state.object_bits,
                        raw_cbs,
                        event_cbs,
                        handler_cb,
                        msg,
                    }
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
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
                    .map(|listener| scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)))
                    .collect::<Vec<_>>();
                let handler_cb = dispatch
                    .handler_cb
                    .map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
                let value = deserialize_message(&dispatch.msg);
                let value = scope.root_nanbox_f64(value);
                for callback in raw_cbs {
                    call_callback1(
                        callback.get_nanbox_f64().to_bits(),
                        target.get_nanbox_f64().to_bits(),
                        value.get_nanbox_f64(),
                    );
                }
                if !event_cbs.is_empty() || handler_cb.is_some() {
                    let event = event_object(
                        "message",
                        target.get_nanbox_f64().to_bits(),
                        Some(value.get_nanbox_f64()),
                    );
                    let event = scope.root_nanbox_f64(event);
                    for callback in event_cbs {
                        call_callback1(
                            callback.get_nanbox_f64().to_bits(),
                            target.get_nanbox_f64().to_bits(),
                            event.get_nanbox_f64(),
                        );
                    }
                    if let Some(callback) = handler_cb {
                        call_callback1(
                            callback.get_nanbox_f64().to_bits(),
                            target.get_nanbox_f64().to_bits(),
                            event.get_nanbox_f64(),
                        );
                    }
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    struct BroadcastDispatch {
        target_bits: u64,
        event_cbs: Vec<EventListener>,
        handler_cb: Option<u64>,
        msg: SerializedMessage,
    }

    loop {
        let candidates: Vec<(u64, u64)> = BROADCAST_CHANNELS.with(|channels| {
            channels
                .borrow()
                .iter()
                .filter_map(|(channel_id, state)| {
                    (!state.closed && !state.inbox.is_empty())
                        .then_some((*channel_id, state.object_bits))
                })
                .collect()
        });
        let mut next: Option<BroadcastDispatch> = None;
        for (channel_id, target_bits) in candidates {
            let handler_cb = object_event_handler(target_bits, "onmessage");
            next = BROADCAST_CHANNELS.with(|channels| {
                let mut channels = channels.borrow_mut();
                let state = channels.get_mut(&channel_id)?;
                if state.closed || (state.message_event_cbs.is_empty() && handler_cb.is_none()) {
                    return None;
                }
                state.inbox.pop_front().map(|msg| {
                    let event_cbs = state.message_event_cbs.clone();
                    state.message_event_cbs.retain(|listener| !listener.once);
                    BroadcastDispatch {
                        target_bits: state.object_bits,
                        event_cbs,
                        handler_cb,
                        msg,
                    }
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let target = scope.root_nanbox_f64(f64::from_bits(dispatch.target_bits));
                let event_cbs = dispatch
                    .event_cbs
                    .into_iter()
                    .map(|listener| scope.root_nanbox_f64(f64::from_bits(listener.callback_bits)))
                    .collect::<Vec<_>>();
                let handler_cb = dispatch
                    .handler_cb
                    .map(|bits| scope.root_nanbox_f64(f64::from_bits(bits)));
                let value = deserialize_message(&dispatch.msg);
                let value = scope.root_nanbox_f64(value);
                let event = event_object(
                    "message",
                    target.get_nanbox_f64().to_bits(),
                    Some(value.get_nanbox_f64()),
                );
                let event = scope.root_nanbox_f64(event);
                if let Some(callback) = handler_cb {
                    call_callback1(
                        callback.get_nanbox_f64().to_bits(),
                        target.get_nanbox_f64().to_bits(),
                        event.get_nanbox_f64(),
                    );
                }
                for callback in event_cbs {
                    call_callback1(
                        callback.get_nanbox_f64().to_bits(),
                        target.get_nanbox_f64().to_bits(),
                        event.get_nanbox_f64(),
                    );
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    // Fire `close` callbacks once for newly-closed ports.
    struct CloseDispatch {
        target_bits: u64,
        raw_cbs: Vec<EventListener>,
        event_cbs: Vec<EventListener>,
    }

    let close_events: Vec<CloseDispatch> = MESSAGE_PORTS.with(|ports| {
        let mut events = Vec::new();
        for state in ports.borrow_mut().values_mut() {
            if state.close_pending {
                state.close_pending = false;
                let raw_cbs = state.close_cbs.clone();
                state.close_cbs.retain(|listener| !listener.once);
                let event_cbs = state.close_event_cbs.clone();
                state.close_event_cbs.retain(|listener| !listener.once);
                events.push(CloseDispatch {
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
    let pending_without_onmessage = MESSAGE_PORTS.with(|ports| {
        ports.borrow().values().any(|state| {
            let has_event_target = state.started
                && (!state.message_cbs.is_empty() || !state.message_event_cbs.is_empty());
            (!state.closed && !state.inbox.is_empty() && has_event_target) || state.close_pending
        })
    });
    if pending_without_onmessage {
        return 1;
    }

    let onmessage_targets: Vec<u64> = MESSAGE_PORTS.with(|ports| {
        ports
            .borrow()
            .values()
            .filter_map(|state| {
                (!state.closed && !state.inbox.is_empty()).then_some(state.object_bits)
            })
            .collect()
    });
    if onmessage_targets
        .into_iter()
        .any(|target_bits| object_event_handler(target_bits, "onmessage").is_some())
    {
        return 1;
    }

    let broadcast_pending = BROADCAST_CHANNELS.with(|channels| {
        channels.borrow().values().any(|state| {
            !state.closed && !state.inbox.is_empty() && !state.message_event_cbs.is_empty()
        })
    });
    if broadcast_pending {
        return 1;
    }

    let broadcast_onmessage_targets: Vec<u64> = BROADCAST_CHANNELS.with(|channels| {
        channels
            .borrow()
            .values()
            .filter_map(|state| {
                (!state.closed && !state.inbox.is_empty()).then_some(state.object_bits)
            })
            .collect()
    });
    if broadcast_onmessage_targets
        .into_iter()
        .any(|target_bits| object_event_handler(target_bits, "onmessage").is_some())
    {
        1
    } else {
        0
    }
}
