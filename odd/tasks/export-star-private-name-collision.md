# export-star-private-name-collision

## Objective
A re-export that travels through an `export *` barrel must link even when
another star source has a PRIVATE binding with the same name.

## Problem
`resolve_exported_binding` (`crates/perry-hir/src/dynamic_import/binding_origin.rs`)
treated any function/class/global/enum/`let` named `foo` as the module's
definition of the exported name `foo`, without checking that the module exports
it. In the `Export::ExportAll` loop a private `c.foo` and an exported `b.foo`
became two differing candidates, so resolution returned `None` (false
ambiguity). `flatten_exports` then fell back to naming the barrel as owner, and
the barrel never emits `perry_fn_<barrel>__foo` -> undefined symbol at link time.

Real-world trigger: zod 4.6.5 (private `validateAsync` in `core/schemas.ts`,
exported `validateAsync` in `core/parse.ts`, both behind `core/index.ts`'s
`export *`).

## Scope / constraints
- Fix at the root cause in `binding_origin.rs`; keep #7980 / #836 green.
- Regression test lives in the Node parity suite (module semantics), not the gap suite.
- zod 4.6.5 fixture under `tests/release/packages/zod-4-6/` as post-fix check.
- Version bump only after asking the user.

## TDD
Mode: on (user brief: "test-first"). Runner: `scripts/node_suite_run.py` (module lane)
plus `cargo test --release -p perry-hir` unit tests.

## Tasks
- [ ] T0 Build Perry + baseline (gap suite, node-suite module lane). Route: inline.
- [x] T1 RED: `test-parity/node-suite/module/imports/export-star-private-name-collision.ts`
      + fixtures in `imports/fixtures/export-star-private/`. Route: inline.
- [ ] T2 Fix at root cause + unit test; GREEN node-suite; zod-4-6 fixture passes;
      #7980/#836 green; perry-hir tests; gap suite no new failures; file-size check. Route: inline (1 file + tests).
- [ ] T3 changelog.d fragment; version bump (ask first).

## Evidence
- Baseline build: 0.5.1654 @ d65528b53, `cargo build --release -p perry -p perry-runtime-static -p perry-stdlib-static` OK.
  Copied to `target/baseline/` so the gap baseline is not contaminated by the fix build.
- Exact 5-file repro (a/b/c/keepalive/main) -> `Undefined symbols: _perry_fn_a_ts__foo`.
- T1 RED: node-suite `module` lane BEFORE fix = **70/74** (diff=3, compile_fail=1);
  the new entry is the compile_fail (link error). The 4 fixture modules print
  nothing and pass (the runner counts every `.ts`, same as `loader/fixtures`).
  Existing 69 entries: 66 pass (committed baseline floor is 29/69).
  Note: without `export { z }` in the entry the namespace is never materialized
  and the graph links, so the entry re-exports the namespace on purpose.
