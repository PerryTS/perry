Fixed the build of `perry-stdlib` with no default features, which the
auto-optimize relink uses. The worker pump called
`common::async_bridge::js_stdlib_process_pending`, which exists only with the
`async-bridge` feature. The call now goes through `worker_threads::async_shim`,
whose inline counterpart returns 0 because nothing is ever pending without the
bridge.
