### CI: eleven post-merge gates were dark — #7205 relapsed on the arm #7856 created

`gate-freshness.yml` filed #7966 naming eleven gates with no recent successful `main`
run. The issue's own hypothesis was starvation. That is half right, and the half it
misses is a bug we have now shipped three times.

**The relapse.** Every scheduled gate carried
`group: <name>-${{ github.event_name }}-${{ github.event_name == 'push' && github.sha || github.ref }}`.
The `github.sha` arm was #7205's fix, and it is guarded on the event being `push` —
correct while the main-line arm *was* `push: branches: [main]`. #7856 moved that arm to
`schedule:`, the guard stopped matching, and the expression fell through to `github.ref`,
constant `refs/heads/main`. All scheduled runs of a gate shared one group again.

GitHub allows at most one PENDING run per concurrency group and cancels the previously
pending one when a new run enters, *regardless of `cancel-in-progress`* — the finding
#7205 was measured by. Observed 2026-08-12, identically on all ten gates: oldest run
`queued` holding the group (20h in the runner queue), the next two `cancelled` with
`jobs: 0`, newest `pending`. `gate-freshness` itself — the alarm, documented as built so
it "cannot be starved by the condition it is alarming about" — was cancelled the same
way. Concurrency groups are now keyed on `github.run_id` for every non-pull-request
event, so schedule / tag-push / dispatch runs can never supersede one another. PR runs
keep the shared per-ref group and keep coalescing.

`scripts/gc_gate_wiring_check.py` gains `check_schedule_group`, swept over all 31
workflow files rather than just the GC gates, since the hazard reached `gate-freshness`
and `test.yml`'s nightly safety net too. It found ten further workflows already carrying
the same latent constant group; all are fixed here. Five new self-test cases, the first
of which is the sabotage case — the existing CLEAN fixture *has* the bad shape, so a
checker that could not fail on it would be worthless. `lint` is a required context, so a
fourth relapse is a red build.

**The capacity half.** Measured queue: 1,529 runs queued against 12–14 concurrent. 794
were `pull_request` runs spread over 63 head branches — and 61 of those branches no
longer existed. GitHub does not reliably cancel a queued run when its PR merges and the
branch auto-deletes, so roughly 790 runs (51% of the entire queue) were dead work pinned
in front of ten six-hourly `main` gates. New `scripts/reap_stale_ci_runs.py` +
`ci-queue-reaper.yml` cancel QUEUED `pull_request` runs that have no open PR: dry-run by
default, `--max` cap, and structurally unable to touch a `push`, `schedule`, tag or
dispatch run. Keyed on open PRs rather than branch existence so fork PRs stay protected.
`zizmor`'s `push: main` arm is path-filtered to `.github/**` (the PR arm already was) and
gains the concurrency block it never had.

**What was actually unprotected.** gc-ratchet dark 33.6h across 37 collector-touching
merges; gc-root-dominance dark 33.6h across 44 codegen merges with an empty allowlist;
three more dark 57.3h. But the honest finding is that **gc-ratchet would not have caught
#7965 even had it run**: its gating metric set has no full-mark-sweep count, the counter
that found the regression (`collection_kind:"full"` 0 → 2) is not one of its metrics and
no gate in the repo ratchets one, the two dimensions that did move (wall time, RSS) are
explicitly `"gating": false` in the `shared_ci` profile CI uses, and its probe corpus is
not the gc-handoff workloads that showed it. That class stays uncovered; #7965's third
ask is still open.

Two of the eleven are not starved at all. `gc-native-roots` has **never had a successful
run** on any branch — three of four arms fail with three distinct causes. `llvm-inprocess`
failed its last three `main` runs and, worse, its PR "successes" show
`native-backend: skipped` — the path filter skips the only real job and the workflow
reports green. Both are filed separately; neither is fixed here.

Not closed: the reaper queues like everything else and cannot dig out an already-full
queue, so the first drain is a manual `--apply`. And `parity` / `compile-smoke` are
required contexts whose jobs never run on a pull request (`test.yml`'s `push:` is tags
only), so every merge needs an admin bypass — which is why pending gates stopped looking
unusual. That is branch-protection state, not a file in the tree.
