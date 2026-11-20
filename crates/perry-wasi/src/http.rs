//! WASI HTTP resources, canonical interfaces and owned HTTP/1.1 transports.
//! The embedding-owned server supplies requests without granting guest network access.
pub mod body;
mod error;
mod fields;
pub mod p2;
pub mod server;
pub mod transport;
pub use error::Error;
pub use fields::{FieldMap, FieldMapError};

// The WASI HTTP host owns these transport/framing headers. Guest fields cannot
// override them. Adapted from wasmtime-wasi-http 48.0.5.
fn forbidden_header(name: &::http::HeaderName) -> bool {
    matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "http2-settings"
    )
}
