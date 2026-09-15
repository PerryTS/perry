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
  still a queue scan. It is ruled out now: the deadline and the expiry read the
  same heap root, and the phase pops exactly the entries that root names. The
  remaining zero-budget returns are the legitimate transient "a timer really is
  due" case.
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

<!-- MEASUREMENTS -->

## Test evidence

<!-- EVIDENCE -->

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

<!-- INTEGRATOR -->
