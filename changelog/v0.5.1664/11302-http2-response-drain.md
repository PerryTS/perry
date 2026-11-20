Fix HTTP/2 compat responses stalling when a producer waits for `drain` after `res.write()` returns false. Accepted writes now preserve their backpressure result without duplicate buffering; drain readiness accounts for the stream's flow-control outbox, and unfinished responses retain their handles until completion.

The native 32-chunk regression now delivers exactly 2 MiB and matches Node 26.5.1, instead of timing out with zero bytes. Three focused regressions fail before the fix and pass afterward; all 180 HTTP crate unit tests pass. No version bump.
