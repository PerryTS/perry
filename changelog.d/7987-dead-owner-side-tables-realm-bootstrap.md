### `dead_owner_side_tables` was measuring the `globalThis` bootstrap, not the death prune

`test_dead_arguments_object_entry_pruned_on_full_gc` and
`test_dead_owner_descriptor_entries_pruned_on_full_gc` (#7975) failed 10 of 200
`--test-threads=10` runs of their module. Filed as a self-race — it is not one.
Every test in the module already takes the same global isolation mutex, and run
**alone** both cases fail **200/200**; they pass only when a sibling ran first.

Two facts compose. Both cases reach an API that resolves the **process-global**
memoized `Object.prototype` address (`array::prototype_addr`) —
`set_property_attrs` for one, `js_object_set_field_by_name` for the other — and a
MISS runs the whole lazy `globalThis` bootstrap, ~1.15 MB allocated and ~410 KB
of it live-and-rooted, *inside the caller*. The cache misses exactly once per
PROCESS, so which libtest thread pays is a scheduling accident. And arena block
reset is all-or-nothing, so `gc::trace::mark_block_persisting_arena_objects`
force-MARKS every object in a block that holds one reachable object — the test's
own unrooted owner included. `dead_owner::PostTraceProbe::owner_is_dead` then
correctly reports "not dead" and the prune correctly keeps the entry: a block
that persists cannot recycle the owner's address, so the entry is not stale. The
test failed on the prune and named the prune.

`GcTestIsolationGuard::with_realm_bootstrapped()` runs the bootstrap inside the
isolation lock but before `ScopedRootScannerRegistryGuard` takes the thread's
scanners and before `reset_global_roots()`, putting the realm graph outside the
measured window.

The force-mark is cleared again by the sweep, so nothing about it survived a
collection for a test to read — the premise was unstateable.
`gc::trace::block_persist_force_mark_count()` is now an always-on,
O(1)-per-pass thread-local census of block-persistence force-marks, recorded by
**both** the whole-cycle pass and the budgeted `BlockPersistCycleState` arm (a
census that counted one arm would read zero on exactly the cycles that are
hardest to reason about). Same rationale as `gc::scan_fallback` (#7148).
`full_gc_with_no_block_persistence()` now fails as a *premise* instead of
letting the subject assertion mis-name it, and
`test_block_persistence_census_moves_when_a_block_has_a_live_tenant` plants the
confounder's exact shape — one rooted object and one unrooted owner in one
block, no realm involved — asserting both that the census moves and that the
force-marked owner's side-table entry correctly survives. Without that the
"premise held" verdicts would be vacuous.

No assertion was weakened. Verified: fixed binary 400/400 green on
`--test-threads=10 dead_owner_side_tables`, 200/200 on each case run alone,
200/200 on the new census case, and 300/300 on the full suite at default
parallelism — against `origin/main`'s 10/200, 200/200-failing alone, and
300/300 full-suite green (unchanged). Sabotage check: reverting only the guard
turns both cases into `test premise: block persistence force-marked objects
during this collection … left: 4083, right: 0`.
