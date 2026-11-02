**fix(gc): a host safepoint no longer starts a budgeted cycle for a nursery-cap trigger it cannot discharge (#7909).**

`gc_budgeted_due_trigger()` reported the young-generation scavenge cap as
`BudgetedGcTrigger::ArenaBytes`, so a host safepoint or mutator assist started a
**budgeted** cycle for it. A budgeted cycle is `low_pause_non_moving` by
construction — it sweeps in place and cannot lower
`copying_from_space_in_use_bytes()`, the exact quantity `young_scavenge_cap_due()`
tests — while `gc_safepoint_moving_minor` rejects every precise safepoint at its
`budgeted` entry guard for as long as the cycle is open. The two compose into a
self-sustaining stall (cap due → cycle started → moving minor locked out →
nothing reclaims → cap still due) that keeps the SATB mark barrier armed for the
rest of the process and reports **nothing**, because the `[gc]` trace is written
by the completion path. Measured on `gc-handoff/apps/asyncpipe.ts` when it was
filed: 1 cycle started, 15 steps, 0 completions, barrier armed 37 ms of a 127 ms
program, zero collections.

The cap is now its own trigger variant, `YoungScavengeCap`. Every collection site
treats it identically to `ArenaBytes`; the split exists solely so the budgeted
stepper can tell them apart at the moment it decides whether to **start** a cycle.
When the cap is the only due trigger and the cycle would be budgeted, no cycle is
started and the pressure is deferred to the precise safepoint — arena baseline
included, so the `moving_defer_within_slack` valve is not left reading a stale,
already-exceeded baseline — exactly as `gc_check_trigger`'s alloc-point arm has
always done. That asymmetry between the two paths was the bug. No new knob;
`PERRY_GC_DIAG=1`'s `[gc-incremental]` line gains `nursery_cap_deferred=N` so the
refusal is distinguishable from "nothing was due".

The regression test drives the real host-safepoint path with a per-thread cap
override and pairs the refusal with a control phase on the same fixture — same
thread, same heap, only the due trigger differs, and the control must still start
a cycle — so the pair discriminates "declines this trigger" from "declines
everything". It is a unit test rather than an end-to-end fixture because the
issue's named reproducer is not durable: `PERRY_GC_SCAVENGE_NURSERY_MB=4` on
`apps/asyncpipe.ts` reproduced the filed signature at 0.5.1490 and reproduces
nothing at 0.5.1495; swept across the whole dial the defect now presents one
notch lower, at cap 2, where a budgeted cycle is still started for the cap and
arms the mark barrier for 87.9 ms of a ~127 ms program.
