# CI gate scheduling: why the heavy gates sweep `main` instead of gating every merge

This page explains one deliberate choice: **the expensive post-merge gates run on a
staggered six-hourly schedule against `main`, not once per merge.** Their
pull-request arm is untouched — every PR is still measured before it can merge.

If you are about to "fix" one of those workflows by putting `push: branches: [main]`
back, read this first. That trigger is what broke them.

## The failure: starvation (#7856)

On 2026-08-11, **every** heavy gate in this repo had produced zero results on `main`
for over two days. They were not failing and not cancelled. They never reached a
runner.

The measurement that matters is not "macOS is slow". It is the ratio between how
many jobs a merge enqueues and how many the repo can run at once:

| quantity | measured 2026-08-11 15:07 UTC |
|---|--:|
| workflow runs created that day (by 15:07) | **600** (392 `pull_request` + 208 `push`) |
| pushes to `main` per day | **10** (08-09) → **32** (08-10) → **58** (08-11) |
| workflows triggering on `push: main` | **14**, totalling ~**29 jobs per merge** |
| jobs running repo-wide, right then | **9** (6 `ubuntu-latest`, 3 `macos-14`) |
| runs queued repo-wide, right then | **100+** |
| `gc-ratchet` `main` runs queued, of its last 25 | **22**, oldest waiting **9h18m** |
| `gc-ratchet` last successful `main` run | **2026-08-09**, two days earlier |

Reproduce the core of it with:

```bash
# What is actually running, repo-wide, at the job level:
gh api "repos/PerryTS/perry/actions/runs?status=in_progress&per_page=100" \
  -q '.workflow_runs[].id' \
| while read id; do
    gh api "repos/PerryTS/perry/actions/runs/$id/jobs?per_page=100" \
      -q '.jobs[] | select(.status=="in_progress") | (.labels|join(","))'
  done | sort | uniq -c

# How deep the queue is, and on which pools:
gh api "repos/PerryTS/perry/actions/runs?per_page=100" -q '.workflow_runs[].id' \
| while read id; do
    gh api "repos/PerryTS/perry/actions/runs/$id/jobs?per_page=100" \
      -q '.jobs[] | select(.status=="queued") | (.labels|join(","))'
  done | sort | uniq -c
```

Demand outran drain, the queue grew without bound, and the oldest entries — the
`main` runs, which are exactly the ones that gate nothing and therefore nobody is
watching — aged out.

## What this is NOT

**It is not the concurrency bug, and it is not macOS runner availability.** Both
were the obvious reading, and both are wrong. Getting this right matters, because
each wrong reading has a "fix" that would make things worse.

**Not the concurrency block.** `gc-ratchet.yml`'s `concurrency:` comment records two
prior attempts (#7205): a shared group with unconditional `cancel-in-progress`
cancelled three consecutive `main` runs, and scoping `cancel-in-progress` to pull
requests did not fix it either, because GitHub allows at most one *pending* run per
group. Keying the group on `github.sha` for push events **did** fix cancellation.
The failure mode simply moved: runs stopped cancelling each other and started
queueing forever instead. **Those blocks are correct. Do not "fix" them again.**

**Not macOS capacity.** This was the natural inference — the gates that went dark
are the macOS ones — but the job-level numbers refute it. At the moment of
measurement the queue held **45 `ubuntu-latest` jobs against 14 `macos-14` jobs**,
and `zizmor` (`ubuntu-latest`) was queued in the very same second as `gc-ratchet`
(`macos-14`). Ubuntu was starved harder in absolute terms.

Two specific claims in #7856 do not survive checking, and are recorded here so the
next person does not re-derive them:

- **`gc-root-dominance` does not run on `ubuntu-latest`.** It runs `macos-14`, in
  two jobs. Its healthy-looking run count was pull-request runs, which drain because
  they supersede each other; its `main` arm was queued like all the others. The
  ubuntu-vs-macOS contrast that localised the problem to macOS was comparing a PR
  arm against a `main` arm, not Linux against Darwin.
- **Moving `gc-ratchet` to Linux is not available as a remedy.** Its baseline is
  captured under the `darwin-arm64` platform key and the checker *refuses* a
  platform mismatch rather than comparing numbers that are not comparable. Moving it
  would turn the gate red, not relieve it. `tls-budget`'s macOS arm is likewise
  irreducible: `_tlv_get_addr` is a Mach-O artefact and the measurement does not
  exist elsewhere.

The consequence of both corrections is the same: **rebalancing pools cannot help.**
Only reducing total job demand can. That is what this change does.

## The change

For ten heavy gates, the post-merge arm became a staggered six-hourly sweep plus
release tags:

```yaml
on:
  pull_request:          # unchanged — every PR is still measured
  schedule:
    - cron: "7 */6 * * *"   # staggered; see the table below
  push:
    tags: ["v*"]         # releases stay individually gated
  workflow_dispatch:
```

Cron minutes are staggered so the ten do not re-create the thundering herd they were
meant to relieve, and none sits at `:00`, where GitHub's scheduler is most contended
and most likely to delay a run:

| workflow | cron | runner of the heavy job |
|---|---|---|
| `gc-ratchet` | `7 */6 * * *` | `macos-14` |
| `gc-moving-witnesses` | `12 */6 * * *` | `ubuntu-latest` |
| `gc-root-dominance` | `17 */6 * * *` | `macos-14` (×2) |
| `auto-opt-app-patterns` | `22 */6 * * *` | `ubuntu-latest` |
| `tls-budget` | `27 */6 * * *` | `macos-14` |
| `eh-transport` | `32 */6 * * *` | `macos-15` |
| `gc-native-roots` | `37 */6 * * *` | matrix |
| `llvm-inprocess` | `42 */6 * * *` | `macos-15` |
| `gc-ptr-shape-off-witness` | `47 */6 * * *` | `ubuntu-latest` |
| `gc-parse-churn-gate` | `57 */6 * * *` | `ubuntu-latest` |

That removes **19 jobs from every merge** (counting matrix expansion). At the cadence
measured above that is ~1,100 job-starts/day of demand replaced by ~76 — a **93% cut
on this slice**, which is what lets the remaining queue drain.

### What was deliberately left alone

- **`security-audit`** is a *required* status context and stays on every merge.
- **`zizmor`** and **`cache-warm`** are single cheap ubuntu jobs; `cache-warm` is
  what makes everything else fast.
- **`container-tests`** (6 jobs on every merge, ~350 job-starts/day) is the
  next-largest lever but is not a GC gate; left for a maintainer decision.
- **Every gate's actual content** — no probe, threshold, baseline, or matrix cell
  was touched. This change alters *when* the post-merge arm runs, nothing else.
- **The in-job relevance filters** were left where they are. Hoisting them to
  `on.pull_request.paths` would save PR-side slots, but a path-filtered workflow
  reports *no* status rather than a passing one, which can wedge a required context.
  Not worth the risk here.

### The cost, stated plainly

**Attribution latency.** A regression that slips past the PR arm used to be pinned
to one commit; now the next sweep names it against a window of commits. Each sweep
prints the SHA it tested, so the window is `previous sweep SHA .. this sweep SHA` —
bisect within that. In exchange the gate produces an answer at all, which for the
two days before this change it did not. Four completed runs a day beat 58 that never
start.

## Staleness alerting

The starvation was silent *by construction*: an empty result set looks exactly like
a healthy one nobody checked. Rescheduling the gates does not fix that — a cron that
silently stops firing fails the same way.

`gate-freshness.yml` runs hourly on `ubuntu-latest` and calls
`scripts/check_gate_freshness.py`, which asks the Actions API for each gate's most
recent **successful** non-PR run on the default branch and fails when it is older
than that gate's budget in `scripts/gate_freshness.json`. On failure it opens — or
updates, never duplicates — a single sticky issue, and closes it once every gate is
fresh again.

```bash
python3 scripts/check_gate_freshness.py --self-test   # proves it can still fail
python3 scripts/check_gate_freshness.py --dry-run     # real API, no issue writes
```

Budgets are the schedule interval plus headroom for a 90-minute job and a queue that
is still draining. A gate whose budget you have to keep raising is a gate that is
still starving; raise the *capacity* or lower the *demand* instead.

**The checker is sabotage-tested, not merely exercised.** `--self-test` plants a
stale gate, a fresh gate, a gate with no successful run at all, and a gate whose only
recent success is a `pull_request` run (the exact shape that made `gc-root-dominance`
look healthy while its `main` arm was dark), and asserts the verdict for each. A
green `--self-test` means the detector works, not that nothing was tried.
