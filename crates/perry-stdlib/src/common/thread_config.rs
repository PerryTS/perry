//! Stack sizing shared by synchronous workers and the async bridge's fallback threads.

/// #10399: stack reservation for perry-spawned threads — `worker_threads`
/// Workers, and the plain threads `perry_ffi_spawn_blocking` and the container
/// executor fall back to when turnloop's long set refuses a job. (It sized
/// tokio's blocking pool too, until tokio was removed.)
pub fn blocking_thread_stack_size() -> usize {
    const DEFAULT: usize = 32 * 1024 * 1024;
    std::env::var("PERRY_THREAD_STACK_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1024 * 1024)
        .unwrap_or(DEFAULT)
}
