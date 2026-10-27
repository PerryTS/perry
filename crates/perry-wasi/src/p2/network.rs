// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use crate::{p2::bindings::sockets::network::ErrorCode, TrappableError};

pub type SocketError = TrappableError<ErrorCode>;
pub type SocketResult<T> = Result<T, SocketError>;

impl From<wasmtime::component::ResourceTableError> for SocketError {
    fn from(error: wasmtime::component::ResourceTableError) -> Self {
        Self::trap(error)
    }
}
impl From<std::io::Error> for SocketError {
    fn from(error: std::io::Error) -> Self {
        ErrorCode::from(error).into()
    }
}

/// WASI 0.2 network capability handle. Permissions live in the owning context,
/// as in upstream Wasmtime; every use still validates this table resource.
pub struct Network {
    pub(crate) identity: std::sync::Arc<()>,
}
