// ============================================================================
// #4728 — in-flight (async-handler) request tracking.
//
// A `(req, res) => { … }` handler that finishes the response on a *later*
// event-loop tick — an outbound `fetch()`, a `setTimeout`, any `await`
// chain that calls `res.end()` from a microtask/timer/tokio resolution —
// returns to `process_pending` before `res.end()` has run. Pre-#4728,
// `process_pending` then synthesized a default empty 200 and freed the
// per-request handles immediately, so the real `res.end(...)` later fired
// on a dropped handle (no-op) and the client saw an empty/closed reply.
//
// Fix: when the handler returns without ending the response, park the
// request here instead of synthesizing+freeing. The reaper runs each pump
// tick (the codegen-emitted main loop keeps ticking while the server is a
// live handle, draining timers / fetch resolutions / microtasks), and
// finalizes a parked request once `res.end()` has flushed the real
// response — or, as a safety net mirroring Node's `requestTimeout`,
// synthesizes the default response and frees the handles if the handler
// never responds within the grace window so a buggy handler can't pin a
// hyper connection (and its request handles) forever.
// ============================================================================

use std::sync::Mutex;
use std::time::{Duration, Instant};

use perry_ffi::get_handle;

use crate::request::close_incoming_message;
use crate::response::ServerResponse;
use crate::server::{synthesize_default_response_if_needed, HttpPendingRequest, HttpServer};

/// A request whose handler returned before finishing the response.
pub(crate) struct InFlightRequest {
    /// Owning server — lets `closeAllConnections()` drop parked requests
    /// whose connection it just destroyed (#4905).
    pub(crate) server_handle: i64,
    pub(crate) request_handle: i64,
    pub(crate) response_handle: i64,
    /// Mirrors `HttpPendingRequest::skip_default_response`: when true the
    /// response is driven elsewhere (e.g. an upgraded/stream path) so the
    /// reaper must not synthesize a default on timeout.
    skip_default_response: bool,
    /// Grace deadline. Past this, synthesize the default response (unless
    /// `skip_default_response`) and free the handles regardless.
    pub(crate) deadline: Instant,
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
    IN_FLIGHT.lock().map(|g| !g.is_empty()).unwrap_or(false)
}

/// Finalize parked requests whose handler has now called `res.end()` (the
/// common case — fetch/timer/await resolved on a later tick), or whose
/// grace deadline has elapsed (a handler that never responds). Called each
/// pump tick. #4728.
pub(crate) fn reap_in_flight_requests() {
    // (request_handle, response_handle, needs_synthesize, recycle_deadline)
    let mut to_finalize: Vec<(i64, i64, bool, Option<Instant>)> = Vec::new();
    let mut drain_listeners: Vec<Vec<i64>> = Vec::new();
    {
        let mut guard = match IN_FLIGHT.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        if guard.is_empty() {
            return;
        }
        let now = Instant::now();
        guard.retain(|e| {
            let ended = response_writable_ended(e.response_handle);
            if !ended {
                // Streaming backpressure cleared — fire `'drain'` (outside
                // the lock) so `res.on('drain')` producer loops resume.
                let ls = crate::response::take_drain_listeners_if_ready(e.response_handle);
                if !ls.is_empty() {
                    drain_listeners.push(ls);
                }
            }
            // #4905: the per-request oneshot receiver died with its
            // connection task (client disconnected / closeAllConnections)
            // — the response can never be flushed, so don't pin the event
            // loop for the rest of the grace window. A streaming response
            // whose body receiver dropped is the same edge.
            let peer_gone = get_handle::<ServerResponse>(e.response_handle)
                .and_then(|sr| sr.response_tx.as_ref())
                .map(|tx| tx.is_closed())
                .unwrap_or(false)
                || crate::response::stream_receiver_gone(e.response_handle);
            let expired = now >= e.deadline;
            if ended || expired || peer_gone {
                // Only synthesize when we're giving up on a handler
                // that never ended the response — not when it ended
                // it itself, never for skip-default paths, and never
                // when the peer is gone (nothing to deliver to).
                let needs_synth = !ended && !e.skip_default_response && !peer_gone;
                // If the response will NOT be ended after finalize (it wasn't
                // ended and we're not synthesizing it to ended — the peer-gone
                // and not-ended-skip-default paths), its `writable_ended` stays
                // unset, so a handler suspended on a slow `await` could resume
                // later and write through the bare id. Defer recycling that id
                // until the request's grace deadline so the late write lands on
                // an empty slot for the whole window, not a recycled response.
                let recycle_deadline = if ended || needs_synth {
                    None
                } else {
                    Some(e.deadline)
                };
                to_finalize.push((
                    e.request_handle,
                    e.response_handle,
                    needs_synth,
                    recycle_deadline,
                ));
                false
            } else {
                true
            }
        });
    }
    for ls in drain_listeners {
        crate::request::emit_no_arg_to_listeners(&ls);
    }
    // Finalize outside the lock — `synthesize_default_response_if_needed`
    // and `drop_handle` don't touch `IN_FLIGHT`, but keeping them off the
    // lock avoids any future re-entrancy surprise.
    for (req, res, needs_synth, recycle_deadline) in to_finalize {
        if needs_synth {
            synthesize_default_response_if_needed(res);
        }
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
    // Grace window mirrors Node's `requestTimeout` (default 300s; `0` =
    // disabled, so fall back to the default rather than parking forever).
    let grace_ms = get_handle::<HttpServer>(pending.server_handle)
        .map(|s| s.request_timeout)
        .filter(|t| *t > 0.0)
        .unwrap_or(300_000.0);
    let deadline = Instant::now() + Duration::from_millis(grace_ms as u64);
    if let Ok(mut guard) = IN_FLIGHT.lock() {
        guard.push(InFlightRequest {
            server_handle: pending.server_handle,
            request_handle: pending.request_handle,
            response_handle: pending.response_handle,
            skip_default_response: pending.skip_default_response,
            deadline,
        });
    } else {
        // Lock poisoned — fall back to the old immediate behavior so we
        // never leak the handles.
        if !pending.skip_default_response {
            synthesize_default_response_if_needed(pending.response_handle);
        }
        finalize_request_handles(pending.request_handle, pending.response_handle);
    }
}
