# turnloop P4 — the blocking pool, and the perry-ffi async ABI v2

Branch `turnloop/p4-pool`, based on `turnloop/integration` at `14803019fc`
(P0+P1+P2+P3 merged). Built and tested on the shared Linux build box
(`perrybuilder`, EPYC 32c/64t) against the pinned gap oracle Node **26.5.1**
(`/opt/node-v26.5.1-linux-x64/bin`, not the box default 26.8.1); the runtime
unit tests also ran on macOS arm64. Nothing here was run on Windows.

## What this phase found, before what it changed

P4's scope is "work that must not run on the thread that owns the JS heap".
Perry had **four** mechanisms for it, and two of them did not take the work off
that thread at all:

| subsystem | before P4 | |
|---|---|---|
| `bcrypt` (stdlib and `perry-ext-bcrypt`) | tokio's blocking pool | one tokio thread per concurrent hash |
| `sharp` | tokio's blocking pool | ditto |
| every other `perry_ffi::spawn_blocking` caller | tokio's blocking pool | ditto |
| `napi_queue_async_work` | **one fresh `std::thread` per queued work item** | nothing bounded how many |
| `argon2.hash` / `argon2.verify` | **inline, on the JS thread** | inside an async block on the *current-thread* runtime |
| `crypto.pbkdf2` / `crypto.scrypt` / `crypto.argon2` | **inline, on the JS thread** | only the callback was deferred |
| `zlib.gzip` / `gunzip` / `deflate` / … one-shots | **inline, on the JS thread** | ditto |

The third column is the finding. `crypto.pbkdf2(pw, salt, 6_000_000, 32,
'sha256', cb)` looked asynchronous from JS and was not: measured on the build
box, the **call itself** cost **724 ms** on the base commit and **0 ms** on Node
26.5.1, because Node runs it on libuv's threadpool and Perry derived the key
before returning. `scrypt` at p=16 cost **418 ms** against **0 ms**. For the
whole of that time no timer, socket, immediate or microtask in the process
could run.

That is what `test-files/test_gap_turnloop_p4_pool.ts` pins, and it is why the
fixture measures the *call*, not a tick count: a tick count does not
discriminate at all. An inline implementation still schedules its callback a
turn later, so both arms report "the loop turned afterwards" — the asymmetry is
that the inline one turned only once the work was already finished.

## One mechanism: `crates/perry-runtime/src/turnloop_pool/`

An owned `Send` closure goes to turnloop's process-wide bounded pool
(DESIGN D8) and its result comes back as an ordinary turnloop completion on the
thread that submitted it.

A job is two closures, and the split is the whole point:

- **`work`** runs on a pool thread. `FnOnce() -> T + Send`, so only owned Rust
  data can cross. perry-runtime's arena is thread-local and a JSValue built on
  a pool thread lands in an arena the owning thread never sees (#1824) — under
  v1 that rule lived in doc comments; here it is a trait bound.
- **`deliver`** runs on the owning thread, inside the completion dispatch that
  follows a turn, and is deliberately **not** `Send`. It is where promises
  settle, JS values get built and callbacks are queued.

The API is `submit` / `submit_rooted` / `submit_or_run_inline` /
`submit_or_run_inline_rooted` / `cancel` / `turn`, plus lifetime counters.

**Exactly one delivery per accepted job** (DESIGN D4): `Done` when the pool ran
it, `Cancelled` when `cancel` won the race *or the loop shut down with the job
still outstanding*, `Failed` when the job panicked (turnloop catches the
unwind). A submission the driver **refuses** never becomes a job and reports
through `submit`'s return value, so a caller never has to guess whether its
completion will run. `refused=` on the stats line counts exactly those.

### Completion routing

One token space, disjoint from the others by construction: the top 8 bits are
the operation class — `0x20`–`0x2F` here, against P1's `1`–`7`, P2's
`0x10`–`0x1F` and P3's `TIMER_TOKEN` — and the low 56 are the job id.
`agent_loop::dispatch_staged` routes on exactly that range test, so no module
can be handed another's completion, and a stale token finds no entry and is
dropped. `the_pool_token_space_is_disjoint_from_every_other_phase` tests the
contract directly rather than inferring it from a passing workload.

### The loop profile, and why the pool shares the net one

A pool submission creates the loop at the **net** profile rather than a cheaper
pool-sized one. A profile upgrade *recreates* the loop, and a recreated loop
takes its blocking-pool `WorkPort` with it: a job still running on a worker
thread would then push its result into a closed port, which discards it, and
the awaiting promise would never settle. Sharing the net profile keeps the only
upgrade edge at Wait → Net and makes it always run *before* the submission that
needed it, so no upgrade can happen underneath an outstanding job.
`agent_loop::upgrade_profile` asserts exactly that, next to P1's handle
assertion.

The cost is the net profile's 64 × 16 KiB pooled read buffers in a process
whose only turnloop work is CPU-bound. That is a deliberate trade: a megabyte
of RSS against a class of bug that produces a promise which never settles.

## perry-ffi async ABI v2

`perry_ffi::pool`, over three new C symbols (`perry_ffi_pool_submit`,
`perry_ffi_pool_cancel`, `perry_ffi_pool_turn`):

| v2 | what it does |
|---|---|
| `submit(work, deliver)` | the two-closure contract above, across the C ABI |
| `submit_or_run_inline(work, deliver)` | as above; on refusal the work has already run inline and `deliver` has already been called, so a binding settles exactly once either way |
| `run(work)` | fire-and-forget, for a binding whose closure already settles its own promise through a deferred resolution |
| `cancel(job)` | best-effort (DESIGN D8) |
| `turn(budget_ms)` | a bounded turn, for a synchronous binding polling for a pool result |

The `ctx` box crosses as a `usize` and only `work`/`out` are touched on the
pool thread, which is what makes a non-`Send` `deliver` sound; both trampolines
contain panics with `catch_unwind` so nothing ever unwinds through an
`extern "C"` frame, whichever way perry-runtime's unwind regime is built
(#8479).

### Which v1 entry points remain, and why

- **`run_pending` is now a v1 shim over v2.** It takes a turnloop turn first —
  a turn is the only thing that collects a pool completion — and then drives
  whatever tokio work is left. The turn is deliberately **non-blocking** rather
  than given the caller's budget: this shim's callers are waiting for something
  *tokio* delivers (`js_ws_wait_for_message`), and parking their budget in
  turnloop would add a poll of latency to each of them.
- **`spawn_blocking`, `spawn_blocking_with_reactor` and `spawn_async` stay on
  tokio.** This is a decision, not an omission. Their remaining callers — the
  `node:http2` accept loop, the HTTP/2 client and request runtimes, and every
  database binding that runs `Handle::current().block_on` — hold their thread
  for the lifetime of a **connection**, not of a job. turnloop's pool is bounded
  and fixed-size by design (four threads by default), so hosting an unbounded
  number of connection-lifetime occupants on it would deadlock under load, and
  would do it to the P5 lane's code rather than to this one's. P5–P7 rewrite the
  tokio I/O inside those callers; P8 deletes the shims with tokio.
- **There is no v2 `spawn_async`.** DESIGN §9 puts it on the calling thread's
  loop executor, but every current caller's future is tokio I/O (hyper,
  tokio-tungstenite, `TcpStream`), so a loop-executor variant today would be an
  API with no caller — the untested-mode shape Perry's own GC knob kill-policy
  says not to ship. It lands with the crates that need it.

## What moved

| subject | where | note |
|---|---|---|
| `bcrypt.hash` / `compare` / `genSalt` | `perry-stdlib/src/bcrypt.rs` | tokio blocking pool → turnloop pool |
| `bcrypt` npm shim | `perry-ext-bcrypt` | also stops allocating the result string on the worker thread |
| `argon2.hash` / `verify` | `perry-stdlib/src/argon2.rs` | was inline on the JS thread |
| `argon2` npm shim | `perry-ext-argon2` | same #1824 fix as bcrypt |
| `sharp` encode / decode / metadata | `perry-ext-sharp` | `pool::run`; its settlements already deferred through `resolve_with` |
| `crypto.pbkdf2`, `crypto.scrypt`, `crypto.argon2` | `perry-stdlib/src/crypto/kdf.rs` | were inline; validation and the result Buffer stay on the owning thread |
| `zlib` one-shot codecs | `perry-stdlib/src/zlib.rs` | were inline |
| `napi_queue_async_work` | `perry-runtime/src/node_api_host/async_work.rs` | one OS thread per work item → the shared pool |

`perry-ext-bcrypt` and `perry-ext-argon2` called `promise.resolve_string(&hash)`
from *inside* the `spawn_blocking` closure — that is `alloc_string` on a tokio
blocking-pool thread, the #1824 hazard perry-stdlib's own copies had already
worked around with a deferred converter and these had not. Under v2 the split is
a trait bound, so the fix is structural rather than remembered.

### What did not move, and why

- **`crypto.hkdf`** is a single extract-and-expand, microseconds, and moving it
  would add a turn of latency for no gain.
- **`dns.lookup`** still calls `getaddrinfo` on the JS thread. It is a genuine
  P4 subject and turnloop even has a first-class `Loop::resolve` for it, but
  Perry's `dns` module builds its result JS values inside the same function
  that resolves, and `dns.promises.lookup` returns an already-settled promise —
  so moving it is a restructuring of that module rather than a transport swap,
  and it changes the ordering of every `dns.lookup` callback in the suite. It
  wants its own change with its own oracle measurement, the way P3 measured its
  phase order.
- **`perry-ext-ads`** uses v1 `spawn_blocking` only to defer four canned error
  resolutions. It is not CPU-bound and its closures call `resolve_string` on the
  worker, so migrating it is the same restructuring as bcrypt's for no
  measurable benefit.
- **Per-agent loops.** P3 noted that worker agents' own loops "wait for P4".
  They still wait: nothing in DESIGN §12's P4 row is about them, and giving
  every agent a loop means reshaping `PRIMARY_ROUTE`, the notify routing and the
  keep-alive accounting — a phase's worth of work on its own. A worker agent
  therefore gets `SubmitError::NoLoop` and runs its job inline on its own
  thread, which blocks the worker rather than the primary agent.

## GC decisions

**No JS heap memory reaches the pool**, the property P1 and P2 established —
except that here the compiler enforces it: `work` is `Send` and a JSValue is
not, so the mistake does not compile.

What is new is that a job has a lifetime and a caller may need a JS value to
survive it. `zlib.gzip(buf, cb)` holds `cb` from submission until the
compression finishes, and `crypto.pbkdf2(..., cb)` likewise; under the old
inline implementation the callback was pushed into an already-scanned queue
before anything could collect. Such a value is now parked in the job entry
through `submit_rooted`, visited by this module's **registered**
`gc_register_mutable_root_scanner` ("runtime:turnloop_pool"), and the
*rewritten* value — not the one the caller passed — is handed to `deliver`. A
raw heap pointer in a runtime-side table is a GC root that the static dominance
checker cannot see (CLAUDE.md), which is why the registration lives in the same
file as the holder.

`a_parked_js_value_survives_a_collection_and_reaches_the_delivery` is the
sabotage test: it parks a real string, runs a real collection while the job is
outstanding, and asserts the delivery still reads the string's *contents* —
an evacuation rewrites the bits, so comparing bits would pass vacuously.

The promise-side rooting is unchanged: `pool_for_promise_deferred` pins the
promise across the crossing exactly as `spawn_for_promise_deferred` did (#859),
and the deferred-resolution queue is scanned by the same `stdlib:async_bridge`
scanner as before.

`scripts/gc_runtime_root_holders.py` is green with no new inventory entry: the
new holder's scanner lives in the same file as the holder it scans.

## Behaviours that needed explicit handling

1. **The keep-alive gate had to learn about jobs.** A job accepted by the pool
   is work the process owes an answer for, and `main()` returning while a hash
   is still on a pool thread must not exit the loop — the shape #591 fixed for
   the tokio pool with `EXT_BLOCKING_TASKS_INFLIGHT`. `turnloop_pool::
   has_pending_jobs()` is read by perry-runtime's `js_stdlib_has_active_handles`
   trampoline, so it covers runtime-only binaries too.
2. **It had to stay out of the tokio-tick predicate.** The obvious move —
   reusing `InflightGuard` — would have been wrong: that counter also feeds
   `native_work_inflight`, which makes the park choose the legacy tokio tick
   instead of a turn. A pool-only workload would then never park in turnloop and
   would collect its own completions only through the 1 ms mixed-transport
   slice. The pool's counter is deliberately a separate one.
3. **A pump has to turn the loop before it drains its queue** — P2's rule,
   inherited. `drain_async_completions` (N-API) drains a queue a *thread* used
   to fill; a pool-backed work item exists only once the loop has been turned,
   so an addon's own poll loop — and this module's unit test, which drives the
   pump without parking — would otherwise spin against a queue nothing can fill.
4. **Shutdown must settle, not drop.** A job still running when the loop goes
   away would push its result into a closed `WorkPort`, which discards it: the
   caller was told "accepted" and then handed nothing. `shutdown_current_thread`
   delivers every outstanding job as `Cancelled` first, off a snapshot of the
   ids so a delivery that submits follow-up work cannot make teardown spin.
5. **A cancelled or panicking job still owes the awaiter an answer.** Every
   migrated call site settles its promise or calls its callback with an error on
   those paths; leaving the promise pending is the one outcome a caller cannot
   recover from.

## Test evidence

All commands as run.

### Runtime unit tests — real work on the real pool

```
RUST_TEST_THREADS=1 cargo test --profile perry-dev -p perry-runtime turnloop_pool
```
→ **10 passed**. Nothing is mocked: every job runs on a turnloop blocking-pool
thread and its result comes back through `Loop::turn`, and each test asserts
*which thread ran the work* by comparing thread ids, so a fixture that never
reached the pool cannot pass:

- a job proven to run on a pool thread and deliver on the submitting one;
- four megabytes hashed byte by byte on the pool with a transformed slice
  carried back, so a lost or reordered byte in **either** direction fails;
- 32 jobs all completing, with `ran == 32` and **more than one distinct worker
  thread**, because a "pool" served by a single background thread would satisfy
  every other assertion;
- a cancel that wins the race against a saturated pool, asserting **exactly
  one** delivery — not zero and not two — and that a second cancel finds the
  job already cancelled;
- a panicking job reported as `Failed` with the pool still usable afterwards;
- a bounded queue that **refuses** rather than growing, with the refusal proven
  to be backpressure, nothing silently accepted-and-dropped, and the inline
  fallback then exercised for real against that saturated pool;
- a real JS string parked across a real collection, asserted by its *contents*
  after delivery (an evacuation rewrites the bits, so comparing bits would pass
  vacuously);
- shutdown settling every outstanding job exactly once and releasing the
  keep-alive gate;
- the token space proven disjoint from P1's, P2's and P3's;
- a stale completion for an already-delivered job dropped.

```
RUST_TEST_THREADS=1 cargo test --profile perry-dev -p perry-runtime --features node-api-host node_api_host
```
→ **18 passed**, including `async_work_executes_off_thread_and_completes_on_owner`,
which now also reads `turnloop_pool::submitted_total()` around the queue. "Off
thread" alone cannot tell the pool from the old thread-per-work-item fallback —
both satisfy every other assertion in that test — so a run that fell back now
says so instead of passing quietly.

### The whole runtime suite, both arms

`RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib`, same host:

| arm | passed | failed |
|---|---|---|
| base `14803019fc` | 3981 | **2** |
| **P4** | **3991** | **2** |

Ten more passes, which is exactly this phase's ten pool tests, and the same two
failures in both arms — neither is P4's:
`gc::tests::heap_generation::a_free_or_move_outside_every_scope_is_caught_in_debug_builds`
(the funnel assertion it waits for is a `debug_assert`, and this is a release
test build) and `native_stack::tests::stack_top_respects_custom_thread_stack_sizes`
(fails in debug too, on this box). P2 and P3 both recorded the same pair.

```
RUST_TEST_THREADS=1 cargo test --release -p perry-ffi
```
→ **43 passed** (39 before this phase; the four new ones are `perry_ffi::pool`'s
outcome-mapping tests, which are the part of the ABI with no `extern` in it and
therefore the part a unit-test binary with no perry-stdlib archive can check).

### The gap fixture, against the pinned oracle

`test-files/test_gap_turnloop_p4_pool.ts` was validated against Node **26.5.1**
five times before Perry ever ran it (all `true`, 5/5), and against the **base
commit** three times to prove it discriminates:

| line | Node 26.5.1 | base `14803019fc` | P4 |
|---|---|---|---|
| `pbkdf2 call returned without deriving` | true | **false** | **true** |
| `scrypt call returned without deriving` | true | **false** | **true** |
| every digest / round-trip / concurrency line | true | true | true |

Three P4 runs are byte-identical to the oracle. The two rows that move are
exactly the two the phase is about; everything else was already correct and
stays correct, which is what says the migration did not change results while
changing where they are computed.

### `PERRY_LOOP_STATS` — the pool's work arrives as turnloop completions

Same fixture, same host, one compiler apart:

| arm | stats line |
|---|---|
| base `14803019fc` | `[perry-loop] driver=turnloop parked=0` |
| **P4** | `[perry-loop] driver=turnloop turns=23 os_waits=9 zero_event_waits=8 native_ticks=0 turn_errors=0 completions=25 …` |
| **P4** | `[perry-loop] p4 pool_submitted=25 completed=25 cancelled=0 failed=0 refused=0` |

The base arm **never parked at all**: every one of those twenty-five operations
was computed inline before the loop had anything to wait for, so turnloop
carried nothing. On P4 all twenty-five are pool jobs — 1 pbkdf2 + 1 scrypt +
1 gzip + 1 gunzip + ten round trips (two jobs each) + one failing gunzip — and
`refused=0` says the pool, not the inline fallback, was the transport for every
one of them.

### Thread counts

**Previously-inline subjects** (8 × pbkdf2 at 3M iterations,
4 × scrypt, 8 × 4 MiB gzip, twenty jobs in flight at once):

| | base | P4 |
|---|---|---|
| idle | 1 | 1 |
| 20 jobs in flight | **1** | **5** |
| after | 1 | 5 |

One thread on the base arm is the finding, not the win: all twenty jobs ran on
the thread that owns the JS heap. Five on P4 is one JS thread plus turnloop's
**four** shared pool workers, which is the bound (DESIGN D8) and does not grow
with the job count.

**tokio's blocking pool** (`scripts/turnloop/apps/pool_thread_census.ts`: 8 × `bcrypt.hash` at cost 11
plus 4 × `argon2.hash`, twelve jobs in flight), base arm, with thread names:

```
idle threads: 1 | threads_bcrypt_ x1
in-flight threads: 13 | threads_bcrypt_ x1, tokio-rt-worker x12
after threads: 13 | threads_bcrypt_ x1, tokio-rt-worker x12
[perry-loop] … native_ticks=6 … completions=1
```

Twelve concurrent hashes cost **twelve tokio threads**, one per job, and they
persist after the work finishes. The P4 arm of the same probe is in the next
section, after the loop-stats table it shares its run with.

### GC stress with pool work in flight

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 ./gcpool
```

Subject: `scripts/turnloop/apps/pool_gc_stress.ts` — a burst of twelve jobs (more than the
pool has threads, so some are still queued while collections run), JS garbage
allocated while they are out, a gzip round trip whose correctness breaks if a
parked callback moves, a scrypt, and a failing gunzip whose rejection must
still arrive.

The fixture is deliberately **not** the gap fixture: at `RATE=1` with
`ALLOC_KB=0` the collector runs at every handled safepoint, and the gap
fixture's four-million-iteration buffer fill would not finish. What has to
survive those collections is the pool's own state — the parked callbacks, the
pinned promises, the deferred resolutions — and that needs jobs in flight, not
a large JS heap.

All three seeds exit 0, with **stdout byte-identical to the unstressed run**,
and the instruments prove they were armed rather than merely quiet:

| seed | copying minors | objects moved | from-space quarantines | gc diagnostic lines | pool jobs |
|---|---|---|---|---|---|
| 1 | 468 | 14,647 | 468 | 17,812 | `pool_submitted=16 completed=16 refused=0` |
| 7 | 468 | 14,647 | 468 | 17,812 | same |
| 12345 | 468 | 14,647 | 468 | 17,812 | same |

Seed 1's own verdict line:

```
[gc-schedule] done: seed=1 safepoints=468 scheduled_collections=468 polls_paced=0 \
              copying_minors=468 moved_objects=14647 loop_polls=400
```

A run with zero copying minors quarantines nothing and would pass vacuously;
468 `[gc-fromspace-protect] retired_set=` lines say the from-space really was
detached, poisoned and `mprotect`ed, and 14,647 moved objects say survivors
really were copied — while sixteen pool jobs were outstanding. No SIGSEGV from
the quarantine reporter: no stale from-space pointer was dereferenced.

### Thread counts, continued — the tokio pool on the P4 arm

Same probe, same host, one compiler apart:

| | base `14803019fc` | **P4** |
|---|---|---|
| idle | 1 | 1 |
| 12 hashes in flight | 13 — `tokio-rt-worker x12` | **5** — `turnloop-blocki x4` |
| after | 13 | 5 |
| `native_ticks` | 6 | **0** |
| pool | — | `pool_submitted=15 completed=15 cancelled=0 failed=0 refused=0` |

Twelve concurrent hashes cost twelve tokio threads on the base arm, one per
job, and those threads persisted after the work finished. On P4 the same
workload runs on turnloop's four shared workers, which is the bound and does
not grow with the job count, and `native_ticks=0` says the loop never had to
drive the legacy tokio tick at all for it — the whole workload is turnloop's.

Fifteen jobs for twelve hashes is the right number: 8 `bcrypt.hash` +
4 `argon2.hash` + 2 `bcrypt.compare` + 1 `argon2.verify`.

### The gap suite, against a baseline built from this branch's own base

Both arms ran the same 8-shard fast tier (`PERRY_SKIP_BUILD=1`, which implies
`PERRY_NO_AUTO_OPTIMIZE=1`) against the pinned oracle on the same box, from
their own `target/release`. The baseline is `14803019fc` — this branch's base —
because the committed snapshot cannot be assumed to agree with it.

| | base `14803019fc` | P4 (`4bc3e877f9`) |
|---|---|---|
| tests | 800 | **801** (the new P4 fixture) |
| pass | 786 | **787** |
| parity_fail | **14** | **14 — the same fourteen** |
| compile_fail / crash | 0 | 0 |
| **status changes vs base** | — | **0** |

Not one test changed status in either direction, and the new fixture passes.
That is the verdict: moving seven subsystems off the JS thread — argon2, both
KDFs, `crypto.argon2`, the zlib one-shots, bcrypt, sharp and N-API async work —
and rerouting every `perry_ffi` blocking submission cost the existing suite
nothing.

The fourteen are identical in both arms and none is P4's. Nine are P3's known
set (`…_defineproperty_class_prototype`, `…_settracesigint`,
`…_static_helpers`, `disposablestack_2875`, `iterator_prototype_next_patch`,
`json_lazy_defineproperty_index`, `perfhooks_3088_3008_3010_3011`,
`prop_plan_cache_invalidation`, `v8_2_3680plus`); the other five
(`backoff_options`, `cron_cronjob`, `dayjs_factory_arg`, `moment_methods`,
`ratelimiter_memory`) are ext-routed tests that shell out to
`cargo build -p perry-ext-…` and were failing on both arms on a box running
sixteen users' builds at load 33 — P3's report documents the same shape. They
are the same five in both arms, so they cancel out of the comparison; anyone
re-running this on a quiet box should expect them to pass.

### Targeted parity for the modules this phase touched

Same tier, same oracle:

| filter | tests | pass |
|---|---|---|
| `test_gap_turnloop_p4_pool` | 1 | 1 |
| `test_parity_argon2` (expected-output) | 1 | 1 |
| `test_parity_zlib` | 1 | 1 |
| `test_gap_zlib_` | 3 | 3 |
| `test_gap_crypto_` (incl. `crypto_scrypt_options`) | 3 | 3 |
| `test_gap_webcrypto_` (the threadpool contract) | 1 | 1 |
| `test_zlib_` | 2 | 2 |

`test_gap_webcrypto_async_threadpool` is worth naming: it is the *existing*
fixture that pins "async crypto crosses at least one macrotask", written for
the same class of divergence this phase fixes from the other end, and it is
unchanged by the migration.

## The turnloop API this phase wants next

In the order that unblocks the most.

1. **A long-occupancy job class, or a second pool.** `PoolConfig` is fixed
   process-wide by the first submission (four threads by default) and a job has
   no way to declare that it will hold its thread for a connection's lifetime
   rather than a job's. That single gap is the whole reason v1
   `perry_ffi_spawn_blocking` could not be shimmed onto v2 here: the `node:http2`
   accept loop, the HTTP/2 client and request runtimes, and every database
   binding running `Handle::current().block_on` are exactly that shape, and
   putting an unbounded number of them on a bounded pool deadlocks under load.
   Either a `Blocking::detached`-style flag whose jobs run on their own threads
   outside the pool, or a second configurable pool class, would let the shim
   land before P5–P7 rewrite those crates.
2. **Grow a loop's configuration without recreating it.** `Config` is fixed at
   `Loop::new`, so Perry's Wait → Net profile upgrade *recreates* the loop — and
   a recreated loop takes its blocking-pool `WorkPort` with it, so a job in
   flight completes into a closed port and is silently discarded. Perry avoids
   the hazard by submitting every pool job at the largest profile, which costs
   that profile's pooled read buffers in a process whose only turnloop work is
   CPU-bound. A `Loop::reserve(config)` that grows in place, or a documented way
   to carry outstanding work to a successor loop, removes the workaround.
3. **A way to ask the pool about itself** — threads, busy threads, queue depth.
   A host that wants to decide "the pool is saturated, run this inline" can only
   discover it by building the job, submitting it and getting `ResourceLimit`
   back. That works and Perry uses it, but the decision is made after the
   allocation rather than before it.

Everything else in `Loop::blocking` behaved exactly as DESIGN D8 specifies:
best-effort cancellation with exactly-once completion, panics caught on the
worker and reported rather than aborting, and completions delivered through the
notifier so a parked turn wakes on them.

## For the integrator

The branch is `turnloop/p4-pool` on `origin`. Nothing here bumps the version —
the maintainer does that at merge.

Run, on a machine with the pinned oracle installed:

```bash
# unit tests (perry-runtime's are NOT parallel-safe)
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --features node-api-host
RUST_TEST_THREADS=1 cargo test --release -p perry-ffi

# the gap suite, against a baseline from this branch's OWN base commit
cargo build --release -p perry -p perry-runtime -p perry-stdlib \
  -p perry-runtime-static -p perry-stdlib-static
PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_

# the P4 fixture on its own
PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_turnloop_p4_pool

# GC stress with pool work in flight
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 ./pool_gc_stress

# the thread census (needs perry-ext-bcrypt and perry-ext-argon2)
PERRY_LOOP_STATS=1 ./pool_thread_census
```

Still to run, and **not** run here:

- **Windows and macOS.** Everything measured in this report was run on Linux
  x86_64; the runtime unit tests also ran on macOS arm64, but no end-to-end
  workload did. The pool bridge is portable Rust over turnloop's own
  cross-platform `Loop::blocking`, and neither arm has been exercised.
- **An instruction A/B at cgu=1 with a control probe** (DESIGN §12's per-phase
  requirement). **Use the base commit as the baseline, not
  `--features perry-stdlib/tokio-wait-driver`.** That feature turns the loop off
  entirely, so `with_pool_driver` returns `None` and every P4 subject takes the
  *inline* fallback rather than the tokio pool it used before this phase — the
  feature arm is no longer a faithful "before" for this phase's subjects. It
  remains one for P0–P3.
- **The auto-optimize gap tier.** Only the fast tier (`PERRY_SKIP_BUILD=1`,
  which implies `PERRY_NO_AUTO_OPTIMIZE=1`) ran here.
- **`cargo test --workspace`** and the ext crates' own suites
  (`perry-ext-bcrypt`, `-argon2`, `-sharp`), whose ABI v2 call sites are
  compile-checked here but were not run.
- **A saturation soak.** The pool is bounded at four threads; a server that
  hashes a password per request will queue. The backpressure path is unit-tested
  (`a_full_pool_queue_refuses_rather_than_growing_without_bound`), but no
  end-to-end workload has been run against a saturated pool.
