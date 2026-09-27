## Notes, known issues and what was not measured

**Build.** Perry arm = `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` (the workspace
`[profile.release]`: thin LTO, codegen-units=1, opt-level 3, panic=abort — not `dist`), then plain
`perry compile <wl>.ts -o <bin>` with **no flags**, so auto-optimize is ON: every compile resolved the workspace from
the binary's own path and rebuilt (or reused) a per-feature-set `libperry_{runtime,stdlib}.a` under
`target/perry-auto-*` (the † compiles below). No `PERRY_GC_*`, `PERRY_WRITE_BARRIERS`, debug or `PERRY_NO_AUTO_OPTIMIZE`
variable was set; the environment is recorded in each `compile.json` (`PERRY_BUILD_COMMIT` was set to the commit
perry was built at — it only stamps the archive build id so harness commits on top don't trip the stale-archive
guard; `PERRY_RUNTIME_DIR`, present in the Linux login profile, was unset). The macOS binaries were compiled by a perry built from `21c9f5438a` = main `36420d2e56` plus this PR's harness-only commits (no compiler/runtime change). Binaries are stripped (default); the
liveness/attribution pass recompiled the same sources with `PERRY_KEEP_SYMBOLS=1`.

**Hosts.** Instructions (+ a Linux RSS sample) on `perrymaster` (x86-64, shared with other agents; its wall-clock is
not trusted, so none is reported from it). Wall-clock, cold start and the RSS table on the quiet M1 bench mini. The
mini has no Postgres / MySQL / MongoDB / Redis, so the database workloads have **instruction counts only**.
Measurements on both hosts took the shared mkdir mutex `/tmp/perry-bench-lock.d` (see
`/tmp/perry-bench-lock.README` on each host), which the Phase 3 profiling agent and the tokio-measure agent also use.

**Controls and the first-draft ~1,040 instr/iter.** An earlier draft reported Perry's bare-loop control at ~1,040
instructions/iteration (Node 58). That was harness cost, not a call floor: every workload re-read `it.n` (a
property of the object returned by `iters()`) in its loop condition, and the checksum helpers called `Math.imul`;
each is a by-name property read in Perry (`js_object_get_field_by_name_f64` → `inherited_read_cache_lookup`, the
cost reported in #11420). Loop bounds are now hoisted into locals and the helpers use an exact
`((x << 24) + x * 403) >>> 0`. `control/bare_loop` (plain function call, no `this`, no property read) is now ~26
instructions/iteration for Perry. `control/prop_read` adds one static-key read of a plain object per iteration
and costs Perry ~93 more instructions (Node ~0 extra) — kept as a visible probe of #11420-style read cost. **No
control is subtracted from any workload.**

**Perry failures on main `36420d2e56`** (each re-run on main and reproduced outside the harness):

| workload | symptom | issue |
|---|---|---|
| `node-forge/*` (all 4) | **regression**: compile fails, `node-forge/lib/prime.js` emits invalid LLVM IR (`use of undefined value`). Compiled and ran at `2febf4214e`. | #11450 |
| `jsonwebtoken/rs256` | `crypto.createSign('RSA-SHA' + bits)` via CJS `require('crypto')` returns `undefined` → `reading 'update'` in `jwa` | #11447 |
| `node-cron/match` | `task.match(date)` returns `null` for tasks created inside `Array.map` (checksum `00000000`) | #11446 |
| `pg/select`, `pg/insert_batch` | first parameterized query segfaults; plain queries work | #11459 |
| `mongodb/insert_find` | `insertOne` after `deleteMany` rejects with `reading 'state'`, then the process hangs — now skipped as `known_hang` | #11460 |
| `rate-limiter-flexible/consume` | awaiting non-Error rejections in `try/catch` trips the async step driver's runaway re-entry guard (n=20000; n=5000 fine) | #11449 |
| `fastify/inject` | `setHeader is not a function` (light-my-request `ServerResponse` subclass) | #10454 (existing) |
| `axios/*` (portability) | a compiled binary `require()`s `mime-db/db.json` through the **build host's absolute path**; the binary breaks when moved to another machine or when the tree moves. The mini run avoided it only by building and running under the same `/private/tmp/claude-pkg-bench` path. | #11448 |

**Fixed between `2febf4214e` and main** (failed in the first run, pass on main): `qs/parse_nested` (nondeterministic
checksum / hangs above n≈3000 — 3/3 correct at n=5000 plus every harness run on main), `mysql2/*` (SIGSEGV; #11335),
`redis/set_get` (one intermittent empty-output run at `2febf4214e`; 15/15 plus all harness runs correct on main — no
issue filed).

**Bun failures** (not Perry's): `mongodb/*` — Bun 1.3.14 throws `node:v8 isBuildingSnapshot is not yet implemented`
while loading `bson`.

**`exponential-backoff/retry` wall ratio is not a compute comparison.** It retries with a 0 ms delay: Node clamps
`setTimeout(0)` to ≥ 1 ms, Perry fires it immediately. Use its instruction ratio.

**Two-N caveats.** Node/Bun instruction counts include their JIT and GC threads; Perry's include its GC. The method
assumes cost is linear between n1 and n2 (true for every workload here once warmed).

**Not done in Phase 1:** deep profiling (Phase 3 — the attribution column is a flat top-5 only); a comparison
against the removed native bindings (owner decision: they were buggy); tursodb / iroh; a CI/nightly job (the harness
is deliberately not wired into any required gate).
