//! A finished server exchange's `IncomingMessage` / `ServerResponse` payloads
//! belong to the JS values that name them, as the client's do
//! (`crate::client_payloads`, perry-feather#111).
//!
//! When the response ended (or can never be flushed) the server finalizes the
//! pair: it closes the request and parks both ids with [`SERVER_PAYLOADS`].
//! A handler or a later callback that kept `req` / `res` reads its url,
//! headers and status like Node; a write after the connection went away hits
//! the real, finished response. A full trace marks a parked payload's edges
//! only through a traced word naming its id, and the trace that finds none
//! drops the payload and gives the id back. Until then the id cannot be
//! reissued, so no late caller reaches a recycled pair.
use perry_ffi::handle_lifecycle::ParkedPayloadOwner;
use perry_ffi::Handle;

use crate::client_payloads::{address_word, handle_word};
use crate::server::request::IncomingMessage;
use crate::server::response::ServerResponse;

/// The ffi payload map as the owner of parked server request/response ids.
pub(crate) struct ServerPayloads;

pub(crate) static SERVER_PAYLOADS: ServerPayloads = ServerPayloads;

impl ParkedPayloadOwner for ServerPayloads {
    fn release(&self, identity: perry_ffi::NativeRegistrationIdentity) {
        // A payload holds strings, vectors, closure addresses and a
        // connection snapshot; dropping it runs no JS and allocates nothing
        // on the GC heap.
        drop(perry_ffi::release_reclaimed_payload(identity));
    }

    fn edges(&self, id: Handle, out: &mut Vec<u64>) {
        out.extend(perry_ffi::native_handle_props(id));
        let agent = perry_ffi::agent_post::current_agent();
        let request = perry_ffi::with_handle::<IncomingMessage, _, _>(id, |im| {
            incoming_edges(im, agent, out)
        });
        if request.is_none() {
            perry_ffi::with_handle::<ServerResponse, _, _>(id, |sr| response_edges(sr, agent, out));
        }
    }
}

fn listener_words(listeners: &std::collections::HashMap<String, Vec<i64>>, out: &mut Vec<u64>) {
    out.extend(listeners.values().flatten().map(|&cb| address_word(cb)));
}

/// The slots `scan_http_server_roots` visits for a request.
fn incoming_edges(im: &IncomingMessage, agent: u64, out: &mut Vec<u64>) {
    listener_words(&im.listeners, out);
    out.extend([im.signal_controller.to_bits(), im.signal.to_bits()]);
    if im.owner_agent == agent {
        out.push(im.socket_value.to_bits());
    }
}

/// The slots `scan_http_server_roots` visits for a response, plus `res.req`.
fn response_edges(sr: &ServerResponse, agent: u64, out: &mut Vec<u64>) {
    if sr.owner_agent == agent {
        out.extend([sr.socket_value.to_bits(), sr.header_values.to_bits()]);
        if let Some((connection, _)) = &sr.turnloop {
            out.extend(connection.edge());
        }
    }
    listener_words(&sr.listeners, out);
    listener_words(&sr.once_listeners, out);
    out.push(sr.standalone_socket.to_bits());
    out.extend(
        sr.pending_write_callbacks
            .iter()
            .map(|&cb| address_word(cb)),
    );
    if sr.req_handle != 0 {
        out.push(handle_word(sr.req_handle));
    }
}

/// The exchange of `request` / `response` is finished: close the request and
/// park both ids. Idempotent.
pub(crate) fn park_server_exchange(request: Handle, response: Handle) {
    crate::server::request::close_incoming_message(request);
    for id in [request, response] {
        if !perry_ffi::handle_lifecycle::is_parked(id) {
            perry_ffi::park_reclaimable_ffi(id, &SERVER_PAYLOADS);
        }
    }
}
