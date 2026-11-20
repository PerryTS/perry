### Changed

- **Documented exactly what `[gc-step-bounds]`'s `weak_steps_sliced` proves
  (#7903 follow-up).** Comment-only; no behaviour change.

  The sabotage run that proved #7938's new tests can fail also falsified the
  reason given for believing them. Reverting the slice to the pre-#7903
  whole-array-in-one-unit behaviour turns 4 of the 5 tests red — but **not** via
  the liveness assertion, which #7938 claimed was unreachable on unsliced code.
  It is reachable: a step can end "mid-registry" at the *entry park*, its budget
  already spent resolving the holder, before a single record is scanned. What
  actually caught the sabotage was the per-step ceiling, failing with
  `charged 64`.

  So the counter is asymmetric — **zero proves the sliced path did not run;
  nonzero does not prove records were sliced** — and `weak_max_records_per_step`
  is the discriminating quantity. Stated at all three places a reader meets it:
  the `[gc-step-bounds]` doc block, the assertion itself, and
  `docs/src/internals/gc-step-bounds.md`.
