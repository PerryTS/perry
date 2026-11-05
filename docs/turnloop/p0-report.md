# turnloop P0 — wait driver, Instant deadlines, O(1) keep-alive

Branch `turnloop/p0-wait-driver`, based on Perry `main` `1cd160f3d1`. Local
host: macOS arm64 (M-series, 10 cores), shared and heavily loaded during the
work (load average 80–150 from another session). Nothing here was measured on
Linux or Windows.

## Commits

| SHA | What |
|---|---|
| `fed96bdb4d` | Codex checkpoint (partial, pre-existing) |
| `0df1f6eecc` | Revert of the checkpoint (reasons in the commit message) |
| `55d55221df` | `turnloop =0.1.0-alpha.2` dependency and lockfile |
| `74989142c6` | Wait driver, Instant deadlines, coexistence bridge, A/B feature, O(1) keep-alive, unit tests |
| `a03042441f` | Probes, statistics scripts, gap test, changelog fragment, this report |
| `907ea0a73c` | `PERRY_LOOP_STATS` wait metrics (`event_pump/loop_stats.rs`) and their tests |
| `cf04ec1ca7` | Server A/B harness (`scripts/turnloop/server_ab.py`) and its `node:http` subject |

The checkpoint was reverted rather than amended. Its `Cargo.lock` was
hand-spliced, its turnloop wake took a process-wide mutex on every
cross-thread notify, several keep-alive conversions were regex-applied and
missed transitions, it turned `MessagePort.onmessage` into an accessor
property (observable in JS) and it changed `setTimeout` delay normalization
(0 ms → 1 ms). The code in `74989142c6` was written fresh.

## Design

### Where the loop lives, and per-agent ownership

- The loop is in **perry-runtime** (`crates/perry-runtime/src/event_pump/agent_loop.rs`),
  not perry-stdlib. Timer- and promise-only programs link runtime-only and
  park in `js_wait_for_event` too; a stdlib-installed driver would never run
  for them, and those are exactly the programs the sub-millisecond spin hit.
- **One `turnloop::Loop` per JS agent, thread-local.** It is created by the
  primary agent's first real park (not by fast-path calls, so a program that
  never parks never opens a poller) and destroyed at the process-exit funnel
  `js_gc_release_current_thread_collection_side_allocations` (and in
  `js_unsettled_top_level_await_exit`). Thread exit also drops it. There is no
  process-global loop.
- The only process-global piece is `PRIMARY_ROUTE`: the primary agent's
  `Notifier` (a wake endpoint, not the loop) and an `in_turn` flag, because
  `js_notify_main_thread` addresses the primary agent by definition.
- **Workers keep today's behaviour.** A thread whose `current_agent()` is not
  the primary agent is declined for life and runs the unchanged legacy park
  (condvar, or the stdlib's registered tokio tick). A second thread acting for
  the primary agent (a host pump thread) is declined too; exactly one thread
  owns the route. `perry/thread` workers cannot `await`; `worker_threads`
  Workers that await use the legacy path.
- P0 submits no turnloop operation. The loop is sized for that
  (`max_handles`/`max_operations`/`events_per_turn`/`post_capacity` = 16, no
  pooled read buffers). The default `Config` preallocates 256 × 16 KiB
  buffers and 4096 operation slots, megabytes for a process that only waits.

### The precise park (`event_pump/precise_wait.rs`)

1. Fast path unchanged: a pending notify or microtask returns at once after
   `invoke_wait_driver_fast()` (the unchanged stdlib fast drive) and
   `agent_loop::fast_turn()`, which turns `Timeout::Now` only when the loop has
   outstanding work (`alive()`), i.e. never in P0 — no OS call on the hot
   promise path.
2. Deadline = min over the three timer queues (`Option<Instant>`, same filters
   as the C functions), the stdlib provider (fractional ms, anchored to a
   clock read taken *after* it returns so conversion can only be late), the
   loop's own `next_deadline()` (always `None` in P0) and the 1 s idle cap.
3. `deadline <= now` → the shared zero-budget return (throttle + fast drive).
4. The GC idle-reclaim hook is offered the budget only when it is ≥ 1 ms (it
   works in 4 ms slices; the legacy path never offered it a zero budget). Its
   `Park(remaining)` is always "caller's deadline minus time spent", which
   the absolute deadline already encodes, so the park uses the deadline.
5. Native work in flight → the legacy tokio tick (see coexistence below).
6. Otherwise one `Loop::turn(Timeout::Until(deadline))`. A turn error falls
   back to the condvar park for the remaining budget and is counted.

**Wake protocol.** The owner sets `in_turn`, then re-reads `NOTIFIED` and the
native in-flight predicate, then turns. A producer publishes its work (stores
`NOTIFIED`, or makes native work visible), then loads `in_turn` (all `SeqCst`);
only when it is set does it lock the route and call `Notifier::notify()`,
whose RUNNING/PARKED/NOTIFIED handshake covers the window before the OS wait.
Outside a turn, `js_notify_main_thread` pays one extra atomic load: no lock,
no syscall and no stale turnloop notification bit.

**The #1114 spin throttle stays**, as a safety net only. With exact deadlines
the zero-budget branch no longer fires for a deadline that is merely
sub-millisecond away. It still fires, legitimately and transiently, when a
timer is due. A sustained run needs a deadline source that reports a due
deadline its pump never consumes (the original #1114 shape). All deadline
sources are still Perry's own queue scans until P3, so nothing rules that out
structurally.

### Transitional coexistence with tokio (P8 deletes it)

- Rule: stdlib registers `js_register_native_inflight` with an O(1)
  predicate: tokio's `RuntimeMetrics::num_alive_tasks() != 0` on the shared
  current-thread runtime, or `EXT_BLOCKING_TASKS_INFLIGHT != 0`. While it is
  true the primary agent drives the registered tick
  (`stdlib_wait_driver` → `run_one_tick(ms)`) exactly as before, including
  its whole-millisecond budget and 1 ms floor. While it is false the wait is a
  pure turnloop turn.
- Every task on the shared runtime counts: fetch connections, net/ws/db
  connection tasks, server accept loops. A server process therefore keeps
  today's tokio tick for as long as it serves. The turnloop path covers
  timer/promise programs, programs whose only native work is cross-thread
  (child_process reactors, fs, stdin, dgram) and the idle time of async
  programs between native operations.
- The fast path still calls the unchanged `stdlib_fast_drive` with its own gate
  (`EXT_BLOCKING_TASKS_INFLIGHT` or the extension registry). It is deliberately
  not widened to "any alive task": that 1 ms tick on every notified iteration
  would slow promise-heavy servers.
- Spawns from another thread (a `worker_threads` Worker, a blocking-pool
  closure) cannot wake a turnloop wait through tokio's own driver unpark. All
  stdlib spawn sites (`async_bridge::spawn_native`, `perry_ffi_spawn_*`, cron,
  the framework server) call `js_native_work_submitted()` after spawning. It
  wakes a primary agent that is inside a turn; otherwise it is one atomic load.
  No tokio helper thread or sidecar exists.

### FFI shape changes

| Symbol | Change |
|---|---|
| `js_register_native_inflight(Option<extern "C" fn() -> i32>)` | new, P0-transitional |
| `js_native_work_submitted()` | new, P0-transitional |
| `js_register_stdlib_next_wake` provider | contract now **fractional** milliseconds; readline returns the exact remainder (its `+1 ms` ceiling kept only under `tokio-wait-driver`) |
| `js_timer_next_deadline` / `js_callback_timer_next_deadline` / `js_interval_timer_next_deadline` | unchanged whole-ms C shapes (embedders, legacy park); now derived from the internal `Option<Instant>` functions, equal by construction (truncation commutes with `min`) |
| `perry_next_wake_ms` (embedder API) | still the min of the above plus the stdlib provider, so its stdlib component can now be fractional |
| `js_register_wait_driver` | unchanged; now the primary agent's transitional tick, the workers' park and the A/B arm |
| `perry_runtime::event_pump::{shutdown_wait_driver, loop_statistics, LoopStats}` | new Rust API |
| `perry_runtime::event_pump::loop_stats::{snapshot, LoopWaitStats, format_line, begin_fast_drive, end_fast_drive, …}` | new Rust API (wait metrics). Deliberately **not** `extern "C"`: perry-stdlib links perry-runtime as an rlib, so the hooks add no FFI symbol and no contract to keep |

### Wait metrics (`PERRY_LOOP_STATS=1`, `event_pump/loop_stats.rs`)

Turns and OS waits say how *often* the loop waited, not where the time went.
Instruction count and RSS can stay flat while the waits between Perry and tokio
decide a server's latency and CPU, so `PERRY_LOOP_STATS=1` also prints one
`[perry-loop-waits]` line at the process-exit funnel:

```
$ PORT=18231 PERRY_LOOP_STATS=1 ./server-turnloop   # two curl requests, then SIGTERM
[perry-loop] driver=turnloop turns=0 os_waits=0 zero_event_waits=0 native_ticks=3 turn_errors=0
[perry-loop-waits] arm=turnloop turnloop_waits=0 turnloop_wait_ns=0 turnloop_wait_max_ns=0
  tokio_ticks=3 tokio_tick_ns=60384792 tokio_tick_max_ns=27291625
  condvar_waits=0 condvar_wait_ns=0 condvar_wait_max_ns=0
  fast_drives=3 fast_drive_ns=5349167 fast_drive_max_ns=2577583
  zero_budget=0 throttle_sleeps=0
  wake_samples=3 wake_lt50us=3 wake_lt200us=0 wake_lt1ms=0 wake_lt5ms=0 wake_ge5ms=0 wake_max_ns=31375
```

(one line in reality; wrapped here. That run is the P0 server story in one
line: the server parked three times, every one of them in tokio — 60.4 ms
total, 27.3 ms in the longest — and never once in turnloop, because the
accept loop keeps a tokio task alive. The three wakes that ended those parks
took at most 31 µs.)

| Field group | What it measures |
|---|---|
| `turnloop_waits` / `_ns` / `_max_ns` | `Loop::turn(Timeout::Until(deadline))` — the pure turnloop wait |
| `tokio_ticks` / `_ns` / `_max_ns` | the P0-transitional registered tick (`run_one_tick`), taken whenever tokio owns in-flight native work — and the *only* wait the `tokio-wait-driver` arm has |
| `condvar_waits` / `_ns` / `_max_ns` | the legacy condvar park: runtime-only binaries, declined threads, a turn-failure fallback |
| `fast_drives` / `_ns` / `_max_ns` | the stdlib's brief tokio drive on the notified path, counted only when it actually drove |
| `zero_budget`, `throttle_sleeps` | zero-budget returns (a deadline read as due) and how many of them hit the #1114 throttle sleep |
| `wake_samples`, `wake_lt50us` … `wake_ge5ms`, `wake_max_ns` | wake latency: notify → the parked wait returning |

`arm=` names the build (`turnloop` / `tokio-wait-driver` / `legacy`), so a
measurement can prove which driver produced it. The A/B comparison is like with
like: **every counter is recorded in both arms**, through the same call sites.
The tokio tick is instrumented in `wait_driver_sleep`, which both arms reach —
the turnloop arm through `precise_wait`'s native-in-flight branch, the
`tokio-wait-driver` arm as its whole park.

**Wake latency.** The waiter clears the stamp slot, publishes which wait kind it
is parked in, and waits; a producer that sees a parked waiter stamps the
monotonic clock (earliest notify wins); the waiter takes the stamp when the wait
returns. There are exactly **two** wake producers and both stamp: almost
everything fans out through `js_notify_main_thread` — a cross-thread producer
(blocking pool, Worker, child-process reactor) and an in-thread native
completion alike (`perry_ffi::notify_main_thread` from ext-http/net/ws, and the
stdlib's own resolution sites) — and `js_native_work_submitted` wakes a parked
turn *directly*, bypassing it, which is why it exists at all. It stamps too; it
did not at first, and that omission was found by review and is now a test.

One notify into one parked wait is exactly one sample; a notify outside a wait
is none. Two windows are left open on purpose, both one-sided: a notify
published between the waiter's last `NOTIFIED` re-check and its parked-flag
store records no sample (the wait is still counted), and a stamp rejected for
belonging to an earlier wait costs a sample rather than inventing a
multi-millisecond one. The module can under-report a wake; it cannot invent or
inflate one.

**Cost and scope.** Diagnostic only. With the variable unset every hook is one
relaxed load of a lazily resolved state byte; nothing allocates and nothing
locks on any wait path. Recording is limited to the primary agent, so a worker's
legacy park cannot blur the comparison. The stats are process-global atomics, so
they survive the agent loop being destroyed at exit — which is why the line can
be printed after `shutdown_current_thread()`.

### A/B switch

`perry-stdlib/tokio-wait-driver` (default off) forwards to
`perry-runtime/tokio-wait-driver`. With it, the agent loop and precise park
are not compiled: every agent runs the pre-P0 `js_wait_for_event` body. The
body was refactored into `zero_budget_return`/`condvar_park` helpers but is
otherwise unchanged. The stdlib registers no in-flight predicate, and readline
keeps its ceiling. The runtime half exists so a runtime-only binary also
measures the legacy arm. `PERRY_LOOP_STATS=1` prints
`[perry-loop] driver=tokio-wait-driver` in that arm, so a run can prove which
arm it measured.

In **both** arms: the O(1) keep-alive counters, the `InflightGuard` RAII
change and the fractional-ms provider contract in the runtime. The switch
covers the driver, not the keep-alive work.

**Caveat:** the auto-optimize path rebuilds the archives with a computed
feature list that does not include `tokio-wait-driver`. The B arm is only
valid with prebuilt archives (`PERRY_SKIP_BUILD=1` / `PERRY_NO_AUTO_OPTIMIZE=1`).
Check the stats marker line in every run.

### O(1) keep-alive

Inventory basis: every predicate the generated loop evaluates per turn. With a
pending ref'd `setTimeout`, the `js_stdlib_has_active_handles` chain ran twice
per turn and the three timer queue scans ran 8, 8 and 2 times; each callback
and interval scan also took the ref-state registry lock once per entry.

| Predicate (per turn) | Before | After |
|---|---|---|
| `js_timer_has_pending` / `js_callback_timer_has_pending` / `js_interval_timer_has_pending`, `should_run_unref_*` | queue scan under lock, plus a per-entry registry lock | primary agent: one atomic per queue (`timer/liveness.rs`, `TimerQueue`); other agents: exact scan |
| `js_native_async_has_active` | GC root-registry lock | length mirror |
| `js_aux_has_active` | lock and `Vec` clone | registry-length gate (exact when 0) |
| `stdin_listeners_keep_loop_alive` (runtime and stdlib chains) | up to 7 mutexes | armed latch (exact until the first listener) |
| `js_process_ipc_has_active` | 2 mutexes | probe/available atomics (exact when there is no channel) |
| `js_thread_has_pending` (microtask liveness) | lock and scan | length gate |
| `diagnostics_channel_has_pending_publishes` | lock (and lazy init) | length mirror |
| stdlib pending resolutions / deferred | 2 mutexes | length mirrors |
| `js_tls_has_active_handles` | 2 map scans | exact count (`tls/liveness.rs`) |
| `js_worker_threads_has_pending` live-worker part | map scan | exact count |

Each counter changes only under the lock of the state it mirrors, at every
insert, removal and state transition. Underflow is a `debug_assert!`. In debug
builds the timer, native-async, TLS and worker counters re-derive their value
on every read and assert equality. The full `perry-runtime` debug suite
(3962 tests) ran with those assertions on.

Deliberate semantic note: callback/interval entries now cache their ref state
(`refed`). The `hasRef()` registry is bounded to 65 536 ids and an evicted id
used to read as ref'd again, so a still-queued unref'd timer could start
keeping the loop alive after 65 536 newer timers. The cached flag keeps the
timer unref'd, which is what Node does. `hasRef()` itself still reads the
registry.

**Still a scan or locks (not converted in P0):**
- readline (8 locks per call once the stdlib pump is registered);
- ws/net/crypto/zlib (lock-only emptiness checks);
- `MessagePort`/`BroadcastChannel` liveness (scans that read JS `onmessage`,
  and are only non-trivial in programs that create channels);
- `fs.watch` watcher scans (armed slot);
- node-api threadsafe functions;
- the bundled cron queue;
- perry-ext-* has-active callbacks (fastify, http, net, ws), now behind the
  length gate.

These subsystems move onto loop handles in P1/P2/P5 (net, ws, TLS, stdin,
child, fs-watch) or P3/P4 (channels, workers, cron), where `Loop::alive()`
replaces them. The P0 per-turn *deadline* computation still scans the timer
queues (not keep-alive; P3 moves timers into the turnloop heap).

## Dependency

- `turnloop = "=0.1.0-alpha.2"` (workspace), perry-runtime for
  `cfg(not(target_arch = "wasm32"))`.
- Published 2026-09-15T01:10:00Z. `Cargo.lock` checksum
  `21053fd229e6437ba256b97b2e491dbb5e777ae14f8e585199d17dd9b46b6493`, equal
  to the crates.io index entry and to `sha256` of the downloaded
  `turnloop-0.1.0-alpha.2.crate`.
- Resolved once with `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo
  metadata`. Every later build and check used `--locked`. `.cargo/config.toml`
  is unchanged.
- **Owner decision needed: forced downgrades.** turnloop pins its own
  dependencies with `=` (`libc =0.2.175`, `js-sys =0.3.85`,
  `wasm-bindgen =0.2.108`, `windows-sys =0.61.2`, `wasip2 =1.0.3`,
  `loom =0.7.2`). Cargo keeps one copy per semver-compatible range, so the
  resolver downgraded the workspace (cargo's own log):
  - libc 0.2.189→0.2.175
  - tokio 1.53.1→1.50.0, tokio-macros 2.7.0→2.6.1, mio 1.2.1→1.1.0
  - redis 1.6.0→1.2.4 (redis 1.2.4 raises a future-incompatibility warning)
  - rustix 1.1.4→1.1.2, linux-raw-sys 0.12.1→0.11.0, tempfile 3.27.0→3.23.0
  - js-sys and web-sys 0.3.99→0.3.85
  - wasm-bindgen family 0.2.122→0.2.108, wasm-bindgen-futures 0.4.72→0.4.58
  - added: generator 0.8.9, loom 0.7.2
- `cargo audit` (same ignores as `security-audit.yml`) reports one
  vulnerability: RUSTSEC-2026-0285 in rustls 0.23.43. That version is identical
  on base `1cd160f3d1`; the advisory is dated 2026-09-14 and is not introduced
  by this change. `cargo deny` is not installed locally: UNRUN.

## Verification

All commands used `CARGO_TARGET_DIR=<worktree>/target` and `CARGO_BUILD_JOBS=6`
unless stated otherwise.

| Command | Result |
|---|---|
| `cargo check --locked --tests -p perry-runtime -p perry-stdlib` | PASS (only pre-existing warnings in files not touched) |
| `cargo check --locked --tests -p perry-runtime -p perry-stdlib --features perry-stdlib/tokio-wait-driver` | PASS |
| `cargo build --locked --profile perry-dev -p perry -p perry-runtime-static -p perry-stdlib-static` | PASS (first attempt: rustc received SIGTERM from outside, retried). `nm` confirms `js_register_native_inflight`/`js_native_work_submitted` in the new `libperry_runtime.a` |
| Same build with `--features perry-stdlib/tokio-wait-driver` into a separate target dir | PASS (archive has no `agent_loop` symbols) |
| `RUST_TEST_THREADS=1 <perry-runtime debug lib test binary>` (all) | PASS — 3962 passed, 0 failed, 4 ignored |
| …filters `event_pump::`, `timer::`, `agent_dispatch`, `native_async`, `thread::`, `diagnostics`, `stdlib_pump`, `gc::tests::idle_reclaim`, `global_sink_isolation` | PASS (13/13/5/12/8/…/10/14/11) |
| `RUST_TEST_THREADS=1 <perry-stdlib debug lib test binary>` (all), 6 runs (one of them skipping the new in-flight test) | 2 runs PASS (141 passed); 4 runs aborted in `readline::mod_tests::listeners_provider_roots_readable_snapshot_across_array_allocation` on a debug-only `gc/young_log.rs:173` assertion (`closure.dynamic_props`). **Pre-existing flake:** the same suite built from base `1cd160f3d1` (exported source, same target dir) aborted with the same assertion in 2 of 3 runs and passed (139 tests) in 1. Subset bisection is non-deterministic (a subset that aborted once passed 6 of 6 reruns) |
| same, `--skip listeners_provider_roots_readable_snapshot_across_array_allocation` | PASS — 140 passed |
| …filters `common::async_bridge` (5), `tls::` (2), `readline` (26), `cron` (1) | PASS |
| `cargo fmt --all -- --check`, `scripts/check_file_size.sh` | PASS |
| `python3 scripts/gc_runtime_root_holders.py` | PASS (new `AGENT_LOOP` verdict; `PASS1_MARKED` window re-audited and re-pinned for the `gc/mod.rs` exit-funnel call) |
| `BASE_SHA=1cd160f3d1 SKIP_COMPILE_GATES=1 scripts/run_lint_gates.sh` | see "Lint gates" below |
| `python3 scripts/turnloop_p0_loop_stats.py` (turnloop arm) | PASS, 7/7 probes (table below) |
| `python3 scripts/turnloop_p0_loop_stats.py --perry <A/B target>/perry-dev/perry --arm tokio-wait-driver` | PASS, 7/7 (stdout equals Node; legacy marker present) |
| `python3 scripts/turnloop_p0_native_probe.py` | fetch PASS (`native_ticks=1`, then `turns=1` for the timer after the fetch; server saw both requests). websocket UNRUN: global `WebSocket` routes to the perry-ext-ws archive, which `PERRY_NO_AUTO_OPTIMIZE=1` refuses to link against a stdlib built separately (tokio identity check) |
| `PERRY_SKIP_BUILD=1 PERRY_BIN=target/perry-dev/perry ./run_parity_tests.sh --filter <name>` for 30 gap tests (timers, promises, async, fetch, child_process, stdin, fs.watch, worker channels, readline, tick order, and the new `test_gap_turnloop_p0_timers`) | 29 PASS, 1 FAIL: `test_gap_9592_child_timeout_threads`. Node itself throws `spawn /bin/true ENOENT` on macOS, and the Perry output is identical on the turnloop and `tokio-wait-driver` arms (3 runs each). This is a host issue, not a P0 regression |
| GC schedule stress: `PERRY_GC_SCHEDULE_SEED=1..5 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0` on `test_gap_turnloop_p0_timers` | PASS (stdout equals Node; every seed ran collections, e.g. seed 5 `copying_minors=2 moved_objects=12764`) |
| `cargo check --target x86_64-pc-windows-msvc -p perry-runtime` | see "Windows" below |
| gap test ext-routed (`net`, `http`, `ws`) and full suites | UNRUN (auto-optimize rebuilds; integrator) |

**Wait metrics and harness (this lane).** All on macOS arm64, worktree target
dir, `CARGO_BUILD_JOBS=6`.

| Command | Result |
|---|---|
| `cargo check --locked --tests -p perry-runtime -p perry-stdlib` | PASS |
| …`--features perry-stdlib/tokio-wait-driver` | PASS (the wait-metric module and its tests compile in both arms) |
| `RUST_TEST_THREADS=1 cargo test --locked -p perry-runtime --lib -- event_pump:: --test-threads=1` | PASS — 21/21 (turnloop arm) |
| `RUST_TEST_THREADS=1 cargo test --locked -p perry-stdlib --lib -- common::async_bridge --test-threads=1` | PASS — 6/6 (turnloop arm) |
| same two, `--features perry-stdlib/tokio-wait-driver` | PASS — 13/13 and 6/6 (the turnloop-only `agent_loop` tests are not compiled in that arm) |
| `cargo fmt --all -- --check`, `scripts/check_file_size.sh` | PASS |
| `python3 scripts/gc_runtime_root_holders.py` | PASS (the new statics are integer atomics, so no new holder verdict is owed) |
| `python3 scripts/turnloop/server_ab.py all --dry-run --work /tmp/turnloop-ab-dry` | PASS (plan printed; the summary/markdown path is driven over generated samples and self-checked) |
| `RUST_TEST_THREADS=1 cargo test --locked -p perry-runtime --lib -- --test-threads=1` (whole suite) | PASS — 3970 passed, 0 failed, 4 ignored, in 175.85 s |
| `python3 scripts/turnloop_p0_loop_stats.py --perry /tmp/tlab/target-turnloop/perry` | PASS, 7/7 (the P0 probes are unaffected by the new line) |
| `server_ab.py build --work /tmp/tlab --skip-cargo` then `run … --load-tool ab` | PASS — both arms verified, arms-differ gate satisfied, 12/12 samples valid |

**Hardware-counter collection (`root@84.32.71.237`, `perrybuilder`, Linux 6.17
x86_64, 64 cores, `perf_event_paranoid=-1`).** Counter collection only — no
benchmarking was done there, and the box sat at loadavg 4.5–10 throughout, which
is exactly why the harness refuses to call its timing authoritative.

| Check | Result |
|---|---|
| `PerfStat.resolve("auto")` | `perf ok (7/7 events, perf_event_paranoid=-1)` — instructions, cycles, task-clock, context-switches, cpu-migrations, page-faults, `raw_syscalls:sys_enter` |
| real attach to a busy-loop process, 2 s | 26 930 731 857 retired instructions, 7 588 745 567 cycles, IPC 3.549, task-clock 1999.1 ms, CPU utilisation 1.00 |
| `perf_probe` with a deliberately bogus event | the bogus event is NAMED as dropped (`unknown or unsupported event on this kernel/PMU`); the other 7 still usable — a denied counter is reported, never silently omitted |
| `--perf off` / `--perf strace` | each returns its own explicit status string, not a bare "off" |
| `timing_verdict` on that host | `timing_authoritative=False`, reasons: shared build box by hostname, **and** loadavg above `--max-loadavg` |
| `measure_load` end to end (stub server honouring the same stderr/SIGTERM contract, stub `ab`) | PASS — sample valid, marker and `arm=` matched, real counters: 233 617 488 instructions, 419 678 268 cycles, IPC 0.557, 9 821 syscalls, and every per-request figure derived (714 427 instructions/req, 30.0 syscalls/req, 5.05 ctx switches/req, 1.02 page faults/req) |

That last row used a stub server rather than a compiled Perry, because building
Perry on that shared box is not something to do for a plumbing check: it
exercises the whole `measure_load` path — perf attach, CSV parse, unit
normalisation, derivation, timing verdict, marker validation — with genuine
hardware counters.

**Caught by that run:** `perf stat -x,` leaves the unit field EMPTY on mainline
perf and reports `task-clock` in **nanoseconds**. Reading it as milliseconds
made CPU utilisation come out as 998 998 instead of 1.00. The parser now honours
the unit field explicitly and treats a unitless `task-clock` as nanoseconds, and
flags any event perf multiplexed below 99 % as an estimate.

**Host-role detection**, checked on both machines of record: `perrybuilder` →
role `shared` (timing never authoritative); `perry-macos` → role `quiet`
(authoritative while quiet — it idles at loadavg ≈ 1.45, which is why
`--max-loadavg` defaults to 2.0).

Which test covers which counter:

| Counter | Test |
|---|---|
| `tokio_ticks` (+ `_ns`), and that a tick is **not** miscounted as a turn | `agent_loop::tests::native_work_in_flight_is_counted_as_a_tokio_tick_not_a_turn` (registered predicate + tick); `async_bridge::tests::a_live_tokio_task_parks_the_main_loop_in_a_counted_tokio_tick` (the real shared tokio runtime, asserting the task ran and the park lasted ≥ 15 ms) |
| `turnloop_waits` | `agent_loop::tests::another_thread_wakes_a_parked_turn_through_js_notify_main_thread` (exactly one turn, one wake sample) |
| `condvar_waits` (+ `_ns`, `_max_ns`) | `loop_stats::tests::one_cross_thread_notify_into_a_condvar_park_is_one_wake_sample`, `…a_timed_out_wait_and_an_unparked_notify_add_no_wake_sample` |
| `wake_samples`, exactly one per notify | the three tests above, one per wait kind |
| a cross-thread **native submission** wake (`js_native_work_submitted`, which does not go through `js_notify_main_thread`) | `agent_loop::tests::a_cross_thread_native_submission_wakes_a_turn_and_is_one_wake_sample` |
| no sample for a timeout, or a notify outside a wait | `…a_timed_out_wait_and_an_unparked_notify_add_no_wake_sample` |
| bucket edges 50 µs / 200 µs / 1 ms / 5 ms | `loop_stats::tests::wake_latency_buckets_split_at_50us_200us_1ms_5ms` |
| `fast_drives` (+ `_ns`), `zero_budget`, `throttle_sleeps` | `loop_stats::tests::fast_drives_and_zero_budget_returns_are_counted`; `…js_wait_for_event_zero_budget_path_is_counted` drives the real entry point |
| worker agents are not recorded; the line names every field | `loop_stats::tests::workers_are_not_recorded_and_the_line_names_every_metric` |


### Lint gates

`BASE_SHA=1cd160f3d1 SKIP_COMPILE_GATES=1 scripts/run_lint_gates.sh`: 76 of 77
script-tier gates PASS; 2 CI-only gates skipped; the compile tier was not run
(UNRUN; covered by the `cargo check`/build rows above, not by clippy or the
API-docs drift gate). One FAIL:
`benchmarks/ci_public_baseline_check.py` ("public artifact benchmark inputs
changed"). It is pre-existing: its fingerprinted inputs are the `[profile*]`
tables of `Cargo.toml` plus files under `benchmarks/`, and both are
byte-identical to base `1cd160f3d1` (checked with the script's own
`_cargo_profile_tables` normalization; `git diff 1cd160f3d1 -- benchmarks` is
empty). The first gate run was killed from outside (exit 144) and rerun.

### Windows and other targets

- `cargo check --locked --target x86_64-pc-windows-msvc -p perry-runtime`:
  FAIL locally before reaching Perry code. The C build scripts of `psm`,
  `stacker`, `libmimalloc-sys` and `zstd-sys` need Windows SDK headers
  (`windows.h`, `wchar.h`) that this macOS host does not have. This is
  environmental; the PR's Windows CI arm is the real check.
- `cargo check --locked --target x86_64-pc-windows-msvc -p turnloop` (the
  IOCP backend P0 uses): PASS.
- `cargo check --locked --target x86_64-unknown-linux-gnu -p turnloop`
  (epoll) and `--target aarch64-linux-android -p turnloop`: PASS.
- perry-runtime itself was not checked for Linux locally (same C sysroot
  problem).

### Sabotage check (the tests can fail)

Two throwaway builds of the perry-runtime debug test binary, reverted
afterwards (`git diff` empty):

1. `Timeout::Until(deadline)` → `Timeout::Now` in `park_until` (a spin), plus
   `clearImmediate` using a plain `retain` (an unpaired counter). Results:
   - `sub_and_whole_millisecond_deadlines_wait_without_spinning` FAILED
     ("500 us: 16 turns");
   - `js_wait_for_event_reaches_a_timer_deadline_in_at_most_two_turns` FAILED;
   - `another_thread_wakes_…` FAILED ("owner never parked");
   - the timer-liveness test run aborted on the debug consistency assertion
     (`timer/liveness.rs:153`, "timer keep-alive count drifted"). The assertion
     fires inside an `extern "C"` predicate, so it aborts the test process
     rather than failing a single test.
2. Only `agent_loop::wake_primary()` removed from `js_notify_main_thread`:
   `another_thread_wakes_a_parked_turn_through_js_notify_main_thread` FAILED
   ("wake was lost: waited 30.00s").

Two more for the wait metrics, each reverted (`git diff` empty afterwards):

3. `loop_stats::note_notify()` removed from `js_notify_main_thread`: the three
   wake-sample tests FAILED, each "left: 0, right: 1" —
   `another_thread_wakes_a_parked_turn_through_js_notify_main_thread`,
   `one_cross_thread_notify_into_a_condvar_park_is_one_wake_sample`,
   `one_notify_into_a_registered_tick_is_one_wake_sample`. The other 12 passed,
   so the failure is specific to the removed hook.
5. `loop_stats::note_notify()` removed from `js_native_work_submitted`:
   `a_cross_thread_native_submission_wakes_a_turn_and_is_one_wake_sample` FAILED
   alone (0 vs 1); the other seven in the filter passed.
4. The `begin_wait`/`end_wait` pair removed from `wait_driver_sleep` (the tick is
   still driven, just not measured):
   `native_work_in_flight_is_counted_as_a_tokio_tick_not_a_turn` FAILED ("the
   tick was not counted as a tokio tick", left 0 right 1) and
   `one_notify_into_a_registered_tick_is_one_wake_sample` FAILED. The
   condvar and turnloop wake tests still passed, so the arms are measured
   independently.

### Measured loop counters (turnloop arm, macOS, `PERRY_LOOP_STATS=1`)

| Probe | turns | os_waits | zero_event_waits | native_ticks |
|---|---|---|---|---|
| `setTimeout(…, 0.5)` | 0 | 0 | 0 | 0 |
| `setTimeout(…, 2)` | 1 | 1 | 1 | 0 |
| `setTimeout(…, 10)` | 1 | 1 | 1 | 0 |
| 2 ms timer, first park with ~0.4 ms left | 1 (9 of 10 runs; 0 when scheduler delay made it due) | 1 | 1 | 0 |
| `setInterval(…, 10)` × 3 | 3 | 3 | 3 | 0 |
| 10 000 awaited promises, then a 10 ms timer | 1 | 1 | 1 | 0 |
| idle 200 ms timer | 1 | 1 | 1 | 0 |
| fetch against a local server, then a 20 ms timer | 1 | 1 | 1 | 1 |
| `test_gap_9592` (50 child spawns and a timed kill) | 4 | 4 | 2 | 0 |

- Perry treats a 0.5 ms delay as due at once, so no park happens. That is
  pre-existing delay normalization; Node clamps to 1 ms.
- Every quiet deadline is exactly one OS wait, and the timeout is its one
  zero-event wait. The Rust test with an idle registered socket asserts
  ≤ 2 turns and ≤ 1 zero-event wait for 500 µs, 2 ms and 10 ms, and that each
  wait ended at or after its deadline.

**Sub-millisecond remainder, both arms**
(`PERRY_MT_PROFILE=1`, `event_wait` counters, 10 runs each):

| Arm | Runs that reached a park | Zero-budget returns |
|---|---|---|
| turnloop | 9 | 0 (`total:1`, one precise turn) |
| legacy (`tokio-wait-driver`) | 4 | 254–305 per run, timer fired at 2.03 ms. The other runs found the timer already due |

Wall times printed by the stats script include macOS first-exec validation of
a freshly linked binary (~0.3–1.5 s). Re-running the same binary: idle probe
0.21 s on both arms.

### Measured wait metrics (macOS, `--profile perry-dev`)

Both arms built from `fa5bb1b40a` in the worktree target dir with the harness's
package and feature set, then copied out to `/tmp/tlab/target-{turnloop,tokio}`
(one target tree, two archive directories — the `--skip-cargo` shape). The
`node:http` app was compiled by each arm's own compiler with
`PERRY_RUNTIME_DIR` pointing at its own archives and `PERRY_NO_AUTO_OPTIMIZE=1`.

`scripts/turnloop_p0_loop_stats.py --perry /tmp/tlab/target-turnloop/perry`:
PASS, 7/7 probes, unchanged by the wait metrics (the new line is
`[perry-loop-waits]`, which its `[perry-loop] ` regex does not match).

A **timer-only** program (`test_turnloop_p0_idle.ts`, one 200 ms timeout) —
the shape P0 is actually about:

```
[perry-loop] driver=turnloop turns=1 os_waits=1 zero_event_waits=1 native_ticks=0 turn_errors=0
[perry-loop-waits] arm=turnloop turnloop_waits=1 turnloop_wait_ns=200737250 turnloop_wait_max_ns=200737250 …
  … tokio_ticks=0 … fast_drives=0 … zero_budget=0 throttle_sleeps=0 wake_samples=0 … wake_max_ns=0
```

One turnloop wait of 200.74 ms for a 200 ms deadline (0.74 ms of overshoot,
scheduler included), no tokio tick, no fast drive, no zero-budget return, and
no wake sample — the wait ended on its own deadline, not on a notify. That is
the whole claim of the P0 park, now a measurement rather than an inference.

A **server** (the harness's `node:http` app, two `curl` requests, then
`SIGTERM`) is the opposite shape, and the metrics say so plainly — see the
`[perry-loop-waits]` example under *Wait metrics* above: three parks, **all
three in tokio** (60.4 ms total, 27.3 ms in the longest), zero turnloop turns,
three fast drives (5.3 ms), and three wakes all under 50 µs (max 31.4 µs).
A P0 server never reaches the turnloop park, because its accept loop keeps a
tokio task alive for the life of the process; the gain for servers arrives with
P1/P5, and this line is how that will be shown rather than argued.

### Harness smoke run (macOS — a shakedown, NOT a measurement)

`scripts/turnloop/server_ab.py` was run end to end locally so the integrator
inherits a harness that has actually executed, not one that only parses. Treat
the numbers as evidence the plumbing works and nothing else: the host is a
shared laptop at load average 30–60, the build is `--profile perry-dev`
(opt-level 1), the load tool is `ab` (millisecond latency resolution, no p999,
single-threaded), `/proc` is absent so the per-window CPU and context-switch
columns are empty, `perf` is unavailable so there is no syscall rate, and there
were 2 rounds instead of 5. Nothing here attributes a difference to either arm.

```bash
# arms prepared into one target tree and copied out (the --skip-cargo shape)
cargo build --locked --profile perry-dev -p perry -p perry-runtime-static \
  -p perry-stdlib-static -p perry-ext-http -p perry-ext-net -p perry-ext-ws \
  --features perry-stdlib/external-http-server-pump,perry-stdlib/external-http-client-pump
cp target/perry-dev/{perry,libperry_runtime.a,libperry_stdlib.a,libperry_ext_http.a,libperry_ext_net.a,libperry_ext_ws.a} /tmp/tlab/target-turnloop/
# …same again with ,perry-stdlib/tokio-wait-driver → /tmp/tlab/target-tokio/

python3 scripts/turnloop/server_ab.py build --work /tmp/tlab --skip-cargo
python3 scripts/turnloop/server_ab.py run --work /tmp/tlab --rounds 2 \
  --concurrency 16 --duration 4 --warmup 1 --idle 2000 --idle-hold 3 --load-tool ab
```

What the run proved about the harness itself:

- both arms verified before any measurement —
  `verified turnloop: [perry-loop] driver=turnloop turns=0 os_waits=0 zero_event_waits=0 native_ticks=1 turn_errors=0`
  and `verified tokio: [perry-loop] driver=tokio-wait-driver`;
- `arms differ: runtime, stdlib and the linked server are distinct builds`
  (`libperry_ext_http.a` is deliberately identical — it links perry-ffi, not the
  feature);
- 12 of 12 samples valid across the first (3-scenario, 2-round) run; the
  markdown and JSON reports were produced from `results.json`;
- the server exits through `SIGTERM` → `process.exit(0)` → the exit funnel, so
  every sample carries a full `[perry-loop-waits]` line.

And what it says about P0 on a server, which is the substantive part:

| | turnloop arm | `tokio-wait-driver` arm |
|---|---|---|
| turnloop turns (load, c=16) | **0** | 0 |
| tokio ticks | 164 | 202 |
| time in tokio ticks | 292 ms | 372 ms |
| fast drives | 15 552 | 16 786 |
| zero-budget returns / throttle sleeps | 0 / 0 | 0 / 0 |
| idle 2 000 keep-alive conns: opened / surviving a 3 s hold | 2 000 / 2 000 | 2 000 / 2 000 |
| RSS per idle connection | 30 880 B | 30 872 B |

**The turnloop arm makes zero turnloop turns on a server.** That is not a
regression, it is the P0 design stated in *Transitional coexistence* — the
accept loop keeps a tokio task alive for the life of the process, so
`native_inflight()` is permanently true and every park goes to the tokio tick.
Until P0 the only way to say that was to read the code; now the line says it.
It also fixes what a server A/B can mean before P1/P5: the two arms are running
**the same wait**, so any difference between them is noise or link layout, not
driver choice. The wait metrics are what will show P1 landing — turns rising off
zero and tick time falling.

Per-connection memory is the other number worth carrying forward: ~30.9 KB of
RSS per idle keep-alive connection, identical in both arms (it is
perry-ext-http's per-socket cost, which P0 does not touch). At the brief's 100k
target that is ~3 GB, which is the figure P1/P5 has to move.

## Commands for the integrator

Build both arms from the same commit, in separate target dirs, with the same
package set:

```bash
# arm A (turnloop, default)
CARGO_TARGET_DIR=$PWD/target-a cargo build --locked --release \
  -p perry -p perry-runtime-static -p perry-stdlib-static
# arm B (pre-P0 driver)
CARGO_TARGET_DIR=$PWD/target-b cargo build --locked --release \
  -p perry -p perry-runtime-static -p perry-stdlib-static \
  --features perry-stdlib/tokio-wait-driver
```

**Fast gap suite, per arm.** The auto-optimize tier cannot run arm B, because
it drops the feature.

```bash
PERRY_SKIP_BUILD=1 PERRY_BIN=$PWD/target-a/release/perry ./run_parity_tests.sh --filter test_gap_
PERRY_SKIP_BUILD=1 PERRY_BIN=$PWD/target-b/release/perry ./run_parity_tests.sh --filter test_gap_
```

**Full auto-optimize tier (arm A).** `./scripts/run_gap_tests.sh` or the
documented CI dispatch.

**Server A/B (`scripts/turnloop/server_ab.py`, Linux x86_64).**
This is the measurement the wait metrics exist for. One command does everything:

```bash
# oha first (the harness prints this if it is missing):
cargo install oha --locked        # or: apt install oha

scripts/turnloop/server_ab.py all --work /root/turnloop-ab --jobs "$(nproc)"
```

`all` = `build` then `run`. Split them when the build and the measurement should
not share a window:

```bash
scripts/turnloop/server_ab.py build --work /root/turnloop-ab [--profile release] [--jobs N]
scripts/turnloop/server_ab.py run   --work /root/turnloop-ab \
    [--rounds 5] [--concurrency 1,64,1024] [--duration 15] [--warmup 3] \
    [--idle 10000,100000] [--idle-hold 10] \
    [--load-tool auto|oha|wrk|ab] [--oha PATH] [--wrk PATH] \
    [--perf auto|perf|strace|off] [--max-loadavg 2.0] [--shared-host]
scripts/turnloop/server_ab.py callgrind --work /root/turnloop-ab    # separate Ir arm, Linux + valgrind
scripts/turnloop/server_ab.py report --work /root/turnloop-ab      # re-render, folding in callgrind.json
scripts/turnloop/server_ab.py all --dry-run                        # plan + reporting self-check, macOS-safe
```

The two machines of record, in the order they are meant to be run:

```bash
# 1. counters, on the shared Linux box. Timing is auto-marked advisory there.
ssh root@84.32.71.237
scripts/turnloop/server_ab.py all --work /root/turnloop-ab --jobs "$(nproc)"
apt install -y valgrind && scripts/turnloop/server_ab.py callgrind --work /root/turnloop-ab
scripts/turnloop/server_ab.py report --work /root/turnloop-ab   # counters + the separate Ir section

# 2. timing, on the quiet mini. perf does not exist there; the perf rows say so.
ssh perry@perry-macos.local
scripts/turnloop/server_ab.py all --work ~/turnloop-ab --max-loadavg 2.0
```

What `build` does, and why each part is there:

- one `cargo build` per arm into `<work>/target-{turnloop,tokio}`, same commit,
  same package set (`perry`, the two `-static` wrappers, ext-http/net/ws) and
  the same `external-http-{server,client}-pump` features that
  `run_parity_tests.sh` uses for no-auto-optimize http. Only the
  `perry-stdlib/tokio-wait-driver` feature differs;
- it records every archive's **mtime**, size and SHA-256, and warns when one
  predates HEAD's commit time — the stale-`.a` failure mode from CLAUDE.md,
  where both arms would behave identically and report a vacuous "no difference";
- it compiles the same app with each arm's compiler under
  `PERRY_NO_AUTO_OPTIMIZE=1` and `PERRY_RUNTIME_DIR=<arm out dir>` (auto-optimize
  drops the A/B feature — arm B is only valid with prebuilt archives);
- it then *runs* each server once and refuses to continue unless the
  `[perry-loop] driver=…` marker and the `arm=` field of the
  `[perry-loop-waits]` line match the arm it just built.

`--skip-cargo` takes arms that are already built: build each arm in turn into one
target tree, copy `perry` and the five archives into `<work>/target-turnloop` and
`<work>/target-tokio`, and `build` records and verifies them without invoking
cargo. That is for a host with room for only one cargo target tree.

What `run` collects, per sample (one fresh server process each):

| Group | Metrics |
|---|---|
| load (`oha`, else `wrk`; `ab` only on request) | throughput, p50/p99/p999, success rate |
| CPU and scheduling | user/sys for the measured window *and* the process lifetime, wall, voluntary and involuntary context switches, threads, CPU µs per request |
| syscalls | `perf stat -e raw_syscalls:sys_enter -p <pid>` over the measured window; else `strace -c -f` in a **separate** server process (perturbing, and labelled as such in the output) |
| memory and size | peak RSS (`rusage`), RSS before/after the idle connections, bytes per idle connection, server binary size |
| idle capacity | 10 000 and 100 000 keep-alive connections: how many opened, how many survive the hold, time to open, CPU and context switches during the hold |
| waits | the whole `[perry-loop-waits]` line — tokio ticks vs turnloop turns, time and max per kind, fast drives, the wake-latency histogram, zero-budget and throttle hits |

Arms alternate order every round (`rounds` defaults to 5). A sample whose marker
or `arm=` does not match the arm it was meant to measure — or whose server
needed `SIGKILL`, or whose load tool errored — is marked invalid, excluded from
the medians and reported by reason, rather than averaged in. Output:
`<work>/results/results.json` (every raw sample), `summary.json` and
`summary.md` (median [min–max] per arm, plus the delta of medians).

### Two instruction counts, and what each one is not

**`perf stat` `instructions` = instructions RETIRED** on the real CPU during the
measured window, with cache misses, branch mispredictions, SMT contention and
interrupts all included. It is a statement about **cost under load**, it moves
with concurrency, and it is what the load table reports (with `cycles`, `IPC`,
`task-clock`, context switches, CPU migrations, page faults and the syscall
tracepoint alongside it).

**Callgrind `Ir` = instructions EXECUTED** under Valgrind's serialising
simulator: no cache model, no branch predictor, one thread at a time. It is
deterministic and **load-independent by construction** — which is exactly what
makes it a good exact A/B of one code path, and no statement at all about cost
under load. A figure like "2.5k instructions per turn" is an `Ir` figure; it
cannot answer "what does this cost a server at c=1024". The two quantities are
never added, never compared, and never substituted for one another; the harness
keeps them in separate sections, each carrying this caveat.

**Per-request normalisation.** Totals alone let a throughput win hide a
per-request regression, so the table reports `instructions / request`,
`cycles / request`, `syscalls / request`, `context switches / request` and
`page faults / request` next to the totals.

**Callgrind and the server workload.** Not run, deliberately. Valgrind costs
roughly 50–100×, so the load generator's connections time out and the loop's
time moves almost entirely into waits that scale with wall-clock rather than
with request handling; the resulting `Ir` would describe an artificial wait
pattern, not the server. The `callgrind` subcommand therefore covers the
timer/promise **microbenchmarks** (`test-files/test_turnloop_p0_*.ts`), where
the measured code path is the park itself and the run is short enough to
simulate honestly.

```bash
scripts/turnloop/server_ab.py callgrind --work DIR [--probes stem,stem] [--valgrind PATH]
```

### Hosts of record, and when timing is only advisory

The harness prints the host it ran on and decides, **per sample**, whether the
timing may be quoted:

| | counters (`perf`, syscalls, page faults) | timing (throughput, latency, wall) |
|---|---|---|
| Linux build box (shared, e.g. `perrybuilder`) | valid — they are per-process | **advisory**, automatically |
| quiet timing host (the Mac mini) | not available (perf is Linux-only) | authoritative while loadavg ≤ `--max-loadavg` |

A host whose name looks like a build box, or `--shared-host`, or a 1-minute
loadavg above `--max-loadavg` (default 2.0) at the start of a window, marks that
sample's timing advisory. The markdown then carries a blockquote naming the
host, its role and every reason, the timing section is headed `— ADVISORY`, and
`run` prints a closing `VERDICT:` line. Counters are never downgraded for
this — they are per-process and survive a busy box. Measure your timing host at
rest and set `--max-loadavg` just above that: the mini idles near 1.45.

**When `perf` cannot run, the column is not dropped.** Every `perf` row is
still rendered, as `n/a` with the reason in the delta cell (`perf is Linux-only;
this host is darwin`, `permission denied (perf_event_paranoid=2)`, `unknown or
unsupported event on this kernel/PMU`, …), and the reason is repeated under the
table and in the header. `--perf auto` falls back to `strace -c -f` for syscall
counts only (in a separate, perturbed process, and labelled as such); `--perf
perf` refuses to fall back; `--perf off` disables it and says so. Each event is
probed individually against `true` first, because one denied event in a single
`perf stat` takes every other counter down with it.

Host preparation for the 100k idle test (the harness warns and records the
limits it found): `ulimit -n 1048576`, `fs.nr_open`,
`net.ipv4.ip_local_port_range` — the client spreads connections over
`127.0.0.1…127.0.0.N` (one source per 25 000) to get past the ephemeral-port
ceiling. `perf` needs `kernel.perf_event_paranoid <= 1`.

The subject is `scripts/turnloop/apps/node_http_hello.ts` (a `node:http`
server), not an existing fastify/hono app: those need auto-optimize or
`compilePackages`, and arm B does not survive auto-optimize. It sets
`keepAliveTimeout = 0` so idle sockets are not reaped during the capacity test,
and exits through `process.exit` on `SIGTERM` so the exit funnel prints the
stats lines the harness reads.


**Loop statistics and bridge probes, per arm.**

```bash
python3 scripts/turnloop_p0_loop_stats.py --perry target-a/release/perry
python3 scripts/turnloop_p0_loop_stats.py --perry target-b/release/perry --arm tokio-wait-driver
python3 scripts/turnloop_p0_native_probe.py --perry target-a/release/perry
```

**GC stress.** Assert that collections ran while native I/O was pending:
`copying_minors > 0` in the `[gc-schedule] done:` line of a fetch probe.

```bash
for seed in $(seq 1 50); do
  PERRY_GC_SCHEDULE_SEED=$seed PERRY_GC_PROTECT_FROMSPACE=1 PERRY_LOOP_STATS=1 ./probe
done
```

**Instruction/wall/RSS/size A/B at cgu=1 (Linux).**
- Build both arms with the release profile (cgu=1).
- Compile the same probes and benchmarks with each arm's compiler under
  `PERRY_NO_AUTO_OPTIMIZE=1`.
- Run interleaved fresh-process rounds under
  `perf stat -e instructions:u,instructions:k`.
- Require the `[perry-loop]` marker line in every run.
- Use the timer-only and promise-churn probes as the subject and a pure-CPU
  program with no timers as the control.

**Windows.** The PR's Windows CI arm. Locally, see below.

## Node divergence found while building the harness

`server.keepAliveTimeout = 0` (`node:http`): **Node reads 0 as "never time
out"; Perry reads it as "no keep-alive"** and answers `Connection: close`,
closing the socket after the first response. Measured 2026-09-15 on macOS with
the arm-A compiler: with `= 0` the response carries `Connection: close` and the
socket is unusable after 0.3 s; with the setter dropped, or set to `600000`, it
carries `Connection: keep-alive` and is still reusable after 4 s. It is not
turnloop-related (both arms behave identically) and is not fixed here — the
harness app just stops relying on the Node meaning. Worth its own issue.

## Open issues for P1–P4

- **P1 (net/IPC).** Size `p0_config()` for real handles. Dispatch completions
  after `turn` and before releasing roots (see `AgentLoop::record`). Move the
  ext-net, TLS and ws keep-alive onto `Loop::alive()` and delete the remaining
  lock-only checks. Revisit `AGENT_LOOP`'s root-holder verdict once tokens name
  JS work.
- **P2 (child/stdin/dgram/fs-watch).** Replace the stdin latch, IPC atomics
  and fs-watch scans with loop handles. Child exits already wake the loop
  cross-thread (see `test_gap_9592` counters).
- **P3 (timers, phase order).**
  - Move timers into the loop heap. That makes `loop_deadline()` real and
    deletes the deadline scans, the `TimerQueue` counters and the #1114
    throttle.
  - Give every worker agent its own loop, poster and route. Retire the
    process-global `NOTIFIED`/`PRIMARY_ROUTE` pair in favour of per-agent
    posters.
  - Replace the "decline non-primary agents" rule with per-agent creation.
  - Map the web/WASI clocks: `turnloop::Instant` is a distinct type on the web
    backend.
- **P4 (pool, perry-ffi v2).** Replace `EXT_BLOCKING_TASKS_INFLIGHT` and
  `spawn_blocking` with pool jobs. Delete `js_native_work_submitted` together
  with the tokio predicate (P8).
- **Coexistence cost.** A process with any live tokio task (a server, or a
  pooled fetch connection) stays on the tokio tick. The P0 gain for servers
  arrives with P1/P5.
- **Embedders.** `perry_next_wake_ms` and `js_*_next_deadline` still truncate
  to whole milliseconds for hosts that drive their own wait.

## turnloop API gaps found (0.1.0-alpha.2)

1. **Exact `=` pins on shared ecosystem crates** force workspace-wide
   downgrades in any host (see Dependency). Caret requirements would avoid it.
2. **No way to clear or consume a pending notification without an OS call.** A
   notify that lands while the host runs costs the next `turn` a zero-timeout
   poll. Perry works around this with its own `in_turn` gate. A
   `Notifier::notify_if_parked()` with a race-free contract, or a
   `Driver::take_notification()`, would remove the workaround.
3. **`TurnInfo` does not say why a turn returned** (deadline, notifier, I/O).
   Hosts must infer it from `zero_event_waits`.
4. **I01 (queued posts plus pending native operations).** Not hit in P0, which
   posts nothing. Relevant from P1. P0 does not depend on the unreleased fix.
5. **`Config::default()` is heavy** (256 × 16 KiB pooled buffers, 4096
   operation slots with per-slot mutex warm-up). A "wait-only" or lazily grown
   configuration would suit embedding.
6. **No foreign readiness source.** A host that must also drive another
   reactor during migration (tokio's) cannot register that reactor's fd or
   waker in the same OS wait. This is why P0 alternates between the two waits
   instead of combining them.
7. **`Completions` default capacity 256** allocates per loop; there is no
   const or empty constructor for a host that expects no completions.
8. **`turn()` does not separate OS-wait time from completion-dispatch time.**
   A host that wants to publish "time parked" has to bracket the whole call, so
   from P1 on — when turns start carrying completions — `turnloop_wait_ns` will
   silently include dispatch. A `TurnInfo::waited` (or a pair of timestamps
   around the OS wait) would keep that number meaning what it says. In P0 the
   two are equal, because P0 submits no operation.
