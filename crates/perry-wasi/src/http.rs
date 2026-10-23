//! HTTP resource and streaming primitives. Native transport and WIT registration
//! are being integrated; this module does not yet enable `wasi:http` imports.
pub mod body;
mod error;
mod fields;
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
