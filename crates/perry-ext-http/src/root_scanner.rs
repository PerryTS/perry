use super::*;
use perry_ffi::GcRootVisitor;

/// Walk every client request, incoming message, and agent closure root.
/// Raw closure pointers are exposed as mutable slots so copying GC can
/// rewrite them after relocation.
pub(super) fn scan_http_roots(visitor: &mut GcRootVisitor<'_>) {
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
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
        for cbs in req.listeners.values_mut() {
            for cb in cbs {
                visitor.visit_i64_slot(cb);
            }
        }
        for listeners in req.once_listeners.values_mut() {
            for listener in listeners {
                visitor.visit_i64_slot(&mut listener.callback);
                visitor.visit_i64_slot(&mut listener.raw_wrapper);
            }
        }
        if req.tls.check_server_identity_callback != 0 {
            visitor.visit_i64_slot(&mut req.tls.check_server_identity_callback);
        }
    });

    iter_handles_of_mut::<IncomingMessageHandle, _>(|msg| {
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
    client_request_surface::scan_roots(visitor);
}
