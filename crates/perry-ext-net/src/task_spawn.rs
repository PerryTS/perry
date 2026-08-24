//! Cooperative socket-task spawning on Perry's shared Tokio runtime.

use std::pin::Pin;

// perry-ffi v0.5.x's only async-runtime entry point was originally
// `spawn_blocking`, which boxed a `FnOnce()` on Tokio's blocking pool. Socket
// futures now run cooperatively through `spawn_async`, matching perry-stdlib's
// shared-runtime path without tying up one blocking-pool thread per socket.
pub(super) fn spawn_socket_runner<F>(fut_factory: F)
where
    F: FnOnce() -> Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + 'static,
{
    // The socket is registered synchronously before this spawn, so
    // `js_ext_net_has_active_handles` keeps the event loop alive for its life.
    perry_ffi::spawn_async(async move {
        fut_factory().await;
    });
}
