//! Compatibility Rust exports; the runtime owns the shared C ABI.
//! WASI guest links include the runtime without the native stdlib archive.
pub use perry_runtime::async_local_storage::*;
