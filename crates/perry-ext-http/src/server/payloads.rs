//! A finished server exchange's `IncomingMessage` / `ServerResponse` payloads
//! belong to the JS values that name them, as the client's do
//! (`crate::client_payloads`, perry-feather#111).
//!
//! When the response ended (or can never be flushed) the server finalizes the
//! pair: it closes the request and parks both ids with [`SERVER_PAYLOADS`].
//! A handler or a later callback that kept `req` / `res` reads its url,
//! headers and status like Node; a write after the connection went away hits
//! the real, finished response. A full trace marks a parked payload's edges
//! only through a traced word naming its id (or while young, through
//! perry-ffi's parked-record scanner), and the trace that finds none
//! drops the payload and gives the id back. Until then the id cannot be
//! reissued, so no late caller reaches a recycled pair.
use perry_ffi::handle_lifecycle::ParkedRecordOwner;
use perry_ffi::{GcRootVisitor, Handle};

use crate::server::request::IncomingMessage;
use crate::server::response::ServerResponse;

/// The owner of parked server request/response records.
pub(crate) struct ServerPayloads;

pub(crate) static SERVER_PAYLOADS: ServerPayloads = ServerPayloads;

impl ParkedRecordOwner for ServerPayloads {
    // A payload holds strings, vectors, closure addresses and a connection
    // snapshot; perry-ffi's drop of it runs no JS and allocates nothing on
    // the GC heap.
    fn visit(
        &self,
        payload: &mut (dyn std::any::Any + Send + Sync),
        visitor: &mut GcRootVisitor<'_>,
    ) {
        if let Some(im) = payload.downcast_mut::<IncomingMessage>() {
            crate::server::scan_incoming_message_roots(im, visitor);
        } else if let Some(sr) = payload.downcast_mut::<ServerResponse>() {
            let mut named = Vec::new();
            crate::server::scan_server_response_roots(sr, visitor, &mut named);
            for mut word in named {
                visitor.visit_nanbox_u64_slot(&mut word);
            }
        }
    }

    fn bytes(&self, payload: &(dyn std::any::Any + Send + Sync)) -> usize {
        if let Some(im) = payload.downcast_ref::<IncomingMessage>() {
            im.retained_bytes()
        } else if let Some(sr) = payload.downcast_ref::<ServerResponse>() {
            sr.retained_bytes()
        } else {
            0
        }
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
