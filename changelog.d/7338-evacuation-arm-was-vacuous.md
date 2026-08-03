`gc-native-roots`' forced-evacuation arm evacuated nothing, on every probe.

The probes drive collection with `gc()`, which takes `manual_collect` — a full
mark-sweep behind a forced conservative scan — and `PERRY_GC_FORCE_EVACUATE` is
read only on the *minor* path. Measured: `copied_objects` and `moved_objects`
were 0 on all eight probes, and 5 of 8 matched zero stack-map records while the
conservative scan did the rooting. The existing `--require-fp-walks` assert
passed throughout, because it checks that a walk *happened*, not that it *found*
anything.

This is #6942/#6946 repeating — the case CLAUDE.md records as costing months of
"passes under evacuation" that meant nothing — inside the gate whose results were
being used to argue for making statepoints the default.

The evacuation arms now drive the minor path
(`PERRY_GC_HEAP_LIMIT=8 PERRY_GC_INCREMENTAL=0 PERRY_CONSERVATIVE_STACK_SCAN=off`),
under which all 8 probes move: 5,946 to 90,271 objects copied each. A new
`scripts/gc_evacuation_liveness_assert.py` requires at least one copying minor
that copied at least one object, and names the cause when it finds none —
`manual_collect` (wrong path) or an ineligible copying minor (#7255).
