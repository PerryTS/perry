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
- [x] T1 RED (commit 796a5c35c): `test-parity/node-suite/module/imports/export-star-private-name-collision.ts`
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
- T2 unit RED: `flatten_export_all_ignores_private_same_named_binding` resolved to
  `"barrel"` instead of `"real"` before the fix (hypothesis confirmed).
  `flatten_reexport_skips_private_binding_of_star_forwarder` (sibling case: a
  re-export into a module with a private `foo` + `export *`) was written after the
  fix, so its RED was not observed.
- Fix: `resolve_exported_binding` takes `as_export`; on export-name hops (import,
  re-export, star, and the `Export::ReExport` caller) a same-named definition or
  import only resolves when the module has `Export::Named { local: name, exported: name }`.
  `is_exported` was rejected as the signal: `export { priv as renamed }` sets it on `priv`.
  The `Export::Named` caller and the alias hop stay in local-binding mode.
- T2 GREEN:
  - `cargo test --release -p perry-hir`: all suites ok, 0 failed (lib: 506 passed, 1 ignored).
  - node-suite `module` AFTER fix = **71/74** (diff=3, the same pre-existing 3).
  - zod-4-6 fixture: PASS with the fix; with the baseline binary it fails to link
    (`_perry_fn_node_modules_zod_src_v4_core_index_ts__validateAsync`).
  - #7980 `test_gap_export_star_variable_reexport` and #836
    `test_issue_836_zod_class_reexports`: output identical to Node.
  - `scripts/check_file_size.sh`: OK. `cargo fmt -p perry-hir --check`: OK.
- Env traps hit: the shell exports `FORCE_COLOR=3`, so Node colorizes output and
  the module lane dropped to 36/74 (38 false diffs). Run suites with
  `env -u FORCE_COLOR`. After every commit: rebuild perry and `rm -rf target/perry-auto-*`.
- Gap suite runs in fast mode (`PERRY_SKIP_BUILD=1`, pinned `PERRY_BIN`/`PERRY_RUNTIME_DIR`)
  so before/after use the same mode; COMPILE_FAILs are re-checked by hand
  (load avg ~40 from a parallel session causes timeouts).
