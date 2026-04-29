//! Common utilities for stdlib modules

pub mod handle;
// Tokio-backed promise/runtime bridge — only needed when an async feature
// (http-server/client, websocket, databases, email, scheduler, rate-limit,
// crypto's bcrypt path, …) pulls in `async-runtime`. Always-on code that
// references it must also be `#[cfg(feature = "async-runtime")]`-gated.
#[cfg(feature = "async-runtime")]
pub mod async_bridge;
pub mod dispatch;

#[cfg(feature = "async-runtime")]
pub use async_bridge::*;
pub use dispatch::*;
pub use handle::*;
