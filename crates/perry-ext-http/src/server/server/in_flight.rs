// Async responses remain owned here until res.end() or transport teardown.
// requestTimeout belongs to incomplete request decoding, never response age.
// A finished pair is parked, not dropped (`crate::server::payloads`): a
// suspended handler that resumes and writes reaches its own finished response,
// because an id that JS still names is never reissued.

use std::sync::Mutex;

use perry_ffi::get_handle;

use crate::server::payloads::park_server_exchange;
use crate::server::response::ServerResponse;
use crate::server::server::HttpPendingRequest;

/// A request whose handler returned before finishing the response.
pub(crate) struct InFlightRequest {
    /// Owning server — lets `closeAllConnections()` drop parked requests
    /// whose connection it just destroyed (#4905).
    pub(crate) server_handle: i64,
    pub(crate) request_handle: i64,
    pub(crate) response_handle: i64,
    /// The agent whose pump parked it — the server's owner (#11433). Only that
    /// agent reaps it, fires its `'drain'` listeners, or is kept alive by it.
    owner_agent: u64,
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

/// The exchange is finished (its response ended, or its connection is gone so
/// it never can): close the request and hand both ids to the GC
/// (`park_server_exchange`). A kept `req` / `res` keeps answering like Node.
pub(crate) fn finalize_request_handles(request_handle: i64, response_handle: i64) {
    park_server_exchange(request_handle, response_handle);
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
    let mut to_finalize: Vec<(i64, i64)> = Vec::new();
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
                to_finalize.push((e.request_handle, e.response_handle));
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
    for (req, res) in to_finalize {
        finalize_request_handles(req, res);
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
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        guard.push(InFlightRequest {
            server_handle: pending.server_handle,
            request_handle: pending.request_handle,
            response_handle: pending.response_handle,
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
        finalize_request_handles(pending.request_handle, pending.response_handle);
    }
}
