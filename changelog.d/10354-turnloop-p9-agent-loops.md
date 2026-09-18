### turnloop P9 — a `turnloop::Loop` per JS agent

`event_pump::agent_loop::net_available()` was `current_agent() == PRIMARY_AGENT`,
so on any JS thread that was not the primary agent — a `node:worker_threads`
Worker, a `perry/thread` agent — sockets, `fetch`, SMTP and all four database
drivers fell back to tokio. That fallback is why tokio could not be deleted: it
was live code, not dead code, and every group in P8's removal plan is gated
behind it.

The admission decision is now "do I have (or may I take) a loop", which is true
on every thread that runs a JS agent's event loop.

- `PRIMARY_ROUTE` becomes `ROUTES`, one entry per agent, holding that agent's
  wake endpoint and a flag saying whether its owner is inside `turn`.
  `PARKED_LOOPS` keeps a wake producer's fast path at the single atomic load the
  one-route design had.
- The route slot is claimed BEFORE the loop is built, so `net_available()` and
  `ensure_loop_with()` cannot disagree. They did once (`c13372cc70`): a Worker
  reported `PRIMARY_AGENT`, the submit guard accepted its `fetch()`, and
  `ensure_loop_with` refused a moment later — a failure after acceptance rather
  than a fallback.
- Exactly one thread owns an agent's loop; a second thread acting for the same
  agent (Android's UI thread pumping for `perry-native`) keeps the legacy park.
- `js_notify_main_thread` broadcasts to every parked agent, because the flag it
  sets and the condvar it signals are both process-global; a point-to-point wake
  addressed to the primary agent would leave a Worker asleep on a
  `postMessage`-driven resolution — a hang, not an error.
- `agent::retire_agent` tears the agent's loop down, settling its outstanding
  operations first, which is the only thing that turns them into completions the
  bindings can see.
- `turnloop_proc` and `turnloop_pool` band their per-thread ids by agent, so an
  id that crosses agents misses the table instead of aliasing another agent's
  entry. The primary agent's band is unchanged.

A Worker's `fetch`, `net.connect` and Redis round-trip now run on that agent's
own loop (`native_ticks=0`, `p6 declined=0`, process `tokio_ticks=0`); on the
base commit its `fetch` was declined to reqwest, its socket returned no bytes at
all and its Redis calls returned `undefined`. The `[perry-loop]` stats line gains
`agent=` as a SUFFIX, never an insertion, because two instruments parse it
positionally.

Measured cost: an idle agent loop costs nothing (an agent that never submits
never builds one, at 1 agent or at 64), and a net-profile loop costs about
3.4 MB — which at 1 and 8 concurrent agents is still less RSS than the reqwest
stack it replaces. A `parallelMap` over 64 cores creates zero loops.

Full writeup, both arms of the gap suite, the GC-stress table and three defects
found and deliberately not fixed: `docs/turnloop/p9-report.md`.
