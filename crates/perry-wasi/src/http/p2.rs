//! Canonical WASI HTTP resources. Guest calls and resource-table mutations stay
//! on the calling thread; the owned transport runs on the existing I/O driver.
mod error;
mod handlers;
mod options;
mod outgoing;
mod resources;
#[cfg(test)]
mod tests;
use super::{transport, FieldMap};
pub use error::{HeaderError, HeaderResult, HttpError, HttpResult};
pub use options::RequestOptions;
use perry_component_io::Submitter;
pub use resources::*;
use wasmtime::component::{HasData, ResourceTable};

pub struct WasiHttp;
impl HasData for WasiHttp {
    type Data<'a> = WasiHttpCtxView<'a>;
}
pub struct WasiHttpCtx {
    pub policy: transport::DestinationPolicy,
    pub options: transport::Options,
    pub field_size_limit: usize,
    pub(crate) submitter: Submitter,
}
impl WasiHttpCtx {
    pub fn new(submitter: Submitter) -> Self {
        Self {
            policy: Default::default(),
            options: Default::default(),
            field_size_limit: 128 * 1024,
            submitter,
        }
    }
}
pub struct WasiHttpCtxView<'a> {
    pub ctx: &'a mut WasiHttpCtx,
    pub table: &'a mut ResourceTable,
}
pub(crate) fn get_content_length(fields: &FieldMap) -> Result<Option<u64>, super::Error> {
    let mut length = None;
    for value in fields.get_all("content-length") {
        let bytes = value.as_bytes();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            return Err(super::Error::HttpProtocolError);
        }
        let next = value
            .to_str()
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(super::Error::HttpProtocolError)?;
        if length.is_some_and(|n| n != next) {
            return Err(super::Error::HttpProtocolError);
        }
        length = Some(next);
    }
    Ok(length)
}
pub mod bindings {
    wasmtime::component::bindgen!({
        path: "wit", world: "http-provider", require_store_data_send: true,
        imports: { default: trappable }, exports: { default: async },
        with: {
            "wasi:io": crate::io::bindings::wasi::io,
            "wasi:http/types.fields": crate::http::FieldMap,
            "wasi:http/types.request-options": crate::http::p2::RequestOptions,
            "wasi:http/types.outgoing-body": crate::http::body::OutgoingBody,
            "wasi:http/types.incoming-body": crate::http::body::IncomingBody,
            "wasi:http/types.future-trailers": crate::http::body::FutureTrailers,
            "wasi:http/types.outgoing-request": crate::http::p2::HostOutgoingRequest,
            "wasi:http/types.incoming-request": crate::http::p2::HostIncomingRequest,
            "wasi:http/types.outgoing-response": crate::http::p2::HostOutgoingResponse,
            "wasi:http/types.incoming-response": crate::http::p2::HostIncomingResponse,
            "wasi:http/types.response-outparam": crate::http::p2::HostResponseOutparam,
            "wasi:http/types.future-incoming-response": crate::http::p2::HostFutureIncomingResponse,
        },
        trappable_error_type: {
            "wasi:http/types.error-code" => crate::http::p2::HttpError,
            "wasi:http/types.header-error" => crate::http::p2::HeaderError,
        },
    });
    pub use wasi::*;
    pub mod sync {
        wasmtime::component::bindgen!({
            path: "wit", world: "http-provider", require_store_data_send: true,
            imports: { default: trappable },
            with: {
                "wasi:io": crate::io::sync::wasi::io,
                "wasi:http": crate::http::p2::bindings::http,
            },
        });
        pub use wasi::*;
    }
}
