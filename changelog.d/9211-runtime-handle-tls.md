On Apple aarch64, runtime-handle scope creation, pushes and reads now take the
handle-stack address from the already-published `HotTls` cache instead of
resolving the thread-local on every operation. Startup retains a raw TLS
fallback that cannot recursively enter `HotTls::fill`, and a teardown guard
clears the cached address before the stack storage is destroyed. Other targets
keep their ordinary fixed-offset TLS path.
