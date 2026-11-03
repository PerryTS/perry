turnloop P0: the primary JavaScript agent now waits in its own thread-local
`turnloop::Loop` (turnloop `=0.1.0-alpha.2`) on exact `Instant` deadlines.
`js_wait_for_event` no longer truncates the next timer or stdlib deadline to
whole milliseconds, so a deadline less than a millisecond away is waited for in
one OS wait instead of being spun toward until the #1114 throttle trips. Worker
agents keep the legacy park until they get their own loop (P3/P4).

While tokio still owns in-flight native work (an O(1) predicate stdlib
registers: tokio's alive-task count plus the native in-flight counter), the
primary agent drives the existing tokio tick exactly as before; this bridge is
P0-transitional and P8 deletes it. A default-off `perry-stdlib/tokio-wait-driver`
cargo feature restores the pre-P0 driver for A/B measurement.

The event loop's per-turn keep-alive checks read counters instead of walking
state: the three timer queues (per primary agent), native async completions,
thread results, diagnostics publishes, the extension registry, stdin listeners,
IPC, stdlib pending resolutions, TLS servers/sockets and worker records.
`EXT_BLOCKING_TASKS_INFLIGHT` references are now RAII guards, so a panicking or
cancelled native task no longer pins the loop alive.

`PERRY_LOOP_STATS=1` prints the loop's turns, OS waits, zero-event waits and
transitional tokio ticks at exit (diagnostic only). The `turnloop 0.1.0-alpha.2`
exact dependency pins (`libc =0.2.175`, `wasm-bindgen =0.2.108`, …) force several
workspace-wide lockfile downgrades; see `docs/turnloop/p0-report.md`.
