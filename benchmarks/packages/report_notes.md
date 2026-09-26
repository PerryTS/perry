## Notes, known issues and what was not measured

**Coverage by host.** Instructions (and a Linux RSS sample) come from `perrymaster` (x86-64, shared with
other agents — its wall-clock is not trustworthy, so no wall numbers are taken there). Wall-clock, cold
start and the RSS table come from the quiet M1 bench mini. The mini has no Postgres / MySQL / MongoDB /
Redis, so the database workloads have **instruction counts only**; their wall-clock arm is SKIP.

**Perry failures found by this run** (Perry commit `2febf4214e`; each reproduces outside the harness):

| workload | symptom | notes |
|---|---|---|
| `qs/parse_nested` | wrong checksum, different run to run; some runs hang (>200 s for n=5000, which Node does in 0.1 s) | n ≤ 3000 is correct; nondeterministic above that — looks like a GC / heap-corruption class bug, not a qs logic bug. |
| `node-cron/match` | `ScheduledTask.match(date)` is always `false` (checksum `00000000`) | deterministic wrong answer. |
| `jsonwebtoken/rs256` | `TypeError: Cannot read properties of undefined (reading 'update')` in `jwa` sign | RS256 signing path (`crypto.createSign`); HS256 works. |
| `fastify/inject` | `setHeader is not a function` | open issue #10454 (light-my-request `ServerResponse` subclass has an empty prototype). `fastify/listen_fetch` (real socket) works. |
| `rate-limiter-flexible/consume` | correct at n=5000; at n=20000 the async step driver trips its runaway re-entry guard and rejects (`issue #712/#921/#922 guard`) | the workload throws `RateLimiterRes` across `await` inside `try/catch` for every rejected consume. |
| `pg/select`, `pg/insert_batch` | SIGSEGV on the first query | no open issue found for this. |
| `mysql2/select`, `mysql2/insert_batch` | SIGSEGV | known: #11366 (segfault, fix pending) / #11341 (wrong results). |
| `mongodb/insert_find` | `Cannot read properties of undefined (reading 'state')`, then hangs until the 600 s timeout | `mongodb/batch_query` on the same server works. |
| `redis/set_get` | one n=2000 instruction run exited with no checksum line (the correctness run and a later re-run were correct) | intermittent; that arm was excluded from timing. |
| `axios/*` (mini only) | `MODULE_NOT_FOUND …/node_modules/mime-db/db.json` | **portability**: the binary resolves a runtime `require()` of a JSON file through the *build host's absolute path*. The binaries were compiled on another Mac and copied to the mini, so the path did not exist there. Works on Linux, where it ran from its build tree. |

**Bun failures** (not Perry's): `mongodb/*` — Bun 1.3.14 throws `node:v8 isBuildingSnapshot is not yet
implemented` while loading `bson`.

**`exponential-backoff/retry` wall ratio is not a compute comparison.** The workload retries with a
0 ms delay: Node clamps `setTimeout(0)` to ≥ 1 ms, Perry fires it immediately, so Perry's per-iteration wall is
~300× lower for timer-semantics reasons. The instruction ratio is the meaningful number.

**Bare-loop control.** The control's hot op is one call of a tiny integer function. Perry spends ~1,250
instructions per iteration on it (Node ~8, Bun ~7) — see its attribution. That floor sits under every
other Perry number here.

**Two-N caveats.** Node/Bun instruction counts include their JIT and GC threads; Perry's include its GC.
Per-iteration numbers assume cost is linear between n1 and n2 (true for every workload here once warmed).

**Not done in Phase 1:** deep profiling (Phase 3); a comparison against the removed native bindings (owner
decision: they were buggy); tursodb / iroh; a CI/nightly job (the harness is deliberately not wired into any
required gate).
