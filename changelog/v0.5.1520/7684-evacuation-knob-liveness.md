### gc: the evacuation knobs stop lying about what they gate (#7611, #6946, #7604)

Three knobs believed to exercise something that did not. One is deleted, one is
made real, one is made checkable.

#### `PERRY_GEN_GC_EVACUATE` — **deleted** (#7611)

Measured on the pinned quiet host, identical binaries and protocol, the only
difference being the knob: a cell-by-cell diff over all 12 gc-ratchet probes ×
8 counters reported **0 of 96 cells moved** — bit-identical medians, and
`gc_ratchet.py check` exit 0 with the knob set. The same procedure with
`PERRY_GEN_GC=0` moved **79** cells and returned 90 findings, so the harness was
sensitive and this knob specifically was not.

The mechanism, which #7611 correctly flagged as unconfirmed and which is
confirmed here from the code: the knob gated `evacuation_policy_allowed` — the
C4b tenured→old-gen policy evacuation in the **non-copying fallback** minor, and
the old-page defrag selection. Every counter the ratchet reads
(`copied_objects`, `copied_bytes`, `promoted_*`) comes from
`gc_collect_minor_copying_fast_path`, which the knob never gated and which is
reached *before* the fallback path.

Its one unique live effect was a footgun: it vetoed
`gc_force_evacuate_enabled()`, so an ambient `PERRY_GEN_GC_EVACUATE=0` silently
disarmed `PERRY_GC_ZEAL` — the #7154 instrument — and a zeal run could report
"clean" having moved nothing. `zeal_implies_forced_evacuation` used to take that
precedence arm and `return` *without exercising zeal at all*, which is the
vacuous-green shape the kill-policy exists to catch. It now has one arm and that
arm always runs.

**The branch is not deleted with the knob**, because the branch has another
controller that *is* exercised: `evacuation_policy_allowed` is still false on
every budgeted low-pause cycle, and `budgeted_low_pause_minor_does_not_evacuate`
asserts that arm behaviourally — nothing moved, no forwarding stub, old-page
selection skipped, `trace.evacuation_policy.reason == "low_pause_non_moving"`.
What stopped existing is the untested *configuration*, per CLAUDE.md's binding
kill-policy: *"a mode that still exists is a decision that hasn't been made."*

#### `PERRY_GC_FORCE_EVACUATE` on the `gc()` path — **fixed** (#6946)

The knob is read only on the minor path while `manual_gc_collect_now` ran a full
mark-sweep, which never evacuates. Five suites still drive collection as
`gc(); assertFreed()` under it and exercised no evacuation at all
(`gc_property_key_operand_rooting_6935`, `gc_dynamic_arith_operand_rooting_6655`,
`gc_string_coerce_property_key_rooting_6943`, `gc_side_table_roots_evacuation`,
`gc/tests/cycle_state.rs`).

An explicit `gc()` under the knob now runs an **evacuating minor first**, then
the full sweep. #7657 is what made this possible: this site used to take
`ManualGcScanGuard::force_full_scan`, and a forced conservative scan makes the
copying minor ineligible outright (`CopiedMinorFallbackReason::ConservativeStack`).
With precise roots the copying minor here is exactly as sound as the full sweep
that follows. New `FullEscalation::Refused` keeps the two throughput-pacing
predicates from turning that minor back into a non-moving full sweep — the
original bug in a new place. Default-off knob, so an ordinary `gc()` is
unchanged.

#### zeal liveness — **readable, and a vacuous run now fails** (#7604)

`zeal_forced_collections()` was **unreachable from a compiled program**: no JS
API, no diagnostic line, no exit report — while CLAUDE.md's instrument table
instructed operators to check it. The only alternative, `PERRY_GC_DIAG=1` plus
grep, wrote **212 MB of stderr in ten minutes** on a 400k-iteration ratchet
probe, so it was not a usable check either.

Three process-global counters are added — `copying_minor_cycles()`,
`moved_objects_total()` and `loop_polls_reached()` — because "zeal forced a
collection", "a collection moved something" and "a loop body was covered at all"
are three different claims and only the third distinguishes in-loop coverage
from event-loop-boundary zeal. At the process-exit boundary a zeal run now
prints its verdict and **exits 70** when any of them says the instrument did not
fire:

```
# an ALLOCATING loop, compiled and run with PERRY_GC_MOVING_LOOP_POLLS=1
[gc-zeal] forced_collections=20069 copying_minors=20069 moved_objects=95506 loop_polls=20064
exit 0

# the COMPUTE-ONLY loop, same flags: codegen emits no poll for a provably
# alloc-free body, so zeal only fired at event-loop boundaries
[gc-zeal] forced_collections=5 copying_minors=5 moved_objects=4 loop_polls=0
[gc-zeal] THIS RUN EXERCISED NOTHING WORTH TRUSTING. … PERRY_GC_MOVING_LOOP_POLLS=1
was set but NOT ONE back-edge poll was reached …
exit 70
```

Note what a two-counter verdict would have done with the second run: `forced=5`,
`copying_minors=5`, `moved_objects=4` — every counter says "live". Only
`loop_polls` says the loop was never covered.

`loop_polls` also exists because the obvious external check is wrong:
`nm`/`objdump -d BIN | grep -c js_gc_loop_safepoint` reports **0** on the
alloc-loop binary whose polls demonstrably fired 20,064 times, so an operator
following that advice concludes the polls are absent when they are not. The
error message says so explicitly.

Stated limitation: `process.exit()` terminates via `libc::_exit` and never
reaches the boundary, as does an uncaught throw.

**What #7604 reported does not reproduce on current `main`.** On
`01_nursery_churn` compiled with `PERRY_GC_MOVING_LOOP_POLLS=1` and run under
`PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`, the recipe produced **741,630**
`[gc-copy-minor] ran` lines with real `copied_objects`, and
`[gc-fromspace-protect] … retired_set=#N` throughout. What *is* real is the
issue's underlying worry, and it has a second cause the issue did not name: a
compute-only loop compiled *with* the flag contains **zero**
`js_gc_loop_safepoint` call sites, because codegen deliberately omits the poll
for provably alloc-free bodies (`loop_purity::loop_may_allocate`) and
incidentally omits it for the specialized `for` / `for-of` / `for-in` lowerings
(the COVERAGE note on `emit_gc_loop_safepoint`). Under zeal that binary collected
5 times, at event-loop boundaries only — and nothing said so.

#### Shown able to fail

* Deleting the forced minor from `manual_gc_collect_now` turns
  `explicit_gc_under_forced_evacuation_runs_a_moving_minor` red with
  `before=0 after=0` — verified with `error[` count 0 and `Running unittests`
  present, so the red is the assertion and not a build break.
* The zeal verdict is a pure function with four unit tests: `forced=0` (no
  safepoint ever fired), `cycles=0` (every forced collection escalated to a
  non-moving full sweep, which `forced > 0` alone would have called live),
  `loop_polls=0` with polls requested (#7604's own shape, with the measured
  compute-only counters), and the passing case with its numbers. A fifth pins
  that `cycles>0, moved=0` is *not* a failure and that `loop_polls=0` *without*
  the request is fine, so a future "tighten it" edit has to argue with a test.
* End to end on real binaries built from this branch: the compute-only probe
  exits **70** where it previously exited 0 silently; the allocating probe exits
  0 with its numbers; an unzealed run of the same binary prints no `[gc-zeal]`
  line at all.
