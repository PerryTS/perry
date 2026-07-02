//! Event-loop pump for `MessageChannel` / `BroadcastChannel` inboxes.
//!
//! Split out of `worker_threads.rs` to keep that file under the 2000-line lint
//! cap. These two `#[no_mangle]` entry points are called from the async bridge
//! pump (`common::async_bridge`) and are re-exported from the parent module so
//! `crate::worker_threads::js_worker_threads_channels_*` keeps resolving.

use super::{
    call_callback0, call_callback1, deserialize_message, event_object, object_event_handler,
    BROADCAST_CHANNELS, MESSAGE_PORTS,
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
        raw_cb: Option<u64>,
        event_cbs: Vec<u64>,
        handler_cb: Option<u64>,
        msg: String,
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
                    && (state.message_cb.is_some() || !state.message_event_cbs.is_empty());
                if state.closed || (!has_event_target && handler_cb.is_none()) {
                    return None;
                }
                state.inbox.pop_front().map(|msg| MessageDispatch {
                    target_bits: state.object_bits,
                    raw_cb: state.message_cb,
                    event_cbs: state.message_event_cbs.clone(),
                    handler_cb,
                    msg,
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                let value = deserialize_message(&dispatch.msg);
                if let Some(cb_bits) = dispatch.raw_cb {
                    call_callback1(cb_bits, dispatch.target_bits, value);
                }
                if !dispatch.event_cbs.is_empty() || dispatch.handler_cb.is_some() {
                    let event = event_object("message", dispatch.target_bits, Some(value));
                    for cb_bits in dispatch.event_cbs {
                        call_callback1(cb_bits, dispatch.target_bits, event);
                    }
                    if let Some(cb_bits) = dispatch.handler_cb {
                        call_callback1(cb_bits, dispatch.target_bits, event);
                    }
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    struct BroadcastDispatch {
        target_bits: u64,
        event_cbs: Vec<u64>,
        handler_cb: Option<u64>,
        msg: String,
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
                state.inbox.pop_front().map(|msg| BroadcastDispatch {
                    target_bits: state.object_bits,
                    event_cbs: state.message_event_cbs.clone(),
                    handler_cb,
                    msg,
                })
            });
            if next.is_some() {
                break;
            }
        }
        match next {
            Some(dispatch) => {
                let value = deserialize_message(&dispatch.msg);
                let event = event_object("message", dispatch.target_bits, Some(value));
                if let Some(cb_bits) = dispatch.handler_cb {
                    call_callback1(cb_bits, dispatch.target_bits, event);
                }
                for cb_bits in dispatch.event_cbs {
                    call_callback1(cb_bits, dispatch.target_bits, event);
                }
                dispatched += 1;
            }
            None => break,
        }
    }

    // Fire `close` callbacks once for newly-closed ports.
    struct CloseDispatch {
        target_bits: u64,
        raw_cb: Option<u64>,
        event_cbs: Vec<u64>,
    }

    let close_events: Vec<CloseDispatch> = MESSAGE_PORTS.with(|ports| {
        let mut events = Vec::new();
        for state in ports.borrow_mut().values_mut() {
            if state.close_pending {
                state.close_pending = false;
                events.push(CloseDispatch {
                    target_bits: state.object_bits,
                    raw_cb: state.close_cb,
                    event_cbs: state.close_event_cbs.clone(),
                });
            }
        }
        events
    });
    for event in close_events {
        if let Some(cb_bits) = event.raw_cb {
            call_callback0(cb_bits, event.target_bits);
        }
        if !event.event_cbs.is_empty() {
            let close_event = event_object("close", event.target_bits, None);
            for cb_bits in event.event_cbs {
                call_callback1(cb_bits, event.target_bits, close_event);
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
                && (state.message_cb.is_some() || !state.message_event_cbs.is_empty());
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
