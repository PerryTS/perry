### `gc`: report incremental cycles that start and never finish (#7909)

A budgeted incremental cycle emits nothing until it *completes* — the `[gc]`
trace is written by `gc_finish_budgeted_cycle`. So a cycle that is started and
then starved is completely invisible: no trace line, no counter, no diagnostic,
while the mutator pays the SATB mark barrier on every heap store, every
shadow-slot root store and every allocation (allocate-black) for as long as the
cycle stays open.

`gc-handoff/apps/asyncpipe.ts` is in exactly that state on `main`, and it is
why the program reads as "zero GC cycles, but a third of the leaf profile is
collector machinery". `PERRY_GC_DIAG=1` now prints, at the process-exit
boundary:

```
[gc-incremental] cycle_starts=1 steps=15 completions=0 active_at_exit=true
                 mark_barrier_arms=1 mark_barrier_armed_us=37214
                 skips(reentrant=0 no_trigger=2 start_blocked=0 resume_blocked=0)
                 safepoints_blocked_by_budgeted=0 copying_minors=0
                 loop_polls=1 poll_arm_events=0 poll_armed_at_exit=0
```

One cycle started, fifteen steps, zero completions, still active at exit, the
mark barrier armed for **37 ms of a 127 ms program**, and not one collection
performed. No new env knob — this is the existing `PERRY_GC_DIAG`.

#### The mechanism the numbers describe

* `nursery_cap_active()` **is** `gc_moving_loop_polls_enabled()`, so the
  young-generation scavenge cap (16 MB, `PERRY_GC_SCAVENGE_NURSERY_MB`) goes due
  and — because nothing collects — stays due.
* every microtask drain runs `gc_runtime_safepoint()`, which starts a budgeted
  cycle as soon as *any* trigger is due, including that one.
* the cycle it starts is `low_pause_non_moving` by construction, so it cannot
  evacuate and cannot lower `copying_from_space_in_use_bytes()` — the quantity
  the cap tests.
* while it is active, `gc_safepoint_moving_minor` rejects every precise
  safepoint at its `budgeted` entry guard, so the collector that *could* clear
  the trigger never runs again.
* and at the pump's cadence (2048 work units per drain, ~17 drains in the whole
  program) it cannot finish.

The alloc-point path already routes nursery pressure away from the budgeted
stepper for exactly this reason; the host-safepoint path does not.

#### What the loop poll had to do with it: nothing

`PERRY_GC_MOVING_LOOP_POLLS=0` measures −14 % on that program, which read as
"incremental work driven at back-edge polls". The new counters retire that:
`poll_arm_events=0`, `loop_polls=1` — the back-edge poll is armed zero times and
taken once (the startup seed release) in the whole run. The knob acts only
through `nursery_cap_active()`. `PERRY_GC_SCAVENGE_NURSERY_MB=4096`, which moves
the cap and nothing else, reproduces it to three digits: −11.81 % vs −11.81 %
(instructions retired, best of 7).

#### Fix deliberately NOT shipped here

Giving the precise collector first refusal at the pump boundary removes the
stall completely (`cycle_starts` 1 → 0, `mark_barrier_armed_us` 37 214 → 0,
`copying_minors` 0 → 2, all 19 corpus programs byte-identical) and costs
**+51.4 % instructions and +57 % RSS** on `asyncpipe`, because the two minors it
unblocks are priced by #7915: one of them is a **134 ms minor that copied zero
objects**, spending its time scanning 82 registered runtime mutable root
scanners over 218 455 pointer roots. With no nursery trigger due the reorder is
inert to three digits (1691.7 M vs 1692.0 M), so that is the price of
collecting, not of the change. It is recorded in
`gc-handoff/GC7909-NOTES.md` §4 and belongs after #7915, not before it.

Tests: `an_active_budgeted_cycle_locks_out_the_moving_minor_and_keeps_the_barrier_armed`
pins the composition and asserts its own subject was live (the trigger is due,
the cycle really was started by the call under test, the block is attributed to
the `budgeted` guard specifically, and the completion counter moves too — so
`starts > completions` can be read as a stall rather than as a dead fixture);
`arm_events_count_arms_and_the_word_is_reported` pins the poll-arming pair.
