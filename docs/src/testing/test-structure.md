# Test Structure

> **Status:** steps 1–3 of the [transition](#transition) are in place: one
> runner, subject selection on pull requests, and no new `test_gap_*` files.
> Steps 4 and 5, moving existing cases into their subjects, follow one subject
> at a time.
>
> **A test's subject decides where it lives; one runner judges whether it
> passes; a pull request runs the subjects it touches.**

## Where a test goes

Ask what the assertion needs to see.

| The assertion is… | It is… | It lives in |
|---|---|---|
| what a program prints or exits with, and Node can produce it | a program diffed against Node | `test-parity/<subject>/` |
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
  node-suite/<module>/   one Node core module, named as Node names it
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

Legacy and subject roots accept `.ts`, `.cts`, `.mts` and `test_*.cjs`. Keep
source extensions during migration: Node uses them to select the module mode.
Case ids omit TypeScript-family suffixes and retain `.cjs`. Helpers remain in
`fixtures/`, excluded regardless of their extension.

The GC representation-selection corpus keeps its stable bare-basename IDs in
`test-parity/gc_repsel_corpus.txt`. The matrix resolves each ID to exactly one
runnable `.ts` case, whether it is still top-level in `test-files/` or has moved
under a subject root. Fixtures are not cases; unknown or duplicate source IDs
fail before the oracle or compiler runs. This keeps GC report names and flat
artifacts stable while the case itself moves with its subject.

The root-dominance corpus and GC registration guards use the same source
resolver's basename-pattern lookup across all roots. Pattern order and repeated
matches remain intact; fixtures never become corpus inputs. Strict corpus
patterns fail when empty, while optional registration categories may be empty.

The WASI acceptance runner's `gap` suite and `scripts/wasi_ir_check.sh` resolve
their fixed sample basenames through the same resolver. Moving a sample does
not change its order, report name or output directory. Repeated acceptance
filters select a union before lookup; missing unselected samples do not block
that selection. An existing legacy source takes precedence; canonical-only
duplicates fail closed. A missing selected source keeps its legacy locator:
acceptance lets the oracle report its failure, while the IR gate reports its
missing-sample diagnostic before compiling.

`expected/` mirrors the test's path. A lookup by basename lets two subjects
with the same case name overwrite each other.

## One runner

One runner, `scripts/parity.py`, judges every program case. It
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

Every snapshot entry carries `issue`, `added`, `category` and `reason`;
`--update` writes a new entry as `untriaged` for review to fill in. A case that is nondeterministic is
listed as such, by name; it is never absorbed into a margin on a floor.

## What a pull request runs

A table, `scripts/test_owners.py`, maps source paths to subjects, in the shape
of `SOURCE_SUITE_MAP` in `scripts/ci_e2e_scope.py`; the `parity-scoped` job
runs what it selects. Its self-test fails on a row that matches no tracked file
or names a subject that does not exist. A path no row owns is cross-cutting.

| The pull request touches… | Program cases it runs |
|---|---|
| a test case or its expected output | that case |
| source owned by one or more subjects | those subject directories |
| cross-cutting source: compiler, CLI, runtime or stdlib core | nothing beyond the gap lane |
| no source and no test | none |

Every core pull request also runs the gap lane, every `test_gap_*` in
`test-files/` and the subject trees in fast mode, so selection only adds cases
and never runs less than the gap lane did. Migrated cases retain their
`test_gap_` basename to stay in this universal lane.
A path no row of `scripts/test_owners.py` owns is cross-cutting. Rust suites keep their
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

Steps 1–3 are in place. Steps 4 and 5 follow, one subject at a time.
