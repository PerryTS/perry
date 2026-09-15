Install a thread-local turnloop wait driver for the primary JavaScript agent.
Preserve timer Instant deadlines through GC idle work and OS waits, and report
actual turns, OS waits and empty waits with PERRY_LOOP_STATS=1. Runtime-only timer
programs use the same driver. Existing workers keep their legacy path pending
per-agent routing in P3/P4.

Keep native Tokio work on its existing current-thread tick during P0, selecting
that bridge with maintained in-flight/task counts. A default-off
perry-stdlib/tokio-wait-driver feature retains the old driver for migration A/Bs.

Replace timer, native request, TLS and worker/channel liveness walks with balanced
membership counters. Add deadline/no-spin, cross-thread wake, teardown, and
counter-balance coverage plus executable TypeScript statistics probes.

See docs/turnloop/p0-report.md for validation, dependency pins, and remaining
migration boundaries.
