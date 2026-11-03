//! A finished client exchange's `ClientRequest` / `IncomingMessage` payloads
//! belong to the JS values that name them (perry-feather#111).
//!
//! Until its exchange ends a request is owned by the native side: the
//! transport, the agent and the event queue hold its id, and its payload's
//! edges are roots. Once `'close'` fired, [`park_client_exchange`] parks both
//! ids with [`CLIENT_PAYLOADS`]. From then on a full trace marks a payload's
//! edges only through a traced word naming its id
//! (`ParkedPayloadOwner::edges`), and the trace that finds no such word drops
//! the payload and gives the id back. User code keeps reading a finished
//! response (`res.statusCode`, `res.req`, a late `'data'` reader) for as long
//! as it holds it, as in Node.
//!
//! Every native holder of a bare client id names it to the trace: the pending
//! event queue, the in-flight set and the agent queues through
//! [`crate::root_scanner`], native closures by capturing a handle word
//! ([`handle_capture`]). A holder on another thread (a worker's exchange runs
//! on the loop thread) holds an operation lease instead, so the id is not
//! reissued while it can still act on it.
use super::*;
use perry_ffi::handle_lifecycle::ParkedPayloadOwner;

/// The ffi payload map as the owner of parked client ids.
pub(crate) struct ClientPayloads;

pub(crate) static CLIENT_PAYLOADS: ClientPayloads = ClientPayloads;

impl ParkedPayloadOwner for ClientPayloads {
    fn release(&self, identity: perry_ffi::NativeRegistrationIdentity) {
        // A payload holds strings, vectors and closure addresses; dropping
        // it runs no JS and allocates nothing on the GC heap.
        drop(perry_ffi::release_reclaimed_payload(identity));
    }

    fn edges(&self, id: Handle, out: &mut Vec<u64>) {
        out.extend(perry_ffi::native_handle_props(id));
        let request =
            perry_ffi::with_handle::<ClientRequestHandle, _, _>(id, |req| request_edges(req, out));
        if request.is_none() {
            perry_ffi::with_handle::<IncomingMessageHandle, _, _>(id, |msg| {
                incoming_edges(msg, out)
            });
        }
    }
}

/// A raw closure or object address as a JSValue word (`0` stays absent).
fn address_word(address: i64) -> u64 {
    if address > 0 {
        POINTER_TAG | (address as u64 & PTR_MASK)
    } else {
        0
    }
}

/// The word naming a client payload's id, as the payload is handed to JS.
pub(crate) fn handle_word(id: Handle) -> u64 {
    POINTER_TAG | (id as u64 & PTR_MASK)
}

/// A native closure capture that names `id` to the trace (a plain Number
/// would not, so the id could be reissued while a timer or listener still
/// fires for it).
pub(crate) fn handle_capture(id: Handle) -> f64 {
    f64::from_bits(handle_word(id))
}

/// Read back a capture written by [`handle_capture`].
///
/// # Safety
///
/// `closure` must be a live closure with a capture at `index`.
pub(crate) unsafe fn capture_handle_at(closure: *const RawClosureHeader, index: u32) -> Handle {
    (perry_ffi::closure_capture_f64(closure, index).to_bits() & PTR_MASK) as Handle
}

/// The same slots `scan_http_roots` visits for a native-owned request, plus
/// the response it produced.
fn request_edges(req: &ClientRequestHandle, out: &mut Vec<u64>) {
    out.extend(
        [
            req.socket_handle,
            req.request_create_connection,
            req.response_callback,
            req.response_raw_wrapper,
            req.end_callback,
            req.tls.check_server_identity_callback,
        ]
        .map(address_word),
    );
    out.extend([req.abort_signal_bits, req.abort_listener_bits]);
    out.extend(
        req.pending_write_callbacks
            .iter()
            .map(|&cb| address_word(cb)),
    );
    for listener in req.listeners.values().flatten() {
        out.push(address_word(listener.callback));
        if listener.raw_wrapper != listener.callback {
            out.push(address_word(listener.raw_wrapper));
        }
    }
    if req.incoming_handle != 0 {
        out.push(handle_word(req.incoming_handle));
    }
}

/// The same slots `scan_http_roots` visits for a native-owned response, plus
/// `res.req`.
fn incoming_edges(msg: &IncomingMessageHandle, out: &mut Vec<u64>) {
    out.push(address_word(msg.socket_handle));
    out.extend(msg.listeners.values().flatten().map(|&cb| address_word(cb)));
    out.extend(msg.pipes.iter().copied());
    if msg.read.pending_error != 0 {
        out.push(msg.read.pending_error);
    }
    for &(wrapper, original) in &msg.read.once_targets {
        out.extend([address_word(wrapper), address_word(original)]);
    }
    if msg.request_handle != 0 {
        out.push(handle_word(msg.request_handle));
    }
}

/// The exchange of `request` is finished (its `'close'` fired): park the
/// request and its response as reclaimable. Called after the terminal
/// handler returned, from the event drain and from `req.destroy()`, so no
/// native frame still uses the ids unnamed. Idempotent.
pub(crate) fn park_client_exchange(request: Handle) {
    if perry_ffi::handle_lifecycle::is_parked(request) {
        return;
    }
    let Some(incoming) = perry_ffi::with_handle::<ClientRequestHandle, _, _>(request, |req| {
        req.close_emitted.then_some(req.incoming_handle)
    })
    .flatten() else {
        return;
    };
    perry_ffi::park_reclaimable_ffi(request, &CLIENT_PAYLOADS);
    if incoming != 0 {
        perry_ffi::park_reclaimable_ffi(incoming, &CLIENT_PAYLOADS);
    }
}

/// A lease that keeps `request`'s id from being reissued while a holder on
/// another thread (a posted loop job, the exchange inside the parser) may
/// still act on it. `None` once the request is no longer registered.
pub(crate) fn request_lease(request: Handle) -> Option<perry_ffi::NativeRegistrationLease> {
    perry_ffi::handle_registration(request).and_then(|identity| {
        perry_ffi::acquire_handle_registration(identity, perry_ffi::NativeLeaseKind::Operation)
    })
}

#[cfg(test)]
#[path = "client_payloads_tests.rs"]
mod tests;
