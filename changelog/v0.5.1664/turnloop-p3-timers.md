### turnloop P3 — JS timers on a per-agent heap, and Node's event-loop phase order

**The timer store.** The three process-global `Mutex<Vec<_>>` timer queues
(`TIMER_QUEUE`, `CALLBACK_TIMERS`, `INTERVAL_TIMERS`) are replaced by one store
per JS agent (`crates/perry-runtime/src/timer/store.rs`): a slab of entries, two
`(deadline, seq)` binary min-heaps — ref'd and unref'd — a FIFO check queue for
`setImmediate`, a FIFO poll queue for native completion callbacks, and an id
index. What that removes:

- **the full-queue scans.** Every tick, every next-deadline computation and every
  liveness question used to walk all three queues, filtering on owner, `cleared`
  and ref state; a clear was a `retain` over the whole queue. Insert, cancel,
  expiry and re-arm are now O(log n) and the earliest deadline is a heap root.
- **the owner filter.** Partitioning by agent *is* the filter: `agent::owns(o)`
  is exactly `o == current_agent()`, so selecting the calling agent's partition
  answers #6185's question structurally. Android's split (TypeScript on the
  `perry-native` thread, the pump on the UI thread) is unaffected — both resolve
  to `PRIMARY_AGENT`.
- **the tombstones.** A cancelled entry leaves the heap immediately (turnloop
  DESIGN D6) instead of surviving as a `cleared` flag until the next scan.
- **the detached expiry batch.** Each phase pops one entry at a time, so #8036's
  batch-wide rooting is gone with it.
- **the pairwise keep-alive counters.** P0's per-queue counts were incremented
  and decremented at every mutation site with a debug assertion re-deriving them;
  the primary agent's counters are now republished from the partition after every
  mutation, so there is no pairing to get wrong.

**Node's phase order.** The generated event loop ran one iteration as
`microtasks → (timeouts and immediates in one batch) → nextTick → intervals →
cron → all I/O pumps → park`, which is not Node's order. It now runs
`microtask/nextTick checkpoint → timers → cron → poll (I/O pump, then the native
completion callbacks) → check (setImmediate)`, parking only when the check and
poll queues are empty — Node computes a zero poll timeout while immediates are
queued. `js_promise_run_microtasks_event_loop` no longer fires timers; the
generated loop emits `js_event_loop_timers_phase`,
`js_event_loop_poll_callbacks` and `js_event_loop_check_phase` itself. The
busy-wait pumps (`for await` over a stream, `fs.cp`, `perry_poll`) keep running
all three back to back, as do the native-UI host loops through
`js_callback_timer_tick`, because neither has a poll phase of its own.

**Behaviour changes, each measured against Node 26.5.1 five times before it was
made** (probes and transcripts in the P3 report):

- **`setImmediate` now runs after I/O, not before it.** Inside an I/O callback,
  `setImmediate` beats a `setTimeout(…, 0)` scheduled beside it, because poll is
  followed by check in the same iteration while the timeout waits for the next
  iteration's timers phase. Perry printed them the other way round.
- **An interval sorts with timeouts.** `setInterval(i, 3)`, `setTimeout(t5, 5)`
  and `setTimeout(t1, 1)`, all overdue, fire `t1, i, t5`; Perry drained a whole
  callback queue and then a whole interval queue and printed `t1, t5, i`.
- **`clearTimeout`/`clearImmediate`/`clearInterval` of a sibling that is already
  due now stops it.** The old tick detached the expired batch before the first
  callback ran, so a cancel from inside one of them arrived too late.
- **Native completion callbacks (`fs`, `dns`, `crypto`) are delivered in the
  poll phase, one turn after they are queued.** Perry performs those syscalls
  eagerly and defers only the callback; Node's are still on the threadpool, so a
  `setImmediate` queued beside a top-level `fs.readFile` wins 10/10 runs in
  either registration order. Staging the completion past the poll phase already
  in flight reproduces exactly that turn of latency.
- **`Timeout.refresh()` no longer re-refs an unref'd handle.** Node's `refresh()`
  re-inserts the timer and never touches `[kRefed]`.
- **An interval re-arms from the phase's clock read, before its callback runs**
  (libuv's `uv_timer_again` from `loop->time`), so an overrunning handler fires
  once per iteration instead of catching up in a burst, and `clearInterval` from
  inside the callback cancels it.

**turnloop.** The agent loop arms a single unreferenced timer handle at the
store's earliest deadline, so a park that ends at a JS timer ends on a real
`OpResult::Timer` completion and `Loop::next_deadline()` answers for Perry's
timers (DESIGN §9). It is `set_ref(false)` on purpose: Perry's own keep-alive
counters decide whether the loop lives. `PERRY_LOOP_STATS` gained `timer_arms=`
and `timer_expiries=`, so a timer workload whose expiries never reached the loop
says so rather than looking green.

Tests: `crates/perry-runtime/src/timer/store_tests.rs` (14 unit tests over the
heap, the id index, the snapshot boundaries, the poll staging and the counters),
and three gap fixtures —
`test-files/test_gap_turnloop_p3_phase_order.ts`,
`test_gap_turnloop_p3_io_phase_order.ts`,
`test_gap_turnloop_p3_timer_heap.ts`.
