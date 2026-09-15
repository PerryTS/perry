# turnloop P3 — JS timers on a per-agent heap, and Node's event-loop phase order

Branch `turnloop/p3-timers`, based on `turnloop/p1-net` at `c6f185d6e8`. All
building, testing and measurement happened on the Linux build box
(`perrybuilder`, 32c/64t) in `/root/claude-turnloop-p3`. The gap oracle is the
pinned Node **26.5.1** (`/opt/node-v26.5.1-linux-x64/bin`), not the box default.

## What moved into the heap

`crates/perry-runtime/src/timer/store.rs` replaces the three process-global
`Mutex<Vec<_>>` queues (`TIMER_QUEUE`, `CALLBACK_TIMERS`, `INTERVAL_TIMERS`)
with **one store per JS agent**:

| structure | holds | phase |
|---|---|---|
| slab (`Vec<Option<Entry>>`) | every entry, at a stable index | — |
| ref'd min-heap on `(deadline, seq)` | `setTimeout`, `setInterval`, promise timers that keep the loop alive | timers |
| unref'd min-heap on `(deadline, seq)` | the same classes after `unref()` | timers |
| check FIFO | `setImmediate` | check |
| poll FIFO (staged + ready) | native completion callbacks (`fs`, `dns`, `crypto`) | poll |
| id index (`BTreeMap<i64, slab index>`) | `clearTimeout`/`ref`/`unref`/`refresh` lookups | — |

Everything a JS timer carries — the promise, the closure, the trailing
arguments, the `AsyncLocalStorage` snapshot, the async-hooks ids — stays in
Perry, because those are GC roots and Perry owns rooting (DESIGN §9). What moved
is the *ordering*, and with it the deadline the loop waits on.

### The scans, the truncation and the spin

- **Scans.** Every tick, every next-deadline computation and every liveness
  question used to walk all three queues end to end, filtering each entry on
  owner, `cleared` and ref state; `clearTimeout` was a `retain` over the whole
  queue and `js_timer_refresh` a linear `find`. Insert, cancel, expiry, re-arm
  and ref-change are now O(log n), and the earliest deadline is a heap root.
- **The owner filter is structural.** `agent::owns(o)` is exactly
  `o == current_agent()`, so selecting the calling agent's partition answers
  #6185's question by construction rather than by a predicate on every entry.
  Android's split (TypeScript on `perry-native`, the pump on the UI thread) is
  unaffected: both resolve to `PRIMARY_AGENT`. `retire_agent` drops a whole
  partition instead of running three `retain`s.
- **Tombstones are gone.** A cancelled timer leaves the heap immediately
  (DESIGN D6). The check and poll FIFOs keep an emptied slot as a placeholder
  that `pop` skips — O(1) cancel without an O(n) queue shift.
- **Millisecond truncation** was already gone from the *park* in P0; P3 removes
  the last place it could reappear, because `next_timer_deadline()` returns the
  heap root as an `Instant` and the three legacy `js_*_next_deadline` C entries
  are now whole-ms views of that one value rather than three independent scans.
- **The spin-until-throttle path.** P0 left the #1114 throttle as the only bound
  on a "deadline reports due, pump never consumes it" loop, and noted that
  nothing ruled that shape out structurally while every deadline source was
  still a queue scan. That hole is closed **for the timer sources**: the
  deadline and the expiry are now the same heap root, and the timers phase pops
  exactly the entries that root names, so a JS timer cannot report a due
  deadline the pump then declines to consume. The throttle stays as the safety
  net for the sources P3 did not touch — the stdlib readline provider and a
  host-registered driver — and for the legitimate transient "a timer really is
  due" return. One new zero-budget return was added deliberately: the park is
  skipped while the check or poll queue is non-empty, which routes through the
  same throttle so a caller that never runs the phase that would drain the
  queue is still bounded.
- **Keep-alive counters are republished, not paired.** P0 maintained a count per
  queue with an increment at every insert and a decrement at every removal, plus
  a debug assertion re-deriving them because an unpaired site is invisible in
  release. The primary agent's counters are now recomputed from the partition at
  the end of every `with_current`, so there is no pairing to get wrong.

### The turnloop timer

`event_pump::agent_loop::arm_timer` keeps **one unreferenced timer handle** per
agent loop, armed at the store's earliest deadline and moved with
`timer_reset` when that deadline changes. A park that ends at a JS timer now
ends on a real `OpResult::Timer` completion, and `Loop::next_deadline()` answers
for Perry's timers (DESIGN §9). Two deliberate details:

- **`set_ref(handle, false)` is load-bearing, not hygiene.** A timer operation
  on a referenced handle counts toward turnloop's `refs`, so an armed deadline
  would otherwise make `Loop::alive()` true on its own and defeat Perry's
  keep-alive accounting. There is a unit test that arms a timer and asserts
  `alive()` stays false.
- **A one-shot expiry is terminal**, so the handle is closed on expiry and a
  fresh one created for the next deadline; `timer_reset` covers every
  before-expiry move. The `Closed` completion carries the same token and is
  ignored.

Perry still computes its own deadline for the park as well. That is not
redundancy for its own sake: a thread with no loop — a worker agent, or the pump
thread acting for the primary agent on Android — has no armed timer, and the
park must still be exact there. The two agree by construction; `loop_deadline()`
is `min`ed with `next_timer_deadline()` and a unit test asserts they match.

## The phase order

### Before

```
iteration = microtasks → (expired timeouts AND immediates, one batch)
            → nextTick → intervals → cron → all I/O pumps → park
```

### After

```
iteration = nextTick+microtask checkpoint (+ unhandled-rejection report)
            → timers   (promise timers, setTimeout, setInterval — deadline order)
            → cron
            → poll     (js_run_stdlib_pump, then the native completion callbacks)
            → check    (setImmediate)
            → park, unless the check or poll queue is non-empty
```

with a `nextTick` + microtask checkpoint after **every** callback in every
phase. `js_promise_run_microtasks_event_loop` no longer fires timers; the
generated loop emits `js_event_loop_timers_phase`, `js_event_loop_poll_callbacks`
and `js_event_loop_check_phase` at the right points. The park at the end of the
iteration **is** the poll block: its deadline is the timer heap's root, so
"park, then run the next iteration's timers phase" is libuv's "block in poll
until the next deadline, then run the timers".

Two Perry-specific notes:

- **Node's *pending callbacks* phase has no Perry counterpart.** It carries
  deferred TCP errors from the previous iteration; Perry has no such deferral
  queue, so the phase would be empty. It is not implemented rather than
  implemented as dead code.
- **Node's *close callbacks* phase has no Perry counterpart either.** Perry
  emits `'close'` synchronously from the subsystem that closes, so there is no
  queue to move into a phase. See "What P3 did not do" below for the measured
  Node behaviour and what implementing it would take.

Hosts without a poll phase of their own keep the composite:
`js_callback_timer_tick` runs timers → poll callbacks → check, which is what the
native-UI loops (iOS, tvOS, watchOS, visionOS, Android, GTK4, WinUI) already
called it for, and `js_await_loop_tick_timers` does the same for the codegen
`await` busy-wait. The busy-wait pumps behind `for await` over a stream, `fs.cp`
and `perry_poll` keep `MicrotaskDrainMode::AllowTimers`' "run whatever is due".

## Behaviour changes, and the Node comparison that justifies each

Every expectation below was measured on the pinned oracle **before** the change
was made, five runs each (twenty for the one that turned out racy). Probe
sources and full transcripts are on the box in
`/root/claude-turnloop-p3/oracle/{probes,results}`.

| # | Change | Node 26.5.1 | Perry before | Perry after |
|---|---|---|---|---|
| 1 | `setImmediate` runs after I/O, not before it | inside an `fs.readFile` callback: `immediate` then `timeout`, 5/5 | `timeout` then `immediate` | matches |
| 2 | An interval sorts with timeouts by deadline | `setInterval(i,3)`, `setTimeout(t5,5)`, `setTimeout(t1,1)` all overdue → `t1, i, t5`, 5/5 | `t1, t5, i` (queue order, not deadline order) | matches |
| 3 | Cancelling a sibling that is already due stops it | `a` clears `b` in the same expired batch → `b` never runs, 5/5; same for `clearImmediate` (and `c` still runs) and for a timeout clearing a same-instant interval | `b` ran: the batch was detached before the first callback | matches |
| 4 | Native completion callbacks are delivered in the poll phase, one turn after they are queued | a top-level `setImmediate` beats a top-level `fs.readFile` callback 10/10 in **either** registration order | FIFO with the immediates: matched when the immediate was registered first, diverged when it was second | matches both orders |
| 5 | `Timeout.refresh()` does not re-ref an unref'd handle | `hasRef()` stays `false` after `refresh()`, 5/5 | `refresh()` forced the handle back to ref'd | matches |
| 6 | An interval re-arms from the phase's clock read, before its callback runs | a 10 ms interval with a 25 ms handler fires once per iteration, ~25 ms apart, no catch-up burst, 5/5 | re-armed from `Instant::now()` after the callback | matches |

Change 3 also fixes the shape #8036 patched from the other side: with one entry
popped at a time there is no detached `Vec` of timer records for the collector to
miss, so the batch-wide rooting that bug needed is gone rather than extended.

### Orderings deliberately NOT pinned

The oracle showed these to be genuinely racy under Node, so no fixture asserts
them and no implementation choice was made to satisfy them:

- `setTimeout(…, 0)` vs `setImmediate` at main-module top level — stable 5/5 in
  this sample, but Node documents it as not guaranteed;
- the same pair scheduled from *inside* a running `setImmediate` callback —
  14/20 one way, 6/20 the other;
- `setImmediate` vs a **cheap** `fs.stat('.')` callback — 3/5 vs 2/5. Change 4's
  10/10 result holds for I/O costly enough to exceed one loop turn, which is why
  the model is "one turn of latency", not "the immediate always wins";
- how many loop turns a top-level `fs.readFile` callback takes (5–7 across
  runs, and 4–8 when issued from inside an immediate).

## `PERRY_LOOP_STATS`

`timer_arms=` and `timer_expiries=` are new. They exist so the arming cannot be
decorative: a timer workload that reports `timer_expiries=0` means the heap's
deadline never reached the loop, whatever the turn count says — the "a gate must
assert its subject was live" rule applied to this change's own instrument.

### Measured, Linux x86_64, release

The subject is `scripts/turnloop/apps/timer_loop_stats.ts`: 25 quiet `await
sleep()` parks, 20 sub-millisecond remainders, 2,000 short timeouts, a 200-deep
`setImmediate` chain and a 50-tick interval. Three interleaved runs per arm.

| arm | turns | os_waits | zero_event_waits | completions | timer_arms | timer_expiries |
|---|---|---|---|---|---|---|
| base `c6f185d6e8` | 107 / 97 / 117 | 97 / 95 / 114 | 107 / 97 / 117 | **0** | — | — |
| P3 | 351 / 373 / 336 | 161 / 173 / 160 | 168 / 181 / 163 | 367 / 385 / 349 | 518 / 521 / 474 | **184 / 193 / 174** |

`completions=0` on the base arm is the point: turnloop carried nothing for a
timer program, and every wake was a timeout Perry had computed for itself. On
P3 every JS timer deadline the loop waited on arrives as an `OpResult::Timer`.

**The quiet-timer cost is unchanged**, which is the claim that matters for
DESIGN §10 rule 4a. A program that is nothing but 20 × `await sleep(20)`:

| arm | turns | os_waits | zero_event_waits | completions | timer_expiries |
|---|---|---|---|---|---|
| base | 20 / 20 / 20 | 20 / 20 / 20 | 20 / 20 / 20 | 0 | — |
| P3 | 38 / 38 / 38 | **20 / 20 / 20** | 20 / 20 / 20 | 37 | 19 |

One OS wait per timer on both arms — no spin. P3's extra 18 *turns* are
non-blocking: a one-shot turnloop timer's expiry is terminal, so its handle is
closed and the resulting `Closed` completion is collected by a `Timeout::Now`
turn. It costs a turn per expiry and no syscall. Arming the deadline as a
**repeating** timer instead would keep the operation alive across expiries and
remove that turn, the handle churn and half the completions; it is a follow-up,
not a correctness issue, and it is not done here because it was measured to cost
no OS wait.

**Where the mixed workload's extra OS waits come from.** The first table's
`os_waits` rise (≈97 → ≈165) is not spread over the whole program. Two probes
split it:

| probe | base os_waits | P3 os_waits |
|---|---|---|
| a 200-deep `setImmediate` chain | the loop never parks (`parked=0`) | the loop never parks (`parked=0`) |
| 2,000 `setTimeout`s across 7 distinct delays | 38 / 41 | 47 / 61 |

The check-phase split costs nothing: neither arm parks at all while immediates
are queued. The increase is in **timer churn** — roughly 10-20 extra waits per
2,000 timers, against a run-to-run spread of the same order. No mechanism is
claimed for it here, because none was measured: an instruction A/B at cgu=1 with
a control probe is what would price it, and P3 did not run one (see "For the
integrator").

## Test evidence

### The three P3 fixtures, byte-for-byte against Node 26.5.1

```
$ /root/claude-turnloop-p3/p3run.sh
=== test_gap_turnloop_p3_phase_order   MATCH
=== test_gap_turnloop_p3_io_phase_order MATCH
=== test_gap_turnloop_p3_timer_heap    MATCH
```

Each was validated against the oracle five times before Perry ever ran it, and
the orderings the oracle showed to be racy were removed from the fixtures rather
than pinned (see above). `test_gap_turnloop_p3_timer_heap` also ends by proving
the loop exits: a lone `setImmediate` keeps it alive for exactly one more turn
while an unref'd 60 s timeout does not hold the process open, so a regression
there shows up as a harness timeout rather than a diff.

### Runtime unit tests

`crates/perry-runtime/src/timer/store_tests.rs` — 14 tests over the structure
itself, each asserting its subject was populated (an empty store would satisfy
most ordering assertions vacuously): deadline-then-creation drain order, the
phase snapshot boundary, immediate cancellation with the heap left ordered, the
class filter that keeps `clearImmediate` off a Timeout, ref/unref moving an
entry between heaps while leaving firing ungated, refresh preserving ref state,
the check FIFO's snapshot and cancelled-slot skipping, the poll queue's
one-turn staging and its keep-alive contribution, interval re-arm from the phase
clock, the republished primary counters, agent purge, and a 1,000-entry
insert/cancel churn that asserts the drain order is the sorted order.

`crates/perry-runtime/src/event_pump/agent_loop_tests.rs` — three new tests for
the arming: that an armed deadline does **not** answer `Loop::alive()` (the
sabotage check for the `set_ref(false)`), that an expiry arrives as a real
`OpResult::Timer` completion after a real OS wait, and that the armed deadline
and Perry's own `next_timer_deadline()` are the same instant.

### The gap suite, against a baseline built from this branch's own base

Both arms ran the same 8-shard fast tier (`PERRY_SKIP_BUILD=1`, which implies
`PERRY_NO_AUTO_OPTIMIZE=1`) against the pinned oracle on the same box. The
baseline is `c6f185d6e8` — this branch's base — because the committed snapshot
already disagrees with it: three tests are non-passing on the base that the
snapshot expects to pass, and crediting P3 with those would be exactly the
mistake P1 avoided by building its own baseline.

| | base `c6f185d6e8` | P3 |
|---|---|---|
| tests | 796 | 799 (the three new P3 fixtures) |
| pass | 787 | **790** |
| parity_fail | **9** | **9 — the same nine** |
| compile_fail | 0 | 0 |
| status changes vs base | — | **0** |

Not one test changed status in either direction, and the three new fixtures
pass. That is the whole verdict: the phase reorder, the unified heap, the
one-at-a-time dispatch, the poll staging and the two cancel/refresh semantics
changes cost the existing suite nothing.

The nine parity failures are identical in both arms and none is P3's:
`test_gap_2159_defineproperty_class_prototype`,
`test_gap_2514_settracesigint`, `test_gap_2899_2779_2777_static_helpers`,
`test_gap_disposablestack_2875`, `test_gap_iterator_prototype_next_patch`,
`test_gap_json_lazy_defineproperty_index`,
`test_gap_perfhooks_3088_3008_3010_3011`,
`test_gap_prop_plan_cache_invalidation`, `test_gap_v8_2_3680plus`. Six are in
the committed snapshot as known failures; the other three
(`…_static_helpers`, `disposablestack_2875`, `iterator_prototype_next_patch`)
are pre-existing regressions on the base commit, not P3's.

The first P3 run of the suite did report 25 compile failures, and they were a
harness artifact rather than a code result: they were exactly the ext-routed set
— every test whose link needs a `perry-ext-*` archive (`http`, `http2`, `net`,
`ws`, `zlib`, `events`, plus the WebAssembly and native-base fixtures). The gap
tier does not prebuild those, so each such test shells out to
`cargo build -p perry-ext-…`, and eight shards plus a `cargo test` running in
the same tree serialised on one cargo lock until the per-test compile timed out.
The table above is the re-run with nothing else holding that lock; all 25 pass.
Worth knowing for anyone repeating this: **do not prebuild the ext archives with
`--features perry-stdlib/external-*-pump` to avoid the fallback.** Those
features make `libperry_stdlib.a` reference `js_ext_http_*`, which then fails to
link for every test that does *not* import `http` — the gap tier wants the plain
archives.

### The workspace unit tests

`RUST_TEST_THREADS=1 cargo test --release -p perry-runtime --lib` on this box:

| arm | result |
|---|---|
| base `c6f185d6e8` | FAILED. 3965 passed; **2 failed** |
| P3 | FAILED. 3972 passed; **2 failed** |

The two failures are the same on both arms and neither is P3's:
`gc::tests::heap_generation::a_free_or_move_outside_every_scope_is_caught_in_debug_builds`
(the name says it — the funnel assertion it waits for is a `debug_assert`, and
this is a release test build; it passes in debug) and
`native_stack::tests::stack_top_respects_custom_thread_stack_sizes` (fails in
debug too, on this box).

`cargo test -p perry-codegen --lib`: **1543 passed, 0 failed**, including the
event-loop entry tests rewritten for the phase order. Linking that test binary
on this box needs `LIBRARY_PATH` pointing at a `libzstd.so` symlink — the box
has `libzstd.so.1` but no dev symlink, which is environmental and unrelated.

Four `promise::microtasks::empty` tests needed updating, and the change is not a
weakening: two of them used `js_promise_run_microtasks_event_loop()` as a
stand-in for one event-loop turn, which it no longer is, so they now drive the
same phases the generated loop emits. Their subjects — beforeExit must not
consume a pending timer and the next turn must; buffered stdin must be delivered
without any timer — are unchanged. (The other two failed only because the second
of those leaks process-global stdin state when it fails.)

### GC stress with pending timers

```
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=<1|7|12345> PERRY_GC_SCHEDULE_RATE=1 \
PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 <program>
```

over four programs — `test_gap_gc_interval_args_rooting`, the two P3 phase
fixtures, and `timer_loop_stats` — at three seeds. All twelve runs exit 0 with
no SIGSEGV from the quarantine reporter, and the instruments prove they were
armed rather than merely quiet:

| program | `[gc-fromspace-protect] retired_set` lines | `[gc…]` diagnostic lines | timer_expiries |
|---|---|---|---|
| `gc_interval` | 1,214 | 37,678 | — (never parks) |
| `phase_order` | 146 | 4,728 | 0 (every timer overdue; never parks) |
| `timer_heap` | 301-304 | 9,786-9,882 | 7 |
| `timer_loop_stats` | 4,624-4,625 | 162,047-162,080 | 28 |

A run with zero copying minors protects nothing and would pass vacuously; every
row above ran hundreds to thousands of them, so the from-space really was
quarantined and `mprotect`ed while timer entries, their arguments and their
async-context snapshots were live in the store.

## What P3 did not do

- **Close callbacks.** Node runs a close-callbacks phase after check, and it is
  observable: a `setImmediate` scheduled at the point a socket is about to close
  always runs before that socket's `'close'` listener (5/5). Perry has no
  deferred close queue at all — `'close'` is emitted synchronously by whichever
  subsystem closes the handle, so there is nothing to move into a phase and an
  empty phase would be untested code. Giving Perry a real close phase means
  routing every `'close'` emission in `net`, the HTTP server, streams and
  `child_process` through a queue, which is P1/P2/P5 surface, not P3's. It is
  the one row of Node's five-phase cycle that remains unimplemented, and it is
  named here rather than stubbed.
- **`setTimeout` delay normalization.** `normalize_timer_delay` is untouched:
  Perry keeps `setTimeout(f, 0)` at 0 ms where Node clamps to 1 ms. The oracle's
  own measurement of delay 0 vs 0.5 vs 1 did not settle cleanly, the change would
  move every `setTimeout(…, 0)` fixture in the suite, and P0 already flagged a
  reverted checkpoint for making exactly this change unreviewed. It belongs in
  its own change with its own measurement.
- **Cron.** `js_cron_timer_tick` still keeps its own stdlib `Vec` and still has
  no deadline provider, so a cron-only program parks to the 1 s idle cap. It is
  emitted adjacent to the timers phase, where it was.
- **Per-agent loops.** Worker agents still have no `turnloop::Loop` (P0's
  position). They get their own timer *partition* here, which is the half of
  DESIGN §5a.7 that P3 owns; the loop itself waits for P4.

## For the integrator

The branch is `turnloop/p3-timers` on `origin`, four commits on top of
`turnloop/p1-net` (`c6f185d6e8`). Nothing here bumps the version — the
maintainer does that at merge.

Run, on a machine with the pinned oracle installed:

```bash
# unit tests (perry-runtime's are NOT parallel-safe)
RUST_TEST_THREADS=1 cargo test --release -p perry-runtime
cargo test --release -p perry-codegen

# the gap suite, against a baseline built from this branch's OWN base commit
cargo build --release -p perry -p perry-runtime -p perry-stdlib \
  -p perry-runtime-static -p perry-stdlib-static
PERRY_SKIP_BUILD=1 ./scripts/run_gap_tests.sh

# the three P3 fixtures on their own
PERRY_SKIP_BUILD=1 ./run_parity_tests.sh --filter test_gap_turnloop_p3_

# GC stress with pending timers
PERRY_GC_DIAG=1 PERRY_GC_SCHEDULE_SEED=7 PERRY_GC_SCHEDULE_RATE=1 \
  PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64 \
  PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_LOOP_STATS=1 ./timer_loop_stats

# the loop-stats subject
PERRY_LOOP_STATS=1 ./timer_loop_stats     # scripts/turnloop/apps/timer_loop_stats.ts
```

Still to run, and NOT run here:

- **Windows and macOS.** Everything in this report was measured on Linux
  x86_64. The timer store is portable Rust and the arming goes through
  turnloop's cross-platform `timer`/`timer_reset`/`close`, but neither arm has
  been exercised. The native-UI host loops (iOS, tvOS, watchOS, visionOS,
  Android, GTK4, WinUI) call `js_callback_timer_tick` + `js_interval_timer_tick`
  and are covered only by the composite entry's definition, not by a run.
- **An instruction A/B at cgu=1 with a control probe** (DESIGN §12's per-phase
  requirement). The change is a clear algorithmic improvement on paper — heap
  operations replacing whole-queue scans — but "on paper" is not a measurement,
  and the extra loop iteration native completion callbacks now take is a real
  cost that an A/B should price.
- **The auto-optimize gap tier.** Only the fast tier (`PERRY_SKIP_BUILD=1`,
  `PERRY_NO_AUTO_OPTIMIZE=1`) ran here.
- **The node-suite behavioural corpus**, in particular its `timers` and `fs`
  modules, which are the two this change most directly touches.
