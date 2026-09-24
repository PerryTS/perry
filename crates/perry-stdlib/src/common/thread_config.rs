//! Stack sizing shared by synchronous workers and optional async bridges.

/// #10399: stack reservation for perry-spawned threads — tokio's blocking
/// pool (`tokio_bridge::RUNTIME`), `worker_threads` Workers, and the plain
/// threads `perry_ffi_spawn_blocking` falls back to without tokio.
pub fn blocking_thread_stack_size() -> usize {
    const DEFAULT: usize = 32 * 1024 * 1024;
    std::env::var("PERRY_THREAD_STACK_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1024 * 1024)
        .unwrap_or(DEFAULT)
}
