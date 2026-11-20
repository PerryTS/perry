//! WASI 0.3 network capabilities over the existing Turnloop native owners.
mod conv;
mod dns;
mod slot;
pub(crate) mod tcp;
#[cfg(test)]
mod tests;
pub(crate) mod udp;
use super::bindings::sockets::types;
use crate::{sockets::WasiSocketsCtxView, TrappableError};
pub type SocketError = TrappableError<types::ErrorCode>;
pub type SocketResult<T> = Result<T, SocketError>;
impl types::Host for WasiSocketsCtxView<'_> {
    fn convert_error_code(&mut self, error: SocketError) -> wasmtime::Result<types::ErrorCode> {
        error.downcast()
    }
}
impl From<wasmtime::component::ResourceTableError> for SocketError {
    fn from(error: wasmtime::component::ResourceTableError) -> Self {
        Self::trap(error)
    }
}
impl From<crate::p2::SocketError> for SocketError {
    fn from(error: crate::p2::SocketError) -> Self {
        match error.downcast() {
            Ok(error) => types::ErrorCode::from(error).into(),
            Err(error) => Self::trap(error),
        }
    }
}
impl From<perry_component_io::Error> for SocketError {
    fn from(error: perry_component_io::Error) -> Self {
        Self::trap(error)
    }
}
impl From<crate::sockets::tcp::Failure> for SocketError {
    fn from(error: crate::sockets::tcp::Failure) -> Self {
        match error {
            crate::sockets::tcp::Failure::BrokenPipe(_) => {
                types::ErrorCode::ConnectionBroken.into()
            }
            error => error.socket().into(),
        }
    }
}
impl From<crate::p2::bindings::sockets::network::ErrorCode> for types::ErrorCode {
    fn from(error: crate::p2::bindings::sockets::network::ErrorCode) -> Self {
        use crate::p2::bindings::sockets::network::ErrorCode as P2;
        match error {
            P2::AccessDenied => Self::AccessDenied,
            P2::NotSupported => Self::NotSupported,
            P2::InvalidArgument => Self::InvalidArgument,
            P2::OutOfMemory => Self::OutOfMemory,
            P2::Timeout => Self::Timeout,
            P2::ConcurrencyConflict | P2::InvalidState => Self::InvalidState,
            P2::AddressNotBindable => Self::AddressNotBindable,
            P2::AddressInUse => Self::AddressInUse,
            P2::RemoteUnreachable => Self::RemoteUnreachable,
            P2::ConnectionRefused => Self::ConnectionRefused,
            P2::ConnectionAborted => Self::ConnectionAborted,
            P2::ConnectionReset => Self::ConnectionReset,
            P2::DatagramTooLarge => Self::DatagramTooLarge,
            P2::Unknown => Self::Other(None),
            error => Self::Other(Some(format!("{error:?}"))),
        }
    }
}
