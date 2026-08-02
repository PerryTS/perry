Four PRs in a row shipped a test file that ran nowhere. #7192 and #7216 each
added a `test_gap_gc_*` stale-root witness and no `test-parity/gc_repsel_corpus.txt`
line; #7252 added a third; #7270/#7271 added two more, caught by hand at merge.
An unregistered file is not a failing test, it is no test at all — the PR goes
green having run it zero times.

Registration checks existed for two of the three prefixes and neither could
catch the pull request that needed it: `gc_repsel_matrix.sh` and
`gc-moving-witnesses.yml` both sit behind a 90-minute release build, behind a
changed-paths relevance filter, and in workflows that are not in branch
protection.

`scripts/check_test_registration.py` is the cheap half, pulled out to where it
can block and generalised past that one corpus. Pure filesystem and text (no
compiler, no Node, ~0.2s) over four mechanisms: the GC/repsel corpus,
`test-features/feature_matrix.toml`, `benchmarks/compiler_output/workloads.toml`,
and Rust test files below a suite root, which compile only if a `mod`
declaration names them (rustc never parses an undeclared one — not dead code,
not code, no warning).

It runs in `lint`, which is ALREADY a required context, so no branch-protection
change is needed. That placement is the point: forgetting to promote a new job
is CLAUDE.md hazard 2, and it is what left `gc-root-dominance` red and blocking
nothing for days.

The step carries `if: ${{ !cancelled() }}`, which is hazard 4 wearing a costume
this repo has not named yet: `lint` is a SEQUENCE of unrelated gates, and one
failing step takes every later step to `skipped`. Not hypothetical — `Public
benchmark evidence freshness` has failed on `main` on every run from 2026-07-29
onward, so `File size limit`, `GC store-site inventory`,
`Address-classification audit`, `Gap snapshot checker self-test` and
`Platform-aware parity allowlist self-test` have all been skipped for days while
the job reported red for an unrelated reason. The five steps above deserve the
same treatment; that is a separate change from this one, and the stale public
benchmark artifact needs regenerating either way.

Built so it cannot pass vacuously: each mechanism floors its candidate set and
fails if the glob stops matching, and every run prints
`checked N files against M registries`. Exclusions are named with reasons rather
than counted (a threshold cannot tell a new dark file from an old one), and a
stale exclusion or a registry entry whose file is gone both fail. `--self-test`
(39 cases, also run in `lint`) plants an unregistered file into each mechanism
over the real registries, asserts the gate names it, then removes it and asserts
green. It also asserts that excluding the planted file clears it and that a
deleted or renamed registry fails by name instead of crashing with a raw
`FileNotFoundError`.

Zero dark files today across all four mechanisms, so it is green on `main` from
the first run. Five candidates are excluded with reasons: four helper modules
imported by a registered test, and `raw_numeric_layout_smoke.ts`, which is
registered in a different registry (the `raw_numeric_layouts` target-collector
workload in `scripts/run_memory_stability_tests.sh`).

Out of scope and said out loud in `--list`: `tests/*.sh|py|ts`, where 143 of 171
files are referenced by nothing in the tree. That has no registry to diff
against and needs per-file triage, not a gate.

The rule is documented in a new `docs/src/testing/test-registration.md`, in
CONTRIBUTING.md, and in each of the three registry files' own headers.
