use super::*;
use perry_ffi::GcRootVisitor;

/// Walk every client request, incoming message, and agent closure root.
/// Raw closure pointers are exposed as mutable slots so copying GC can
/// rewrite them after relocation.
///
/// Only native-owned (in-flight) payloads are registered: a finished
/// exchange's records are parked (`client_payloads::CLIENT_PAYLOADS`) and
/// visited by perry-ffi's parked-record scanner while young, otherwise
/// through a word naming the id, so a payload nothing names is collected even
/// though its listeners capture it. Every native holder of a
/// bare client id names it here instead: queued events, the in-flight set and
/// the agent queues. Handle words are visited after the registry walk,
/// because marking one reads its payload from the same registry.
pub(super) fn scan_http_roots(visitor: &mut GcRootVisitor<'_>) {
    let agent = perry_ffi::agent_post::current_agent();
    let full_mark = visitor.is_full_mark();
    let mut named = Vec::new();
    if let Ok(mut queue) = HTTP_PENDING_EVENTS.lock() {
        for (_, event) in queue.iter_mut().filter(|(owner, _)| *owner == agent) {
            if let PendingHttpEvent::Upgrade { socket_handle, .. } = event {
                visitor.visit_i64_slot(socket_handle);
            }
            if full_mark {
                named.push(client_payloads::handle_word(pending_request_handle(event)));
                if let PendingHttpEvent::IncomingDrain { incoming, .. } = event {
                    named.push(client_payloads::handle_word(*incoming));
                }
            }
        }
    }
    if full_mark {
        if let Ok(inflight) = CLIENT_REQUESTS_INFLIGHT.lock() {
            named.extend(
                inflight
                    .iter()
                    .filter(|(owner, _)| *owner == agent)
                    .map(|&(_, request)| client_payloads::handle_word(request)),
            );
        }
    }
    iter_handles_of_mut::<ClientRequestHandle, _>(|req| {
        scan_client_request_roots(req, visitor, &mut named)
    });
    iter_handles_of_mut::<IncomingMessageHandle, _>(|msg| {
        scan_client_incoming_roots(msg, visitor, &mut named)
    });
    agent::scan_agent_roots(visitor, &mut named);
    if full_mark {
        for mut word in named {
            visitor.visit_nanbox_u64_slot(&mut word);
        }
    }
}

/// A client request's GC edges for the current mutator, in flight or parked.
/// A full mark pushes handle words and NaN-boxed values onto `named` (marking
/// one may read a payload of the registry the caller is walking).
pub(crate) fn scan_client_request_roots(
    req: &mut ClientRequestHandle,
    visitor: &mut GcRootVisitor<'_>,
    named: &mut Vec<u64>,
) {
    if req.owner_agent != perry_ffi::agent_post::current_agent() {
        return;
    }
    let full_mark = visitor.is_full_mark();
    visitor.visit_i64_slot(&mut req.socket_handle);
    visitor.visit_i64_slot(&mut req.request_create_connection);
    visitor.visit_i64_slot(&mut req.response_callback);
    visitor.visit_i64_slot(&mut req.response_raw_wrapper);
    visitor.visit_i64_slot(&mut req.end_callback);
    visit_value(visitor, full_mark, &mut req.abort_signal_bits, named);
    visit_value(visitor, full_mark, &mut req.abort_listener_bits, named);
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
    if full_mark && req.incoming_handle != 0 {
        named.push(client_payloads::handle_word(req.incoming_handle));
    }
}

/// A client response's GC edges for the current mutator, in flight or parked
/// (`named` as for [`scan_client_request_roots`]).
pub(crate) fn scan_client_incoming_roots(
    msg: &mut IncomingMessageHandle,
    visitor: &mut GcRootVisitor<'_>,
    named: &mut Vec<u64>,
) {
    if msg.owner_agent != perry_ffi::agent_post::current_agent() {
        return;
    }
    let full_mark = visitor.is_full_mark();
    visitor.visit_i64_slot(&mut msg.socket_handle);
    for cbs in msg.listeners.values_mut() {
        for cb in cbs {
            visitor.visit_i64_slot(cb);
        }
    }
    // `.pipe(dest)` destinations stay live until their body streams.
    for dest in &mut msg.pipes {
        visit_value(visitor, full_mark, dest, named);
    }
    visit_value(visitor, full_mark, &mut msg.read.pending_error, named);
    for (wrapper, original) in &mut msg.read.once_targets {
        visitor.visit_i64_slot(wrapper);
        visitor.visit_i64_slot(original);
    }
    if full_mark && msg.request_handle != 0 {
        named.push(client_payloads::handle_word(msg.request_handle));
    }
}

/// A NaN-boxed payload slot. In a full mark it may be a handle word whose
/// marking reads a payload of this registry (a piped client request), so it is
/// visited after the walk; a mark never rewrites a slot, so a copy is the same
/// visit. Minors and relocation scans visit it in place.
fn visit_value(
    visitor: &mut GcRootVisitor<'_>,
    full_mark: bool,
    slot: &mut u64,
    named: &mut Vec<u64>,
) {
    if *slot == 0 {
        return;
    }
    if full_mark {
        named.push(*slot);
    } else {
        visitor.visit_nanbox_u64_slot(slot);
    }
}

/// Agent teardown drops only that heap's app ownership edges. The runtime
/// retires driver capabilities separately; this hook never runs JS/driver.
pub(super) extern "C" fn retire_client_roots(agent: u64) {
    // Finished exchanges this agent parked hold the same edges.
    let clear_request = |req: &mut ClientRequestHandle| {
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
    };
    iter_handles_of_mut::<ClientRequestHandle, _>(clear_request);
    perry_ffi::handle_lifecycle::iter_parked_payloads_of_mut::<ClientRequestHandle, _>(
        clear_request,
    );
    let clear_incoming = |msg: &mut IncomingMessageHandle| {
        if msg.owner_agent != agent {
            return;
        }
        msg.socket_handle = 0;
        msg.listeners.clear();
        msg.pipes.clear();
        msg.read.once_targets.clear();
        msg.read.pending_error = 0;
    };
    iter_handles_of_mut::<IncomingMessageHandle, _>(clear_incoming);
    perry_ffi::handle_lifecycle::iter_parked_payloads_of_mut::<IncomingMessageHandle, _>(
        clear_incoming,
    );
    if let Ok(mut queue) = HTTP_PENDING_EVENTS.lock() {
        queue.retain(|(owner, _)| *owner != agent);
    }
    if let Ok(mut inflight) = CLIENT_REQUESTS_INFLIGHT.lock() {
        inflight.retain(|(owner, _)| *owner != agent);
    }
    agent::retire_agent_roots(agent);
}
