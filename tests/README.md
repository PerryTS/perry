# Testing in Perry

External test sources, metadata and resources live under `tests/`, organized
by purpose. This is the shared placement and command index, not a universal
runner: each suite keeps the execution interface and oracle it needs.
Rust unit tests remain beside their implementation; native Cargo integration
tests remain in `crates/<crate>/tests/`. Embedded tool self-tests stay with the
implementation they exercise.

## Find the right suite

| Purpose | Directory |
|---|---|
| Program output and exit parity | `tests/parity/{node-suite,language,typescript,perry,npm}/` |
| Upstream and package compatibility | `tests/compat/{node-core,test262,npm-sweep,cc-parity}/` |
| Language feature matrix | `tests/features/` |
| Compiler, CLI, runtime and platform integration | `tests/integration/` |
| Testing tools and gate regressions | `tests/tooling/` |
| FFI textual-coverage accounting | `tests/coverage/` |
| Shared resources and retained program fixtures | `tests/fixtures/` |

Integration keeps its release, E2E, linker, UI, Wasm and WASI namespaces.
Tooling separates parity, compatibility, coverage, feature, infrastructure,
publish, soak and documentation checks. Preserve these ownership groups when
adding tests; sharing one parent directory does not flatten their fixtures.

## Run the relevant checks

| Suite | Entry point |
|---|---|
| Program parity | `python3 scripts/parity.py run <subject> --mode fast` |
| Feature matrix | `python3 scripts/gen_feature_matrix.py --check --perry-bin target/release/perry` |
| Compatibility radars | [Node-core](compat/node-core/README.md), [Test262](compat/test262/README.md), [npm](compat/npm-sweep/README.md) |
| Release integration | [Pinned release fixtures](integration/release/README.md) |
| Script regressions | `npm run test:scripts` |
| Coverage audit | `bash tests/coverage/audit.sh` |
| Layout regressions | `python3 tests/tooling/coverage/test_coverage_layout.py -v`, `python3 tests/tooling/features/test_feature_layout.py -v`, `python3 tests/tooling/compat/test_compat_layout.py -v` |
| Rust | The owning crate's Cargo command; see [AGENTS.md](../AGENTS.md#building-and-testing) |

Existing runners remain in `scripts/`; their standalone regression sources
and external fixture payloads live in the families above. Explicit unittest
start directories or qualified module names preserve collection; moving a
module does not automatically enroll it in root-level discovery.

Parity requires the pinned Node oracle, compiler and static runtime archives.
Follow the [building instructions](../AGENTS.md#building-and-testing), not an
unqualified binary or stale archive from another checkout.

## Retained legacy interfaces

`--suite test-files` remains a logical compatibility selector for top-level
entries in `tests/fixtures/programs/`. Their bare case IDs and `test-files`
snapshot subject remain stable; there is no top-level source directory with
that name. Nested resources keep their relative namespaces. New program cases
belong in their parity subject rather than this retained fixture corpus.

`tests/parity/compile-smoke.txt` retains standalone compilation for previously
relocated TypeScript entries. Together with the legacy top-level TypeScript
sources, it also supplies coverage accounting's original TS corpus; the same
legacy TSX files remain included. Fixture exclusion and parity skips must not
silently remove separate compile or PR gates.

Coverage accounting searches source text; it is not runtime behavior proof.
`tests/coverage/audit.sh --markdown` rewrites `tests/coverage/COVERAGE.md`.
`tests/coverage/regen_ts_surface_inventory.py` rewrites five FFI inventories in
`tests/fixtures/programs/`. Run mutating-generator regression checks in an
isolated repository rather than regenerating worktree evidence by accident.

Physical reorganization preserves logical test identities, snapshots, expected
outputs, pins and CI tier policy. It does not manufacture native-only oracles,
fix existing compiler failures or replace specialized upstream harnesses.
See [test structure](../docs/src/testing/test-structure.md) and
[test registration](../docs/src/testing/test-registration.md).
