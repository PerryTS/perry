//! WASI 0.3 HTTP resources and streaming exchanges over Turnloop.
pub mod bindings;
mod body;
mod client;
mod conv;
mod helpers;
mod options;
#[cfg(test)]
mod tests;
mod types;

use crate::{
    http::{
        p2::{WasiHttp, WasiHttpCtxView},
        FieldMapError,
    },
    TrappableError, WasiView,
};
use bindings::http::types as wit;
pub use body::{Request, Response};
use wasmtime::component::Linker;
pub type RequestOptions = options::MaybeMutable<crate::http::p2::RequestOptions>;
pub type HttpError = TrappableError<wit::ErrorCode>;
pub type HttpResult<T> = Result<T, HttpError>;
pub type HeaderError = TrappableError<wit::HeaderError>;
pub type HeaderResult<T> = Result<T, HeaderError>;
pub type RequestOptionsError = TrappableError<wit::RequestOptionsError>;
pub type RequestOptionsResult<T> = Result<T, RequestOptionsError>;
impl From<FieldMapError> for HeaderError {
    fn from(error: FieldMapError) -> Self {
        match error {
            FieldMapError::Immutable => wit::HeaderError::Immutable,
            FieldMapError::InvalidHeaderName | FieldMapError::InvalidHeaderValue => {
                wit::HeaderError::InvalidSyntax
            }
            FieldMapError::TooManyFields | FieldMapError::TotalSizeTooBig => {
                wit::HeaderError::SizeExceeded
            }
            FieldMapError::Forbidden => wit::HeaderError::Forbidden,
        }
        .into()
    }
}
impl From<crate::http::Error> for HttpError {
    fn from(error: crate::http::Error) -> Self {
        wit::ErrorCode::from(error).into()
    }
}
impl From<wasmtime::component::ResourceTableError> for HttpError {
    fn from(error: wasmtime::component::ResourceTableError) -> Self {
        Self::trap(error)
    }
}
pub fn add_to_linker<T: WasiView + 'static>(linker: &mut Linker<T>) -> wasmtime::Result<()> {
    fn view<T: WasiView>(state: &mut T) -> WasiHttpCtxView<'_> {
        let view = state.ctx();
        WasiHttpCtxView {
            ctx: &mut view.ctx.http,
            table: view.table,
        }
    }
    bindings::http::types::add_to_linker::<T, WasiHttp>(linker, view::<T>)?;
    bindings::http::client::add_to_linker::<T, WasiHttp>(linker, view::<T>)?;
    Ok(())
}
