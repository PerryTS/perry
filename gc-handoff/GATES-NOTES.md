# Gate starvation investigation — issue #7966

Measured 2026-08-12 ~15:00Z from `origin/main` @ a769fafc6.

## Headline

The #7856 fix (move the ten expensive gates' main-line arm from `push: branches:[main]`
to a staggered six-hourly `schedule:`) was correct in its diagnosis and **re-introduced
#7205 on the arm it created**. The gates are now dark for a *different* reason than the
issue text assumes.

## Measured queue state

| metric | value |
|---|---|
| queued runs | **1,529** |
| in-progress runs | 14 |
| queued by event | 794 `pull_request`, 181 `push`, 19 `schedule` |
| distinct head branches among queued PR runs | 63 |
| **of those branches still existing on the remote** | **2** |
| open PRs | 8 |
| lifetime success / failure / cancelled | 19,824 / 6,482 / **15,283** |
| last 100 completed runs | **100 cancelled, 0 executed** |

**~790 of 1,529 queued runs (51%) are for 61 branches that no longer exist** — PRs that
already merged and auto-deleted. GitHub does not reliably cancel queued runs on branch
deletion, so they hold runner slots ahead of the scheduled `main` gates forever.

## The #7205 relapse (this is the fixable bug)

Every one of the ten gates carries:

```yaml
group: <name>-${{ github.event_name }}-${{ github.event_name == 'push' && github.sha || github.ref }}
cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

The `github.sha` arm was #7205's fix, and it keys on the event being **`push`**. #7856
then moved the main-line arm to **`schedule`**, which falls through to `github.ref` —
constant `refs/heads/main` for every scheduled run. So all scheduled runs of a gate
share one group again.

Per the repo's own measured finding (quoted in gc-ratchet.yml:63-67): GitHub allows at
most one PENDING run per group and cancels the previously pending one when a new run
enters, *regardless of `cancel-in-progress`*.

Observed, identically across all ten gates:

```
2026-08-12T13:37Z schedule pending     <- newest, blocked on the group
2026-08-12T07:46Z schedule cancelled   <- jobs: 0  (never reached a runner)
2026-08-12T02:37Z schedule cancelled   <- jobs: 0
2026-08-11T19:15Z schedule queued      <- holds the group, 20h in the runner queue
```

`jobs: 0` is the exact zero-execution signature #7205 was measured by. The oldest run
holds the group and is stuck behind the 1,529-deep queue; every newer scheduled run is
cancelled on arrival. **The gate cannot run again until that one run drains.**

`Gate Freshness` itself was cancelled at 2026-08-12T13:38Z — same shape
(`gate-freshness-${{ github.event_name }}-${{ github.ref }}`). The alarm designed so it
"cannot be starved by the condition it is alarming about" is now starved by it.

## Verdict per cause

- **#7205 relapse on the schedule arm** — 10 gates + gate-freshness. Fixable in one line each.
- **Capacity / zombie queue** — 51% of the queue is dead PR work. Needs a reaper.
- Neither is "the gate is broken". No gate content is at fault.

---

## Per-gate verdict (the issue assumed one cause; there are three)

| gate | verdict | evidence |
|---|---|---|
| gc-ratchet | STARVED + #7205 relapse | sched runs: oldest `queued` 20h, two `cancelled` w/ `jobs: 0`, newest `pending` |
| gc-root-dominance | STARVED + relapse | same shape |
| tls-budget | STARVED + relapse | same shape |
| gc-ptr-shape-off-witness | STARVED + relapse | same shape |
| gc-parse-churn-gate | STARVED + relapse | same shape |
| gc-moving-witnesses | STARVED + relapse | same shape |
| auto-opt-app-patterns | STARVED + relapse | same shape |
| eh-transport | STARVED + relapse | same shape |
| security-audit | STARVED | 90 queued `push`/`main` runs; required context, so merges bypass |
| **gc-native-roots** | **BROKEN** | **never had a single successful run, any branch, any event.** 3 of 4 arms fail with 3 distinct causes: aarch64-linux SIGSEGV (139) under `PERRY_STACKMAP_WALKER=verify`; windows Rust panic (101); macos-14 `gc_evacuation_liveness_assert.py` reports 0 copying minors / 0 objects copied |
| **llvm-inprocess** | **BROKEN + VACUOUS-GREEN** | last 3 `main` runs `failure`. Worse: sampled PR "successes" show `changes=success, native-backend=skipped` — the path filter skips the only real job and the workflow reports green. Hazard 4 |

## Structural finding not in the issue: required contexts that can never pass

Required contexts on `main` are:
`lint, cargo-test, parity, compile-smoke, api-docs-drift, security-audit, conformance-smoke-complete`

`parity` and `compile-smoke` carry
`if: github.event_name == 'push' || (workflow_dispatch && inputs.run_extended_tests) || (pull_request && contains(labels, 'run-extended-tests'))`
and `push:` in test.yml is **tags only**. So on an ordinary PR they never report at all, and a required context that never reports blocks the merge button forever. Meanwhile `security-audit` sits `queued`.

**Consequence: every merge needs an admin bypass**, which bypasses the required contexts that DO work. This is why ~20 PRs merged today with checks pending, and it is upstream of the whole incident: bypass is the normal path, so nothing about a pending gate looks unusual. Fixing this is a branch-protection edit (server-side state, not in the tree) and is NOT in this PR.

## What was unprotected, and what got through

- **#7965 (2.2-4.8x regression)** — introduced by #7902 (`1bd5eeb6b`), merged in #7944 at 10:17Z, fixed by #7968 at 14:27Z, ~4.2h on main.
  **gc-ratchet would NOT have caught it even if it had run.** Its gating metric set has no full-mark-sweep / major-cycle count; the counter that found the bug (`collection_kind:"full"` 0 -> 2) is not a gc-ratchet metric, and no gate in the repo ratchets one. The two dimensions that did move (wall time, RSS) are explicitly `"gating": false` in the `shared_ci` profile CI uses. The workloads (`retain`, `deeplist`) are the gc-handoff corpus, not gc-ratchet's. **This regression class is structurally uncovered by all 11 gates**, independent of the starvation. #7965's ask #3 (gate on the full count) is still open.
- **#7843's seven red rows** — retired by #7921 re-pinning the baseline, not by a code fix. The re-pin was measured at `98e9ecdb5`, which contains #7888 but NOT #7901/#7902 — so `changelog.d/7921-gc-ratchet-repin.md`'s "the merged bounded untraced-promotion changes" is inaccurate. **19 collector-touching PRs merged after that pin with zero gc-ratchet runs completing; whether the new pin holds on current main is unmeasured.**
- **gc-ratchet dark 33.6h** across 37 collector-touching merges. **gc-root-dominance dark 33.6h** across 44 codegen merges with an EMPTY allowlist (every new hit is meant to be a red build). **gc-moving-witnesses / gc-parse-churn-gate / gc-ptr-shape-off-witness dark 57.3h.**

## Capacity math

- Arrival: ~58 merges/day, each PR fanning out to ~11 workflows, plus a per-merge `push` arm for security-audit + zizmor.
- Drain: 12-14 concurrent runs observed; lifetime cancelled (15,283) is approaching lifetime success (19,824).
- Standing queue 1,529 with ~790-1,200 of it dead PR work.

Reaping dead PR runs is worth ~51% of the queue immediately and is the only lever that does not trade away coverage. The six-hourly schedule (#7856) already cut ~19 jobs/merge; it cannot help further while the queue in front of it is half garbage.

## What this PR changes

1. **Concurrency group keyed per RUN for non-PR events** across 22 workflows + the new one. `${{ github.event_name == 'pull_request' && github.ref || github.run_id }}`. PR coalescing preserved; main-line runs can no longer supersede each other.
2. **`scripts/gc_gate_wiring_check.py` gains `check_schedule_group`**, swept over ALL 31 workflows (not just the GC gates -- the hazard took out `gate-freshness` too). Requires `github.run_id` in the group of any workflow with a `schedule:` trigger. 5 new self-test cases incl. the CLEAN fixture as the sabotage case. `lint` is required, so a fourth relapse is now a red build.
3. **`scripts/reap_stale_ci_runs.py` + `ci-queue-reaper.yml`** — cancels QUEUED `pull_request` runs with no open PR. Dry-run by default, `--apply` to act, `--max` cap, self-tested incl. two sabotage cases. Only `event==pull_request` + `status==queued` + no open PR; a push/schedule/tag/dispatch run is structurally unreachable.
4. **zizmor**: `push: main` arm path-filtered to `.github/**` (it was unfiltered while the PR arm was already scoped), plus the concurrency block it never had.

## NOT closed by this PR

- The **bootstrap**: the reaper queues like everything else, so it cannot dig out an already-saturated queue. First drain must be a manual `python3 scripts/reap_stale_ci_runs.py --apply`. I did not run it -- it cancels ~780 runs on shared infrastructure and is a maintainer call.
- **Branch protection**: `parity` / `compile-smoke` required-but-never-reporting. Server-side; admin only.
- **gc-native-roots** and **llvm-inprocess** are broken, not starved. Filed separately.
- **No gate ratchets collection KIND.** #7965's class stays uncovered.
