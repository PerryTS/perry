# Test Structure

> External suites are organized by purpose under `tests/`; Rust implementation
> tests and native Cargo integration tests remain with their owning crates.
> **A test's assertion decides its layer. Each suite keeps its oracle and
> execution interface; a pull request runs the gates it owns.**

## Where a test goes

Ask what the assertion needs to see.

| The assertion is… | It is… | It lives in |
|---|---|---|
| what a program prints or exits with, and Node can produce it | a program diffed against Node | `tests/parity/<subject>/` |
| the same, but only meaningful under a GC mode | a `.ts` program run by the GC matrix | `tests/parity/<subject>/`, modes declared in its header |
| what Perry prints for a Perry-only API | a `.ts` program with committed expected output | `tests/parity/perry/<api>/` |
| an internal invariant: rooting, layout, ordering, allocation, private state | a Rust unit test | beside the code |
| the shape of emitted IR or lowered HIR | a Rust test | beside the lowering code where it can be; `crates/<crate>/tests/` when it needs the crate's public API |
| a diagnostic, stderr, a compile failure, CLI output, determinism | a Rust test that runs the `perry` binary | `crates/perry/tests/` |
| ECMAScript spec conformance | a Test262 case | the pinned corpus; never hand-written |

A Rust test whose only assertion is stdout that Node prints identically is a
parity case in the wrong language. A parity case built to observe something the
runtime keeps internal is a unit test in the wrong suite. `crates/*/tests/`
suites run on a pull request only when the diff selects them, so coverage that
must gate every change belongs beside the code or in `tests/parity/`.

## External suite organization

The [testing index](../../../tests/README.md) is the shared entry point for
`tests/{parity,compat,features,integration,tooling,coverage,fixtures}`. These
families centralize external suites without merging their oracles or replacing
Cargo and specialized runners. Rust unit tests remain beside implementation;
native Cargo integration suites remain in their owning crate's `tests/`.

The physical layout and logical identities are separate. Parity subject IDs,
feature identities and upstream compatibility pins stay stable across moves.
The logical `test-files` suite reads retained top-level program fixtures from
`tests/fixtures/programs/` and keeps bare IDs and its snapshot subject name.
This compatibility interface preserves existing witnesses while native-only
oracles and fixture semantics are handled explicitly, not rebaselined by a move.

## Layout

```text
tests/parity/
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
Case ids omit TypeScript-family suffixes and retain `.cjs`. New helpers belong
in `fixtures/`, excluded regardless of their extension. Legacy helper entry
cases retain plain subject paths until their standalone coverage has an
explicit replacement; moving an existing entry into `fixtures/` removes it
from discovery, including the universal gap lane when its name starts with
`test_gap_`.

`tests/parity/compile-smoke.txt` retains standalone TypeScript compilation for
relocated legacy entries. The existing smoke selector combines its checked
paths with top-level `tests/fixtures/programs/*.ts`, sorting by basename to preserve shard
assignment. Missing, duplicate or escaping manifest paths fail selection. Basenames must
also be unique because CI names its output and result markers by basename.
Add a relocated `.ts` entry there when it already participates in compile
smoke; a parity skip does not imply a compile-smoke skip.

The GC representation-selection corpus keeps its stable bare-basename IDs in
`tests/parity/gc_repsel_corpus.txt`. The matrix resolves each ID to exactly one
runnable `.ts` case, whether it is still top-level in `tests/fixtures/programs/` or has moved
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

## One program-parity runner

One runner, `scripts/parity.py`, judges every program case. It
takes subjects or paths, runs cases in parallel, and judges each one the same
way: compiled, run, and compared with Node on stdout, stderr and exit status
after one normalization profile. The compile mode is a flag: `--mode fast`
against a prebuilt compiler, `--mode auto` with auto-optimize, which is what
users ship, and `--mode web`, which compiles with `--target web` and runs the
emitted page under Node through `scripts/parity_web_host.cjs`.

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
`tests/fixtures/programs/` and the subject trees in fast mode, so selection only adds cases
and never runs less than the gap lane did. Migrated cases retain their
`test_gap_` basename to stay in this universal lane.
A path no row of `scripts/test_owners.py` owns is cross-cutting. Rust suites keep their
own selection (`scripts/ci_test_scope.py`, `scripts/ci_e2e_scope.py`). The
nightly run executes every subject.

## Transition

`tests/fixtures/programs/test_gap_*` keeps running on every pull request until it is empty.
Nothing leaves it before its subject directory gates pull requests.

Legacy leftovers need classification, not a blanket move: a helper can still
be enrolled in the gap lane, and a skipped interactive case can still belong
to compile smoke. Preserve those independent gates before changing discovery.
Perry-only cases need committed expected output or an explicit Node oracle
loader; a missing oracle is not a reason to invent a passing baseline.

1. **One runner** lands beside the old one and takes over each gate in turn,
   without moving a file.
2. **Ownership and selection** land: subject directories gate pull requests.
3. **New `test_gap_*` files are refused** by `lint`; the message names the
   subject directory to use.
4. **Migration**, grouped by subject in coherent batches: move the files —
   `test_gap_*`, `test_parity_*` and `tests/parity/node-suite/<module>/` into
   `tests/parity/<subject>/` — move their snapshot entries, update the
   registries that name them, and delete exact duplicates of an existing
   subject case.
5. **Rust parity duplicates** in `crates/perry/tests` become `.ts` cases;
   the suites that stay share one compile-and-run helper.

Steps 1–3 are in place. Subject migration and Rust parity-duplicate cleanup
retain their separate acceptance criteria; centralizing physical roots does
not claim either semantic cleanup is complete.
