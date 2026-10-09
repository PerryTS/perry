use super::*;
use perry_ffi::GcRootVisitor;

/// Walk every client request, incoming message, and agent closure root.
/// Raw closure pointers are exposed as mutable slots so copying GC can
/// rewrite them after relocation.
pub(super) fn scan_http_roots(visitor: &mut GcRootVisitor<'_>) {
    let agent = perry_ffi::agent_post::current_agent();
    if let Ok(mut queue) = HTTP_PENDING_EVENTS.lock() {
        for (_, event) in queue.iter_mut().filter(|(owner, _)| *owner == agent) {
            if let PendingHttpEvent::Upgrade { socket_handle, .. } = event {
                visitor.visit_i64_slot(socket_handle);
            }
        }
    }
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
        if req.owner_agent != agent {
            return;
        }
        visitor.visit_i64_slot(&mut req.socket_handle);
        visitor.visit_i64_slot(&mut req.request_create_connection);
        visitor.visit_i64_slot(&mut req.response_callback);
        visitor.visit_i64_slot(&mut req.response_raw_wrapper);
        visitor.visit_i64_slot(&mut req.end_callback);
        if req.abort_signal_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut req.abort_signal_bits);
        }
        if req.abort_listener_bits != 0 {
            visitor.visit_nanbox_u64_slot(&mut req.abort_listener_bits);
        }
        for cb in &mut req.pending_write_callbacks {
            visitor.visit_i64_slot(cb);
        }
        for listeners in req.listeners.values_mut() {
            for listener in listeners {
                let shared_wrapper = listener.raw_wrapper == listener.callback;
                visitor.visit_i64_slot(&mut listener.callback);
                if shared_wrapper {
                    listener.raw_wrapper = listener.callback;
                } else {
                    visitor.visit_i64_slot(&mut listener.raw_wrapper);
                }
            }
        }
        if req.tls.check_server_identity_callback != 0 {
            visitor.visit_i64_slot(&mut req.tls.check_server_identity_callback);
        }
    });

    iter_handles_of_mut::<IncomingMessageHandle, _>(|msg| {
        if msg.owner_agent != agent {
            return;
        }
        visitor.visit_i64_slot(&mut msg.socket_handle);
        for cbs in msg.listeners.values_mut() {
            for cb in cbs {
                visitor.visit_i64_slot(cb);
            }
        }
        // `.pipe(dest)` destinations stay live until their body streams.
        for dest in &mut msg.pipes {
            visitor.visit_nanbox_u64_slot(dest);
        }
    });

    agent::scan_agent_roots(visitor);
}

/// Agent teardown drops only that heap's app ownership edges. The runtime
/// retires driver capabilities separately; this hook never runs JS/driver.
pub(super) extern "C" fn retire_client_roots(agent: u64) {
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
        if req.owner_agent != agent {
            return;
        }
        req.socket_handle = 0;
        req.socket_snapshot = None;
        req.response_callback = 0;
        req.response_raw_wrapper = 0;
        req.end_callback = 0;
        req.request_create_connection = 0;
        req.abort_signal_bits = 0;
        req.abort_listener_bits = 0;
        req.pending_write_callbacks.clear();
        req.listeners.clear();
        req.tls.check_server_identity_callback = 0;
        req.completed = true;
    });
    iter_handles_of_mut::<IncomingMessageHandle, _>(|msg| {
        if msg.owner_agent != agent {
            return;
        }
        msg.socket_handle = 0;
        msg.listeners.clear();
        msg.pipes.clear();
    });
    if let Ok(mut queue) = HTTP_PENDING_EVENTS.lock() {
        queue.retain(|(owner, _)| *owner != agent);
    }
    if let Ok(mut inflight) = CLIENT_REQUESTS_INFLIGHT.lock() {
        inflight.retain(|(owner, _)| *owner != agent);
    }
    agent::retire_agent_roots(agent);
}
