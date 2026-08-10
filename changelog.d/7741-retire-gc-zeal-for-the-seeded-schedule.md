### Removed

**`PERRY_GC_ZEAL` and `PERRY_GC_ZEAL_ALLOC_KB` are gone.** The seeded GC
schedule (`PERRY_GC_SCHEDULE_SEED`) covers everything they did:
`PERRY_GC_SCHEDULE_RATE=1` resolves to the always-threshold, so it selects
every candidate safepoint — the same points the removed knob collected at — and
`PERRY_GC_SCHEDULE_ALLOC_KB` carries the same allocation pacing with the same
default (4 KB) and the same `0` escape hatch for literal every-poll candidacy.

Two knobs that differ only in how they pick safepoints are two configurations
to keep exercised, and CLAUDE.md's GC knob kill-policy is explicit that a mode
which still exists is a decision that has not been made. Keeping both had
already forced a precedence rule — one owned the poll arm when both were set,
purely so their forced-collection counters could not double-count the same
minor — which is coupling that exists only to reconcile a redundancy. The
schedule is a strict superset: normal pacing at rate 0, maximum density at rate
1, everything in between, and it is the only one of the two that hands back a
reproducer.

The instrument-liveness counters (`copying_minor_cycles`,
`moved_objects_total`, `loop_polls_reached`, #7604) now live in
`gc/instruments.rs`. They count what the **collector** did, not what forced it,
so a counter no longer dies with the knob that happened to carry it; they also
appear in the schedule's exit summary, so even a sub-endpoint run states
whether it exercised anything. The #7604 verdict — same three failure causes,
same exit 70 — is `schedule_liveness_report`, and it fires **only at the rate-1
endpoint**: below that, a sampling run that forces nothing is a legitimate
outcome (`RATE=0` is the documented on-but-selects-nothing control arm), so
holding those runs to the endpoint's contract would turn every quiet seed into
a false failure.

**Coverage moved, not shrank.** The pacing bound and its two liveness
assertions, the `ALLOC_KB=0` every-poll arm, the high-water-mark rearm, the
poll-word arming contract, the forced-evacuation implication and the
quarantine-compose pairing all have schedule counterparts; three tests were
dropped as exact duplicates of schedule tests that already existed.
`scripts/gc_instrument_smoke.sh`'s two stress arms — the #7254
`VERIFY_EVACUATION` pairing and #7728's termination budget — run against the
schedule with their non-vacuity assertions intact, so neither can go green from
a run that collected or moved nothing.

**Validation.** `cargo check -p perry-runtime -p perry-codegen --all-targets`
clean (remaining warnings are `main`'s pre-existing `-D warnings` debt,
byte-identical on the parent). 207 tests green under `--test-threads=1` across
`gc::tests::schedule`, `gc::tests::fromspace_protect`, `gc::tests::evacuation`,
`gc::tests::triggers`, `gc::tests::copying` and `gc::poll_arm`.
`scripts/check_file_size.sh` OK; `bash -n` clean on the smoke script.

**Review follow-ups.** A blocked safepoint no longer charges the pacing stride:
`gc_safepoint_moving_minor` now reports whether it handled the safepoint, and
the poll arm rearms only when it did — a safepoint blocked by an entry guard
consumes no schedule slot, so charging it a stride silently dropped the realised
density below the requested rate. `PERRY_GC_SCHEDULE_ALLOC_KB` clamps at 1 GiB
instead of saturating, because a stride nothing can reach is an off switch
wearing an on label. The poll-word arming test now runs the real resolution in
both directions instead of reading the startup value, which it could do because
the resolution became a resettable flag rather than a `std::sync::Once`; it
fails if either direction is broken. `scripts/gc_schedule_fuzz.sh` rejects an
out-of-range rate rather than letting the runtime clamp it and then printing a
reproduce command for a density it did not run at, and pins allocation pacing
into both the runs and the printed command.

**Fixed during the merge audit — worker-teardown abort under any resolved
seed.** The exit summary runs on the per-thread teardown funnel
(`js_gc_release_current_thread_collection_side_allocations`), where a tokio
worker's TLS is already destroyed; its main-thread gate called
`std::thread::current()` (which panics there), and even past the gate a
TLS-dead worker printing would panic inside `eprintln!`'s reentrant stderr
lock. Under the mode's own panic hook that aborted every seeded run
(exit 134) after correct output. The gate now compares OS thread ids
(`pthread_self`, no TLS) against an owner recorded when the seed resolves
(`native_handle::record_diagnostics_owner_thread` — its own word, not
piggy-backed on the handle-scheme capture, which a handle call could have won
first and left the gate open). The rate-1 verdict gets the same owner gate
plus a once-guard, so a worker can never `exit(70)` on non-final counts.
Regression test: `the_diagnostics_owner_gate_blocks_other_threads`.
