// Async responses remain owned by their server until res.end() or transport
// teardown. requestTimeout belongs to incomplete request decoding, never
// response age. The retirement grace below only protects a dropped legacy
// response id from a suspended handler's late write; it starts when that
// response is retired.

use std::time::{Duration, Instant};

use perry_ffi::{get_handle, TransientRootScope};

use super::{
    object_handle, owned_server_handles, with_base_server, with_base_server_mut,
    HttpPendingRequest, HttpServer,
};
use crate::server::request::close_incoming_message;
use crate::server::response::{is_legacy_response_handle, response_state};

/// A request whose handler returned before finishing the response. It lives
/// in its server's `in_flight`, whose scanner visits `response`.
pub(crate) struct InFlightRequest {
    pub(crate) request_handle: i64,
    /// The response object word.
    pub(crate) response: f64,
    /// Quarantine duration after a peer/force close with an unfinished
    /// response. It is not an active-response expiration deadline.
    retire_grace: Duration,
}

#[cfg(test)]
impl InFlightRequest {
    /// A parked exchange holding only its response word.
    pub(crate) fn parked_for_test(response: f64) -> Self {
        Self {
            request_handle: 0,
            response,
            retire_grace: Duration::ZERO,
        }
    }
}

impl InFlightRequest {
    pub(super) fn recycle_deadline(&self) -> Instant {
        Instant::now()
            .checked_add(self.retire_grace)
            .unwrap_or_else(Instant::now)
    }
}

/// True iff `res.end()` has flushed the response (or its state is already
/// gone). A missing state reads as "done" so a stray entry can't wedge the
/// reaper.
pub(crate) fn response_writable_ended(response_handle: i64) -> bool {
    response_state(response_handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true)
}

/// Free the per-request request + response handles. Mirrors the tail of
/// the synchronous-handler path in `process_pending`. The response is ended
/// (or about to be, via synthesize), so its id takes the one-tick fast path.
fn finalize_request_handles(request_handle: i64, response_handle: i64) {
    finalize_request_handles_deferred(request_handle, response_handle, None);
}

/// Free the per-request handles. A heap response has no id: closing its
/// payload is its whole teardown, and a late write finds a closed payload.
/// For a legacy response id, `recycle_deadline` is `Some` only when the
/// response is being finalized WITHOUT having ended (the reaper's
/// peer-disconnect / server-force-close paths) — its id is then held in the
/// deadline-gated quarantine through the request's grace window so a
/// long-suspended handler that resumes and writes hits an empty slot rather
/// than a recycled response (see `perry_ffi::drop_handle_until`). The request
/// id always takes the one-tick path — `IncomingMessage` has no late-write
/// surface, and it is closed here first.
pub(crate) fn finalize_request_handles_deferred(
    request_handle: i64,
    response_handle: i64,
    recycle_deadline: Option<Instant>,
) {
    close_incoming_message(request_handle);
    perry_ffi::drop_handle(request_handle);
    retire_response(response_handle, recycle_deadline);
}

/// Release a finished or abandoned response.
pub(crate) fn retire_response(response_handle: i64, recycle_deadline: Option<Instant>) {
    if !is_legacy_response_handle(response_handle) {
        crate::server::response_payload::close(response_handle);
        return;
    }
    match recycle_deadline {
        Some(deadline) => perry_ffi::drop_handle_until(response_handle, deadline),
        None => perry_ffi::drop_handle(response_handle),
    };
}

/// True iff any request is parked awaiting an async handler — keeps the
/// server's handle "active" so the main loop doesn't exit before the
/// pending response is flushed.
pub(crate) fn has_in_flight_requests() -> bool {
    super::any_owned_server(|s| !s.in_flight.is_empty())
}

/// Finalize parked requests whose handler has now called `res.end()` (the
/// common case — fetch/timer/await resolved on a later tick), or whose
/// peer has disconnected. An unfinished live response has no age cutoff.
pub(crate) fn reap_in_flight_requests() {
    let roots = TransientRootScope::enter();
    let mut to_finalize = Vec::new();
    let mut drain_listeners: Vec<Vec<i64>> = Vec::new();
    let mut heap_drains = Vec::new();
    for server in owned_server_handles() {
        let mut index = 0;
        loop {
            let Some((request, response, deadline)) = with_base_server(server, |s| {
                s.in_flight
                    .get(index)
                    .map(|e| (e.request_handle, e.response, e.recycle_deadline()))
            })
            .flatten() else {
                break;
            };
            let response = roots.root_nanbox(response);
            let handle = object_handle(response.get());
            let ended = response_writable_ended(handle);
            if !ended && crate::server::response::drain_ready(handle) {
                // Streaming backpressure cleared — fire `'drain'` (after the
                // walk) so `res.on('drain')` producer loops resume.
                if is_legacy_response_handle(handle) {
                    let ls = crate::server::response::response_event_listeners(handle, "drain");
                    if !ls.is_empty() {
                        drain_listeners.push(ls);
                    }
                } else {
                    heap_drains.push(roots.root_nanbox(response.get()));
                }
            }
            // #4905: the response's connection is gone (client
            // disconnected / closeAllConnections) — the response can never
            // be flushed, so don't pin the event loop for the rest of the
            // grace window.
            let peer_gone =
                crate::server::response::stream_receiver_gone(object_handle(response.get()));
            if ended || peer_gone {
                let _ = with_base_server_mut(server, |s| s.in_flight.remove(index));
                to_finalize.push((request, response, (!ended).then_some(deadline)));
            } else {
                index += 1;
            }
        }
    }
    for ls in drain_listeners {
        crate::server::request::emit_no_arg_to_listeners(&ls);
    }
    for response in heap_drains {
        crate::server::response_payload::emit(object_handle(response.get()), "drain");
    }
    // Retire after the walk; teardown can deliver close callbacks.
    for (req, res, recycle_deadline) in to_finalize {
        finalize_request_handles_deferred(req, object_handle(res.get()), recycle_deadline);
    }
}

/// Finalize a just-dispatched request, or park it on its server for the
/// reaper if its handler returned before finishing the response (an async
/// handler that will call `res.end()` on a later tick). Shared by the HTTP/1
/// and HTTPS dispatch paths. #4728. `pending.response` must be the caller's
/// fresh (rooted) word.
pub(crate) fn finalize_or_park_request(pending: &HttpPendingRequest) {
    let response = pending.response_handle();
    if response_writable_ended(response) {
        finalize_request_handles(pending.request_handle, response);
        return;
    }
    // Keep the existing late-write retirement grace. Disabled receiving
    // timeouts still use a finite quarantine after explicit teardown.
    let grace_ms = get_handle::<HttpServer>(pending.server_handle)
        .map(|s| s.request_timeout)
        .filter(|t| *t > 0.0)
        .unwrap_or(300_000.0);
    let retire_grace = Duration::from_millis(grace_ms as u64);
    let entry = InFlightRequest {
        request_handle: pending.request_handle,
        response: pending.response,
        retire_grace,
    };
    if with_base_server_mut(pending.server_handle, move |s| s.in_flight.push(entry)).is_none() {
        // The server is gone: destroy the actual response connection; it
        // must not invent a successful response for an unfinished handler.
        let connection = response_state(response)
            .and_then(|state| state.turnloop.map(|(connection, _)| connection));
        if let Some(connection) = connection {
            crate::server::turnloop_route::destroy(&connection.root(crate::server::response::response_socket(response)));
        }
        let deadline = Instant::now().checked_add(retire_grace);
        finalize_request_handles_deferred(pending.request_handle, response, deadline);
    }
}
