### Fixed

- **A finished program no longer lingers about a second before exiting.**
  The generated event loop's body ended in an unconditional
  `js_wait_for_event()`, which parks for up to `IDLE_CAP_MS` (1 s) when no timer
  deadline is nearer. That park happened even on the iteration that CONSUMED
  the last event source — the drain that fired the final timer, or dispatched
  stdin's `'end'` — so the loop header discovered "nothing left to do" a full
  second after the answer was already known. On the reporting machine a program
  whose only work was a 20 ms `setTimeout` took 1082 ms (min of 5) against
  node's 132 ms, while the same binary exits in 45 ms when `process.exit(0)`
  runs in the first tick. It is pure user-visible latency, and it lands after
  the useful work is done — the part a user actually waits on.

  Lowering the constant is not the fix, and neither is a new wake. Every
  off-main producer in the runtime already calls `js_notify_main_thread` when
  its source goes away — the stdin reader's EOF path, the child-process and pty
  reactors, dgram, signals, the bun-FFI callback bridge — so the removals left
  unserved are precisely the ones the MAIN thread performed itself, inside the
  very body iteration that then parks. For those a wake is not an available
  shape: the main thread is the pump. It has to look before it sleeps.

  So the loop now looks. The body hands off to a new `event_loop.body_check`
  block that re-asks the same liveness question the header asks, and only the
  live answer reaches the park; the dead answer goes straight back to the
  header, which exits. Asking in codegen rather than inside `js_wait_for_event`
  is what makes the question EXACT: `js_cron_timer_has_pending` is a stdlib
  symbol perry-runtime's own archive does not define, so a runtime-side
  predicate would have had to omit cron — and would then spin on a cron-only
  program instead of parking. Here the predicate is literally the header's, by
  construction: both call one `emit_event_loop_liveness`, so the two cannot
  drift.

  Affected files:

  - `crates/perry-codegen/src/codegen/entry.rs` — `emit_event_loop_liveness`
    (extracted from the header's inline sequence, no arm changed), the
    `event_loop.body_check` re-check, and `event_loop.body_wait`, which still
    holds the park for a loop that does have live work.

  Validation: `test-files/test_gap_9441_idle_exit_latency.ts`, byte-compared
  against node 26.5.1. Each role's wall clock is compared against a role that
  exits in its first tick, so process spawn and runtime init cancel out, and
  the minimum of three samples is taken so a scheduling spike cannot decide the
  verdict. On unfixed `origin/main` the `timer-idle` and `stdin-end` arms both
  report `false`; node reports `true` for all four. `refed-timer` is the
  anti-regression arm — a live 300 ms timer must still hold the loop open — and
  `test_gap_9416_stdin_only_loop_liveness` covers the same ground for stdin.

  The mechanism is pinned structurally rather than on a stopwatch by
  `event_loop_body_rechecks_liveness_before_parking` in
  `crates/perry-codegen/src/codegen/entry/tests.rs`: the body must not contain
  `js_wait_for_event`, the re-check must consult every arm the header consults,
  and `event_loop.body_wait` must still park. Confirmed to fail against
  `origin/main`'s `entry.rs` ("the body must hand off to the liveness re-check,
  not park unconditionally") and pass with the fix.
