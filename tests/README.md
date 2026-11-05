# Testing in Perry

External test suites are organized by purpose under `tests/`. This index is
the common entry point; each suite keeps the runner and oracle it needs.
Rust unit tests remain beside their implementation, and Cargo integration
tests remain in `crates/<crate>/tests/`.

## Find the right suite

| Purpose | Target directory | Current location |
|---|---|---|
| Program output and exit parity | `tests/parity/` | `test-parity/` during migration |
| Upstream compatibility and package radars | `tests/compat/` | `test-compat/`, plus `tests/cc-parity/` |
| Language feature matrix | `tests/features/` | `tests/features/` |
| External compiler, CLI and platform integration | `tests/integration/` | Existing integration subtrees and `test-wasi/` |
| Testing tools and gate regressions | `tests/tooling/` | New layout regressions here; existing tests migrate with their callers |
| FFI textual-coverage accounting | `tests/coverage/` | `tests/coverage/` |
| Shared test resources | `tests/fixtures/` | `tests/fixtures/`; preserve each consumer's namespace |

The target column is the agreed structure, not a claim that every suite has
moved. Legacy `test-files/` mixes program entries, compiler fixtures and
helpers; classify each group before relocating it. Moving a directory must
not remove discovery, registration, compile smoke or PR selection.

## Run the relevant checks

| Suite | Entry point |
|---|---|
| Program parity | `python3 scripts/parity.py run <subject> --mode fast` |
| Feature matrix | `python3 scripts/gen_feature_matrix.py --check --perry-bin target/release/perry` |
| Compatibility radars | The [Node-core](../test-compat/node-core/README.md), [Test262](../test-compat/test262/README.md) and [npm](../test-compat/npm-sweep/README.md) suite instructions |
| Coverage audit | `bash tests/coverage/audit.sh` |
| Coverage relocation regressions | `python3 tests/tooling/test_coverage_layout.py -v` |
| Feature relocation regressions | `python3 tests/tooling/test_feature_layout.py -v` |
| Rust | The owning crate's Cargo test command; see [AGENTS.md](../AGENTS.md#building-and-testing) |

Parity needs its pinned Node oracle, compiler and static runtime archives.
Use the [building and testing instructions](../AGENTS.md#building-and-testing)
rather than an unrelated prebuilt compiler or stale archive.

Coverage accounting searches source text; it does not prove runtime behavior.
`tests/coverage/audit.sh --markdown` rewrites `tests/coverage/COVERAGE.md`, and
`tests/coverage/regen_ts_surface_inventory.py` rewrites the five FFI inventory
programs still under `test-files/`. Neither is a read-only verification command.
Run mutating-generator regression tests in an isolated synthetic repository.

## Migration invariants

- Keep logical case IDs, expected outputs, snapshots and feature verdicts stable.
- Update discovery, generators, executable callers, documentation and CI filters
  in the same change as the move; verify both old and new layouts.
- Keep private-state tests with their implementation and upstream corpora with
  their specialized harnesses. Shared organization does not mean one universal
  executor or oracle.

See [test structure](../docs/src/testing/test-structure.md) and
[test registration](../docs/src/testing/test-registration.md) for layer and
registration rules.
