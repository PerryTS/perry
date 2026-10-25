// Async responses remain owned here until res.end() or transport teardown.
// requestTimeout belongs to incomplete request decoding, never response age.
// The retirement grace below only protects a dropped response handle from a
// suspended handler's late write; it starts when that response is retired.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use perry_ffi::get_handle;

use crate::server::request::close_incoming_message;
use crate::server::response::ServerResponse;
use crate::server::server::{HttpPendingRequest, HttpServer};

/// A request whose handler returned before finishing the response.
pub(crate) struct InFlightRequest {
    /// Owning server — lets `closeAllConnections()` drop parked requests
    /// whose connection it just destroyed (#4905).
    pub(crate) server_handle: i64,
    pub(crate) request_handle: i64,
    pub(crate) response_handle: i64,
    /// Quarantine duration after a peer/force close with an unfinished
    /// response. It is not an active-response expiration deadline.
    retire_grace: Duration,
    /// The agent whose pump parked it — the server's owner (#11433). Only that
    /// agent reaps it, fires its `'drain'` listeners, or is kept alive by it.
    owner_agent: u64,
}

impl InFlightRequest {
    pub(super) fn recycle_deadline(&self) -> Instant {
        Instant::now()
            .checked_add(self.retire_grace)
            .unwrap_or_else(Instant::now)
    }
}

pub(crate) static IN_FLIGHT: Mutex<Vec<InFlightRequest>> = Mutex::new(Vec::new());

/// True iff `res.end()` has flushed the response (or the handle is already
/// gone). A missing handle reads as "done" so a stray entry can't wedge
/// the reaper.
pub(crate) fn response_writable_ended(response_handle: i64) -> bool {
    get_handle::<ServerResponse>(response_handle)
        .map(|sr| sr.writable_ended)
        .unwrap_or(true)
}

/// Free the per-request request + response handles. Mirrors the tail of
/// the synchronous-handler path in `process_pending`. The response is ended
/// (or about to be, via synthesize), so its id takes the one-tick fast path.
fn finalize_request_handles(request_handle: i64, response_handle: i64) {
    finalize_request_handles_deferred(request_handle, response_handle, None);
}

/// Free the per-request handles. `recycle_deadline` is `Some` only when the
/// response is being finalized WITHOUT having ended (the reaper's
/// peer-disconnect / server-force-close paths) — its id is then held in the
/// deadline-gated quarantine through the request's grace window so a
/// long-suspended handler that resumes and writes hits an empty slot rather
/// than a recycled response (see `perry_ffi::drop_handle_until`). `None` is the
/// ended / synchronous case. The request id always takes the one-tick path —
/// `IncomingMessage` has no late-write surface, and it is closed here first.
pub(crate) fn finalize_request_handles_deferred(
    request_handle: i64,
    response_handle: i64,
    recycle_deadline: Option<Instant>,
) {
    close_incoming_message(request_handle);
    perry_ffi::drop_handle(request_handle);
    match recycle_deadline {
        Some(deadline) => perry_ffi::drop_handle_until(response_handle, deadline),
        None => perry_ffi::drop_handle(response_handle),
    };
}

/// True iff any request is parked awaiting an async handler — keeps the
/// server's handle "active" so the main loop doesn't exit before the
/// pending response is flushed.
pub(crate) fn has_in_flight_requests() -> bool {
    let agent = perry_ffi::agent_post::current_agent();
    IN_FLIGHT
        .lock()
        .map(|g| g.iter().any(|e| e.owner_agent == agent))
        .unwrap_or(false)
}

/// Finalize parked requests whose handler has now called `res.end()` (the
/// common case — fetch/timer/await resolved on a later tick), or whose
/// peer has disconnected. An unfinished live response has no age cutoff.
pub(crate) fn reap_in_flight_requests() {
    let mut to_finalize: Vec<(i64, i64, Option<Instant>)> = Vec::new();
    let mut drain_listeners: Vec<Vec<i64>> = Vec::new();
    {
        let mut guard = match IN_FLIGHT.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_empty() {
            return;
        }
        let agent = perry_ffi::agent_post::current_agent();
        guard.retain(|e| {
            if e.owner_agent != agent {
                // Another agent's request: its handles and listeners live in
                // that agent's heap (#11433).
                return true;
            }
            let ended = response_writable_ended(e.response_handle);
            if !ended {
                // Streaming backpressure cleared — fire `'drain'` (outside
                // the lock) so `res.on('drain')` producer loops resume.
                let ls = crate::server::response::take_drain_listeners_if_ready(e.response_handle);
                if !ls.is_empty() {
                    drain_listeners.push(ls);
                }
            }
            // #4905: the response's connection is gone (client
            // disconnected / closeAllConnections) — the response can never
            // be flushed, so don't pin the event loop for the rest of the
            // grace window.
            let peer_gone = crate::server::response::stream_receiver_gone(e.response_handle);
            if ended || peer_gone {
                let recycle_deadline = (!ended).then(|| e.recycle_deadline());
                to_finalize.push((e.request_handle, e.response_handle, recycle_deadline));
                false
            } else {
                true
            }
        });
    }
    for ls in drain_listeners {
        crate::server::request::emit_no_arg_to_listeners(&ls);
    }
    // Retire outside the queue borrow; teardown can deliver close callbacks.
    for (req, res, recycle_deadline) in to_finalize {
        finalize_request_handles_deferred(req, res, recycle_deadline);
    }
}

/// Finalize a just-dispatched request, or park it for the reaper if its
/// handler returned before finishing the response (an async handler that
/// will call `res.end()` on a later tick). Shared by the HTTP/1 and HTTPS
/// dispatch paths. #4728.
pub(crate) fn finalize_or_park_request(pending: &HttpPendingRequest) {
    if response_writable_ended(pending.response_handle) {
        finalize_request_handles(pending.request_handle, pending.response_handle);
        return;
    }
    // Keep the existing late-write retirement grace. Disabled receiving
    // timeouts still use a finite quarantine after explicit teardown.
    let grace_ms = get_handle::<HttpServer>(pending.server_handle)
        .map(|s| s.request_timeout)
        .filter(|t| *t > 0.0)
        .unwrap_or(300_000.0);
    let retire_grace = Duration::from_millis(grace_ms as u64);
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        guard.push(InFlightRequest {
            server_handle: pending.server_handle,
            request_handle: pending.request_handle,
            response_handle: pending.response_handle,
            retire_grace,
            owner_agent: perry_ffi::agent_post::current_agent(),
        });
    } else {
        // An internal queue failure destroys the actual response connection;
        // it must not invent a successful response for an unfinished handler.
        let connection = get_handle::<ServerResponse>(pending.response_handle)
            .and_then(|response| response.turnloop.map(|(connection, _)| connection));
        if let Some(connection) = connection {
            crate::server::turnloop_route::destroy(&connection.root());
        }
        let deadline = Instant::now().checked_add(retire_grace);
        finalize_request_handles_deferred(
            pending.request_handle,
            pending.response_handle,
            deadline,
        );
    }
}
