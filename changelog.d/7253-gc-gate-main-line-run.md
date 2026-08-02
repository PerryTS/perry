### CI

- **The moving-GC matrix now runs on `main`.** `gc-stress` carries
  `scripts/gc_repsel_matrix.sh`, the only CI execution of the `requires=move`
  allocation-point arms over the representation corpus, and it had **no
  main-line run at all**. `test.yml`'s `push:` trigger is tags-only ("Direct
  pushes to main do NOT trigger tests"), so `push` in the job's `if:` only ever
  meant a release tag; and the nightly cron — which the same file calls "the
  only backstop for integration-suite regressions a scoped PR run can't see" —
  fires as `schedule`, which the `if:` did not list. Twelve consecutive nightly
  `main` runs reported `gc-stress` as `skipped`. Between release tags nothing
  adjudicated the matrix on `main`, which is how `test_gap_repsel_p4a3_ptr_numarray`
  (#7194) stayed red on ten arms for over a week and forced three separate PRs
  to hand-exonerate the same seventy cells. `schedule` is now in the `if:`.

- **New `lint` gate: `scripts/gc_gate_wiring_check.py`.** Asserts, for each of
  the four moving-GC gate jobs (`gc-stress`, `gc-moving-witnesses`,
  `gc-root-dominance`, `gc-ratchet`), that it executes on main-line code, carries
  no job-level `continue-on-error`, does not swallow its gating step's exit
  status, and does not let a new merge cancel the previous `main` run — CLAUDE.md's
  four ways a gate can be unable to fail, mechanised. A **tag-only** run does not
  count as main-line: a gate that speaks only at release time cannot name the
  merge that broke it. The checker fails on the pre-fix tree, naming `gc-stress`,
  and passes after; `--self-test` covers seven cases. It reports, and cannot
  enforce, that none of these jobs is in branch protection's required contexts.

### Fixed (verification only — no product change)

- **#7194 `test_gap_repsel_p4a3_ptr_numarray` is closed as fixed by #7249.**
  Bisected across the five GC/codegen merges that followed the issue's last
  measurement, one build per hop and ten runs each: red at `c9cd73ba5`,
  `8b024958f` (#7242), `c7893c4ac` (#7250) and `061b24163` (#7252); green at
  `64c1f56fb` (#7249, the globalThis bootstrap no-move window). The base
  reproduction is byte-identical to the issue's evidence, including the
  `scavenged=6514`/`6515` split across arm groups; the failure was one lost
  `counts[v]++` out of 5000, deterministic 10/10. Because #7249 changes *where*
  minor #0 lands, the head build collects less than the failing base did
  (`cycles=1 scavenged=3585` vs `cycles=2 scavenged=6515`), so green was
  re-established *above* the base's movement rather than below it: under loop
  polls + `PERRY_GC_ZEAL=1` the test survives **14 373 evacuating minors**
  byte-exact, 10/10.
