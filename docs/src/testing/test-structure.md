# Test Structure

> **A test's subject decides where it lives; one judge decides whether it
> passes; a pull request runs the subjects it touches.** Today the directory
> decides which gate protects a test, so every subject crowds into the one
> directory that gates pull requests. This page is the target structure and the
> order in which it lands. The evidence behind it is in
> [the 2026-11-02 census](../../audits/test-structure-census-2026-11-02.md).

## Where a test goes

Ask what the assertion needs to see.

| The assertion is… | It is… | It lives in |
|---|---|---|
| what a program prints or exits with, and Node can produce it | a `.ts` program diffed against Node | `test-parity/<subject>/` |
| the same, but only meaningful under a GC mode | a `.ts` program run by the GC matrix | `test-parity/<subject>/`, modes declared in its header |
| what Perry prints for a Perry-only API | a `.ts` program with committed expected output | `test-parity/perry/<api>/` |
| an internal invariant: rooting, layout, ordering, allocation, private state | a Rust unit test | beside the code |
| the shape of emitted IR or lowered HIR | a Rust test | `crates/<crate>/tests/` |
| a diagnostic, stderr, a compile failure, CLI output, determinism | a Rust test | `crates/perry/tests/` |
| ECMAScript spec conformance | a Test262 case | the pinned corpus; never hand-written |

A Rust test whose only assertion is stdout that Node prints identically is a
parity case in the wrong language. A parity case built to observe something the
runtime keeps internal is a unit test in the wrong suite.

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

`scripts/parity.py` runs every program case. It takes subjects or paths,
runs cases in parallel, and judges each one the same way: compiled, run, and
compared with Node on stdout, stderr and exit status after one normalization
profile. The compile mode is a flag: `--mode fast` against a prebuilt
compiler, `--mode auto` with auto-optimize, which is what users ship.

Each subject carries one snapshot per platform and mode of its cases that do
not pass. The snapshot is checked in both directions: a new failure is red,
and so is a listed case that starts passing. A crash is a defect, never a
snapshot entry.

| Replaced | By |
|---|---|
| `run_parity_tests.sh`, `run_gap_tests.sh`, `node_suite_run.py` | `parity.py run` |
| `gap_snapshot.json`, `gap_snapshot.py` | per-subject snapshots, `parity.py run --update` |
| `known_failures.json`, `threshold.json` and their scripts | the same snapshots; a floor lets passing cases hide a new failure |
| `node_suite_baseline.json` per-module floors | the same snapshots; a floor hides which case regressed |
| `parity_report_merge.py`, `parity_matrix_trend.py` | `parity.py` reading several reports |
| two normalization profiles | one |

Every snapshot entry keeps the provenance the allowlist already requires:
`issue`, `added`, `category`, `reason`. A case that is nondeterministic is
listed as such, by name; it is never absorbed into a margin on a floor.

## What a pull request runs

`scripts/test_owners.py` maps source paths to subjects, in the shape of
`SOURCE_SUITE_MAP` in `scripts/ci_e2e_scope.py`. Its self-test fails when a
tracked source file under `crates/` matches no row.

| The pull request touches… | It runs… |
|---|---|
| a test case or its expected output | that case |
| source owned by one or more subjects | those subject directories |
| cross-cutting source: compiler, CLI, runtime or stdlib core | the core set: `language/` plus each `node/` module's smoke cases |
| only docs | nothing |

The nightly run executes every subject. A selection above the cap runs the
core set instead and says so in its summary.

## Transition from `test_gap_*`

`test-files/test_gap_*.ts` keeps running on every pull request until it is
empty. Nothing leaves it before its subject directory gates pull requests.

1. **One runner** lands beside the old one and takes over each gate in turn, without moving a file.
2. **Ownership and selection** land: subject directories gate pull requests.
3. **New `test_gap_*` files are refused** by `lint`; the message names the
   subject directory to use.
4. **Migration**, one subject per pull request: move the files, move their
   snapshot entries, update the registries that name them, and delete exact
   duplicates of an existing subject case.
5. **Rust parity duplicates** in `crates/perry/tests` become `.ts` cases;
   the suites that stay share one compile-and-run helper.

Steps 1–3 land as a chain of stacked pull requests. Steps 4 and 5 follow,
one subject at a time.
