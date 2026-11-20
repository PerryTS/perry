### turnloop P7 — the database drivers move off tokio's blocking pool

Perry's MySQL, PostgreSQL, Redis and MongoDB bindings each held one tokio
blocking-pool thread per in-flight operation: `perry_ffi::spawn_blocking` around
`tokio::runtime::Handle::current().block_on(async { … })`. P4's report named that
pattern as the reason tokio's blocking pool survives its phase — turnloop's pool
is bounded and fixed-size, and a connection-shaped occupant cannot be hosted on
it. This phase replaces the pattern rather than rehosting it.

**`crates/perry-db-turnloop`** is the new shared host driver: it drives a
sans-I/O protocol core over P1's `turnloop_net` sockets, from the event loop's
own completion dispatch. A connection becomes one turnloop handle plus a
protocol state machine and a table of outstanding operation tokens. No thread is
held at any point, so N connections cost N descriptors and the agent's own
thread.

Each binding keeps its legacy transport for the cases that decline — a
`worker_threads` agent (no loop of its own), the `tokio-wait-driver` A/B arm, and
any TLS connection, because a database binding has no TLS layer to hand an
upgrade to. This is a narrowing, not a removal: those paths are real and still
exercised.

`turnloop_net`'s `MAX_SUBSYSTEMS` rises from 4 to 8. Each database binding is a
separately linked `staticlib` with its own completion sink, so four of them need
four slots even though they share one transport module.

No JS-visible surface moves: the same symbols, the same resolved values, the
same rejection messages. One latent defect is fixed on the way: `js_ioredis_hgetall`
built its result object *inside* the `spawn_blocking` closure — an arena
allocation on a pooled worker thread, the #1824 hazard `JsPromise::resolve_with`
exists to prevent. Every reply now crosses to the main thread as owned Rust data.

Full writeup, including the thread counts before and after: `docs/turnloop/p7-report.md`.
