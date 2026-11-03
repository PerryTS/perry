# Test Structure

> **Status: proposal.** This page is the target structure for Perry's program
> tests and the order in which it lands; the tools it names arrive with the
> pull requests that implement each step.
>
> **A test's subject decides where it lives; one runner judges whether it
> passes; a pull request runs the subjects it touches.** Today the directory
> decides which gate protects a test, so every subject crowds into the one
> directory that gates pull requests.

## Where a test goes

Ask what the assertion needs to see.

| The assertion is… | It is… | It lives in |
|---|---|---|
| what a program prints or exits with, and Node can produce it | a `.ts` program diffed against Node | `test-parity/<subject>/` |
| the same, but only meaningful under a GC mode | a `.ts` program run by the GC matrix | `test-parity/<subject>/`, modes declared in its header |
| what Perry prints for a Perry-only API | a `.ts` program with committed expected output | `test-parity/perry/<api>/` |
| an internal invariant: rooting, layout, ordering, allocation, private state | a Rust unit test | beside the code |
| the shape of emitted IR or lowered HIR | a Rust test | beside the lowering code where it can be; `crates/<crate>/tests/` when it needs the crate's public API |
| a diagnostic, stderr, a compile failure, CLI output, determinism | a Rust test that runs the `perry` binary | `crates/perry/tests/` |
| ECMAScript spec conformance | a Test262 case | the pinned corpus; never hand-written |

A Rust test whose only assertion is stdout that Node prints identically is a
parity case in the wrong language. A parity case built to observe something the
runtime keeps internal is a unit test in the wrong suite. `crates/*/tests/`
suites run on a pull request only when the diff selects them, so coverage that
must gate every change belongs beside the code or in `test-parity/`.

## Layout

```text
test-parity/
  node/<module>/          one Node core module, named as Node names it
  language/<area>/        JavaScript semantics: object, string, json, regex,
                          promise, class, iterator, bigint, intl, …
  typescript/<feature>/   TypeScript-only syntax: enum, namespace, decorators,
                          parameter properties, …
  perry/<api>/            Perry-only APIs and globals
  npm/<package>/          a package compiled through perry.compilePackages
  expected/<same path>    committed output where Perry intentionally differs
```

A case that exists to drive an optimization path still lives under the
subject whose behavior it observes. Its purpose belongs in its header, not in
its directory.

`expected/` mirrors the test's path. A lookup by basename lets two subjects
with the same case name overwrite each other.

## One runner

The target is one runner, `scripts/parity.py`, for every program case. It
takes subjects or paths, runs cases in parallel, and judges each one the same
way: compiled, run, and compared with Node on stdout, stderr and exit status
after one normalization profile. The compile mode is a flag: `--mode fast`
against a prebuilt compiler, `--mode auto` with auto-optimize, which is what
users ship.

Each subject carries one snapshot per platform and mode of its cases that do
not pass. The snapshot is checked in both directions: a new failure is red,
and so is a listed case that starts passing. A crash is a defect, never a
snapshot entry.

| Replaced | By |
|---|---|
| `run_parity_tests.sh`, `run_gap_tests.sh`, `node_suite_run.py` | `parity.py run` |
| `gap_snapshot.json`, `gap_snapshot.py` | per-subject snapshots, `parity.py run --update` |
| `known_failures.json` and `parity_known_failures.py` | the same snapshots; a second per-test list for the same cases judges them twice |
| `threshold.json` category floors, `node_suite_baseline.json` per-module floors | the same snapshots; a floor lets passing cases hide which case regressed |
| `parity_report_merge.py`, `parity_matrix_trend.py` | `parity.py` reading several reports |
| two normalization profiles | one |

Every snapshot entry keeps the provenance the allowlist already requires:
`issue`, `added`, `category`, `reason`. A case that is nondeterministic is
listed as such, by name; it is never absorbed into a margin on a floor.

## What a pull request runs

A table, `scripts/test_owners.py`, maps source paths to subjects, in the shape
of `SOURCE_SUITE_MAP` in `scripts/ci_e2e_scope.py`. Its self-test fails when a
tracked source file under `crates/` matches no row.

| The pull request touches… | Program cases it runs |
|---|---|
| a test case or its expected output | that case |
| source owned by one or more subjects | those subject directories |
| cross-cutting source: compiler, CLI, runtime or stdlib core | every case the pull-request tier runs today |
| no source and no test | none |

A cross-cutting change never runs less than the pull-request tier runs today;
selection only narrows what a subject-owned change runs. Rust suites keep their
own selection (`scripts/ci_test_scope.py`, `scripts/ci_e2e_scope.py`). The
nightly run executes every subject.

## Transition

`test-files/test_gap_*` keeps running on every pull request until it is empty.
Nothing leaves it before its subject directory gates pull requests.

1. **One runner** lands beside the old one and takes over each gate in turn,
   without moving a file.
2. **Ownership and selection** land: subject directories gate pull requests.
3. **New `test_gap_*` files are refused** by `lint`; the message names the
   subject directory to use.
4. **Migration**, one subject per pull request: move the files —
   `test_gap_*`, `test_parity_*` and `test-parity/node-suite/<module>/` into
   `test-parity/<subject>/` — move their snapshot entries, update the
   registries that name them, and delete exact duplicates of an existing
   subject case.
5. **Rust parity duplicates** in `crates/perry/tests` become `.ts` cases;
   the suites that stay share one compile-and-run helper.

Steps 1–3 land as a chain of stacked pull requests. Steps 4 and 5 follow,
one subject at a time.
