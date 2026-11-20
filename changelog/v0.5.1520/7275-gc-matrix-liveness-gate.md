### Fixed

**The GC stress matrix can now fail when an arm stops exercising the collector (#7255).**

`scripts/gc_repsel_matrix.sh` classifies a cell as `UNVER` when the output matched
the Node oracle but the arm was measurably inert, and that classification is
correct. What was missing is that `UNVER` could not turn a run red: the exit
status counted only `FAIL`. An arm that went inert across the *whole* corpus
therefore produced a yellow table and exit 0, which is CLAUDE.md's fourth way a
gate can be unable to fail — the job runs, and its subject never does.

That is not a hypothetical. #7024 measured the `default` arm at copy-minor
`0/22`, fixed it to `12/22`, and recorded the fix in the script header in its
strongest formatting. #7161 then flipped `PERRY_GC_MOVING_LOOP_POLLS`
default-OFF as a stopgap for #7154 — the gate for *both* halves of that route
(perry-codegen's `moving_safepoint_polls_enabled`, which decides whether
`js_gc_loop_safepoint` back-edge polls are emitted at all, and perry-runtime's
`gc_moving_loop_polls_enabled`, which decides whether the alloc-point nursery
trigger defers to them) — taking the same arm back to `0/50`. The header still
read `12/22`, because it was a hand-maintained number in a comment. Four of the
six PR-gating arms were inert, and #6982 and #7018 could not be judged in either
direction because both crash *inside* a copying minor that no longer ran.

Three changes, in the order the issue asked for them.

* **A relocating arm is back in the PR subset.** New arm `safepoint_minor`
  compiles *and* runs with `PERRY_GC_MOVING_LOOP_POLLS=1` and nothing else — no
  `PERRY_GC_INCREMENTAL=0`, no `PERRY_CONSERVATIVE_STACK_SCAN=off`, no forced
  evacuation. That is the sound route: the copying minor runs at a loop
  back-edge where roots are precise and rewritable, which is exactly what
  `default` was between #7024 and #7161 and what it becomes again when the
  stopgap lifts. It is in `PR_ARMS`.
* **The declarations are honest again, and self-healing.** `default`,
  `verify_evac`, `cons_scan_off` and `cons_scan_off_force` keep
  `requires=scavenge` — that is still what they are *for* — and are listed in the
  new `test-parity/gc_matrix_inert_arms.txt` with the issue that blocks them.
  Registering an arm is the only way to be inert without failing.
* **`scripts/gc_matrix_liveness_check.py` runs at the end of every matrix
  invocation** and fails it when an unregistered arm satisfied its `requires=` on
  zero cells — *and* when a registered arm starts biting again, so the registry
  cannot rot the way the header did. It has `--self-test` and `--check-registry`
  modes wired into `lint`, so the rule that decides red-versus-green is a tested
  program rather than untested bash, and it is verified to be *wired* (the
  registry check fails if the matrix stops calling it).

No liveness number is written down anywhere any more. The per-arm table is
derived from the collector's own `PERRY_GC_TRACE` / `PERRY_GC_DIAG` output and
printed on every run.
