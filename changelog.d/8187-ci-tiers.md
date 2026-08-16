### CI restructured into three tiers: PR gate / main sweep / full suite (#8187)

**Why.** CI could not gate a merge. Measured 2026-08-16 over ~24 h of runs: the org
runs on 20 concurrent hosted jobs (5 macOS) and every PR push fanned out to 14
workflows / 48 jobs / ~650 runner-minutes; at ~66 pushes and ~58 merges a day that
is 1.5–2× total capacity. Job queue waits were 3–7 h, **0 of 66** PR runs of `Tests`
reached a conclusion, and the last 12 merges all bypassed branch protection with
`lint`/`cargo-test` still queued, `conformance-smoke-complete` red on `main`
(#8117) and two required contexts (`parity`, `compile-smoke`) that never ran on a PR
at all. `conformance-smoke` alone was 480 job-minutes per push, 96 % of it the
auto-optimize path rebuilding a feature-stripped runtime per feature set,
redundantly in every shard; sccache wrote ~200 GB/day of PR-scoped tarballs (which
other PRs cannot even read) into a 10 GB cache budget; `cache-warm.yml` had not
completed since 07-28.

**What.** One workflow (`test.yml`), one policy file (`scripts/ci_plan.py`,
self-tested; `--table` prints the job × tier matrix and `lint` keeps the docs copy
current), one fan-in status per tier:

- **pr** (every PR push): `lint` (now also the changeset step), `check` (clippy ×2 +
  api-docs-drift), `warnings`, scoped `cargo-test`, the gap suite in **6 shards of
  the harness's `fast` mode** (`PERRY_SKIP_BUILD=1`, one prebuilt release compiler,
  ~1.5 s/test), `gc-stress` (PR subset), `e2e-scoped`, and `security-audit` only when
  a lockfile/manifest/policy file changed. Docs-only PRs run `lint` only. Fan-in
  **`pr-gate` is the single required status context** — adding/removing a job never
  needs a branch-protection edit again, and a docs-only PR still gets a verdict (no
  more `paths-ignore` wedge).
- **sweep** (every push to `main`, coalesced via a constant concurrency group with
  `cancel-in-progress: false`): the PR tier unscoped plus Windows x64/ARM64 builds,
  full `gc-stress`, `compiler-output-regression`, `repsel-census`, `harmonyos-smoke`,
  `security-audit`. Sweep-only jobs chain behind `check` so a merge
  does not take every runner slot. Fan-in `main-gate`. This is also the
  cache-producing build on `main` (rust-cache + sccache save here), replacing
  `cache-warm.yml`.
- **full** (nightly, `v*` tags, `workflow_dispatch`, PRs labelled `run-extended-tests`):
  the sweep plus `parity`, `compile-smoke`, the 8-shard auto-optimize gap suite,
  `doc-tests`, `binary-size`, the drizzle/ink/effect smokes and
  `native-abi-evidence-packet` —
  **all without `continue-on-error` now**. Fan-in `full-suite-gate`;
  `release-packages.yml` dispatches `tier=full` and accepts only a run whose
  `full-suite-gate` succeeded on the release SHA.

The twelve satellite gates (`gc-*`, `tls-budget`, `auto-opt-app-patterns`,
`eh-transport`, `llvm-inprocess`, `ext-link`, `container-tests`) keep their
six-hourly / nightly `main` sweeps and tag arms; their PR arm is now **opt-in via
the `run-extended-tests` label** (job-level `if:`, so an unlabelled PR costs no
runner slot). `ext-link` gains a nightly arm (it had no main-line arm); `container-
tests` drops its `push: main` arm. `security-audit.yml` becomes `workflow_call` +
weekly. sccache saves only from main-line runs; PRs restore the newest main-line
blob.

**Follow-up (admin, after merge):** set the required status checks on `main` to
`pr-gate` only. Full page: `docs/src/testing/ci-tiers.md`.
