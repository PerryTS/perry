//! Browser HTTP through the web backend's host fetch capability.
//!
//! Host fetch controls TLS, HTTP version, redirects, decompression, cookies and
//! proxy policy. This backend revision exposes bounded GET bodies only; custom
//! methods/headers, response metadata, streaming, raw CONNECT, explicit ALPN and
//! listening servers are unavailable. Capacity is ExecutorConfig::buffer_size.
use std::io;
use turnloop_io::{ExecutorHandle, Instant, turnloop::backend::web::Web};
/// Fetch through the registered host scheduler, with abort on timeout or drop.
pub async fn get(
    executor: &ExecutorHandle<Web>,
    url: &str,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    turnloop_io::deadline(executor, deadline, async {
        executor.fetch(url).await.map_err(turnloop_io::error)
    })
    .await
}
