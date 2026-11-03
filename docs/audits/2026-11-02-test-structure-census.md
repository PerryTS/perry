# Test structure census, 2026-11-02

Point-in-time evidence behind `docs/src/testing/test-structure.md`.
Measured on `main` at `d76c3e1def`. Counts drift; the design page does not
depend on any single number here.

## Program-test lanes and how each is judged

| Lane | Files | Judge | Tier |
|---|---|---|---|
| `test-files/test_gap_*` | 2,454 cases (`.ts`, `.cts`, `.mts`, `test_*.cjs`) of 2,457 files | `gap_snapshot.json`, per test, both directions | pr (12 fast shards), sweep, full |
| all top-level `test-files/*.ts` | ~3,450 | `known_failures.json` + `threshold.json` category floors | full |
| `test-parity/node-suite/<module>/` | 4,498 `.ts`, 53 modules | `node_suite_baseline.json` per-module floors | full |
| test262 (pinned corpus) | ~53k | `test262/baseline.json` pass-count ratchet | full |
| `test_gap_{gc,repsel,specabi}_*` registered in `gc_repsel_corpus.txt` | ~140 | pinned Node output + GC liveness per arm | pr (1 shard), sweep, full |

One `test_gap_*` file can be judged by up to six rules: the gap snapshot, the
full-parity allowlist and its 98.5% `legacy/gap` floor, compile-smoke, the GC
representation matrix, the WASI gap sample, and cargo tests that name it.

The same node-suite case is judged two ways. `scripts/node_suite_run.py` (the
`node-suite-guard` job) compares stdout only and never reads
`test-parity/expected/`; `run_parity_tests.sh --suite node-suite` compares
stdout and stderr, honours `expected/`, and requires every case to pass.

`scripts/node_suite_regression_check.py` reports every baseline module that did
not run as `MISSING from run`, so a partial run fails by construction.

## What `test_gap_*` holds

Classified from imports, Node-global use, name tokens and header vocabulary
over all 2,457 `test_gap_*` files (three `.js` helpers included, so shares are
approximate); 160 names and 22 headers checked by hand. Medium
confidence: the line between language semantics and optimization witness is
fuzzy.

| Subject | Share |
|---|--:|
| One Node core module | ~26% (522 files import `node:*`) |
| JavaScript language semantics | ~48% by name |
| Internal-mechanism witness by name (gc, repsel, layout, objtables) | ~22% |
| TypeScript-only syntax | ~2% |
| npm packages | ~1% |
| Perry-only APIs | <1% |

Node modules are tested in three lanes at once: `test_gap_*` (per PR),
`test_parity_<module>` (full) and `node-suite/<module>` (full).

In the 30 days before this census, `main` added 1,187 `test_gap_*` files, 116
node-suite cases and 43 `test_parity_*` files.

## Rust tests

- `crates/*/src`: ~14,900 `#[test]` attributes in 1,056 files; perry-runtime
  holds 6,500, almost all internal invariants (gc, arena, shapes, layouts).
- `crates/perry/tests`: 532 suite files. ~417 compile a TypeScript program
  and assert a hard-coded stdout that Node prints identically; 46 more do the
  same under `PERRY_GC_*` modes; ~67 inspect IR or HIR; 34 assert stderr and
  10 expect a compile failure. `fn perry_bin` is redefined in 424 files.
- `perry-codegen` and `perry-hir` integration suites never run programs: they
  assert IR text and HIR shape.
- A change under `src/` of perry-runtime, perry-hir or perry-transform selects
  no `crates/perry/tests` suite on a PR (`scripts/ci_e2e_scope.py`); the
  nightly full run is the backstop.

## Source ownership

Every `*.rs`/`*.c` file under `crates/` (excluding `tests/` and `target/`),
2,197,110 lines, classified by path prefix, first match wins:

| Owner | Lines |
|---|--:|
| Exactly one Node module (or http/https/http2) | 15.4% |
| A JavaScript language area | 21.9% |
| GC | 4.2% |
| Perry-only, platform, npm-package crates | 15.1% |
| Cross-cutting: compiler, CLI, runtime and stdlib core | 43.4% |

No source-to-subject map exists. The closest precedent is
`SOURCE_SUITE_MAP` in `scripts/ci_e2e_scope.py`, a path-prefix dictionary with
a completeness self-test.

## Lanes with no driver

`tests/e2e`, `tests/wasm`, the root-level `tests/test_*.ts`, and
`test-files/multi` are referenced by no runner or workflow.
