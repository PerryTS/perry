### CI: the post-merge gates were starved, not broken — rescheduled, plus an alarm for the next time (#7856)

Ten heavy gates ran on `push: branches: [main]`. Fourteen workflows fired on every
merge, ~29 jobs each time, against a repo that runs ~9 jobs concurrently. At the
cadence `main` reached (10 pushes/day on 2026-08-09 → 32 → **58** on 08-11) demand
outran drain, the queue grew without bound, and the entries that aged out were the
`main` runs — precisely the ones that gate nothing, so nobody watches them.
`gc-ratchet` had **22 of its last 25 `main` runs queued** (oldest 9h18m) and had not
succeeded on `main` since 2026-08-09, across five collector-touching merges (#7799,
#7809, #7812, #7834, #7839). #7843's seven genuinely-red rows landed inside that
blind window.

This is a **third variant of CLAUDE.md's "four ways a gate can be unable to fail"**:
not `continue-on-error`, not missing from required contexts, not cancelled —
**starved**.

**The constraint is total Actions concurrency, not macOS capacity**, which was the
natural reading and is wrong. At the moment of measurement the queue held **45
`ubuntu-latest` jobs against 14 `macos-14`**, and `zizmor` (ubuntu) was queued in the
same second as `gc-ratchet` (macOS). Two claims in #7856 do not survive checking, and
both mattered: `gc-root-dominance` runs `macos-14`, not `ubuntu-latest` — its
healthy-looking run count was *pull-request* runs, which drain because they supersede
each other, while its `main` arm queued like the rest. So the ubuntu-vs-macOS contrast
that localised the problem to macOS was comparing a PR arm against a `main` arm.
**Rebalancing pools cannot help; only cutting total demand can.** (Moving `gc-ratchet`
to Linux was never available anyway: its baseline is keyed `darwin-arm64` and the
checker refuses a platform mismatch rather than comparing incomparable numbers.)

The `concurrency:` blocks are **left untouched** — they are already correct and
twice-repaired (#7205). That fix worked; the failure mode simply moved from
*cancelled* to *never scheduled*.

**Change.** The post-merge arm of ten gates (`gc-ratchet`, `gc-root-dominance`,
`tls-budget`, `gc-native-roots`, `gc-ptr-shape-off-witness`, `gc-parse-churn-gate`,
`gc-moving-witnesses`, `auto-opt-app-patterns`, `eh-transport`, `llvm-inprocess`)
becomes a staggered six-hourly sweep of `main` plus release tags. That removes **19
jobs from every merge** — ~1,100 job-starts/day replaced by ~76, a **93% cut** on this
slice. Cron minutes are staggered and none sits at `:00`, so the ten do not re-create
the herd they were meant to relieve. Pull-request arms are unchanged, so every PR is
still measured before it can merge, and no probe, threshold, baseline or matrix cell
moved. `eh-transport` and `llvm-inprocess` also needed their relevance guard changed
from `= "push"` to `!= "pull_request"`; under a `schedule` event the old test fell
through to the PR branch and would dereference an empty PR number.

**The cost, stated plainly:** attribution latency. A regression that slips past the PR
arm is now named against a window of commits rather than one (bisect `previous sweep
SHA .. this sweep SHA`). Four completed runs a day beat 58 that never start.

**Alarm.** `gate-freshness.yml` + `scripts/check_gate_freshness.py` close the hole that
let this last two days: an empty result set is indistinguishable from a healthy one
nobody checked. It fails when a gate has no successful **post-merge** `main` run inside
its budget (`scripts/gate_freshness.json`) and maintains one self-closing sticky issue.
`pull_request` runs are deliberately not counted as evidence of health — counting them
is exactly what made `gc-root-dominance` look fine while it was dark. The checker is
**sabotage-tested, not merely exercised**: `--self-test` plants a stale gate, a gate
with no successful run at all, and a gate whose only recent successes are PR runs, and
asserts the verdict for each; three independent sabotages of the detector were each
caught with a specific message. Its first live run found two things outside the issue's
scope — **`llvm-inprocess` had been dark for 171.8h**, and **`security-audit`, a
required context left on every merge, was itself stale at 17.4h**.

Full measurement, the refuted claims, and what was deliberately not changed:
`docs/src/testing/ci-gate-scheduling.md`.
