### CI: the moving-GC stale-root witnesses can now fail

`test-files/test_gap_gc_*.ts` are reproducers, not parity tests — each was written
for a specific stale-root defect (#6981, #7114, #7154, #7192, #7200/#7201/#7202,
#7206, #7208/#7209, #7214, #7216) and each is clean on the shipped default by
construction. Since #7161 flipped the evacuating minor default-OFF, the bug they
reproduce is only expressible under `PERRY_GC_MOVING_LOOP_POLLS=1` at compile
**and** run time, and no CI job ran them that way: `gc-root-dominance` compiles
with the flag but never runs a program, `gc_instrument_smoke.sh` runs with it
against its own fixture, and `gc-stress`'s PR arm subset contains no arm that
compiles with it. They went green on every PR while proving nothing.

New non-required `gc-moving-witnesses` workflow runs
`gc_repsel_matrix.sh --arms loop_polls --filter test_gap_gc_` and then rejects
`UNVER` as hard as `FAIL` — on a `requires=move` arm an inert cell means nothing
relocated, so the witness could not have failed. Proven red-then-green by
reverting #7214's two production codegen files: 15/15 PASS → 12 PASS/3 FAIL on
exactly #7214's three witnesses → 15/15 PASS restored.

Found two witnesses that were registered nowhere and therefore ran nowhere:
`test_gap_gc_new_instance_rooting` (#7192) and
`test_gap_gc_assign_string_source_rooting` (#7216). Both registered; the second
surfaced a pre-existing #7217 red on the allocation-point arms (a sharper
reproducer than the one that issue names), triaged with measurements.

Also fixes #7205: `gc-ratchet` and `gc-root-dominance` key their push-event
concurrency group on the commit. `cancel-in-progress: false` does not protect a
*queued* `main` run — GitHub allows one pending run per group and cancels the
previous one — which is why `gc-ratchet` executed zero times across three merges.
