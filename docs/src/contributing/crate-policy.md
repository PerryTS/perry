# Crate policy

Perry uses crates to isolate real build, linking, platform, and compatibility
seams. A crate is not a substitute for an internal Rust module: every workspace
member adds dependency-graph, CI, release, and maintenance cost.

## When a crate is justified

A crate should satisfy at least one of these conditions:

- it produces an independently consumed artifact;
- it implements a selectable codegen or platform target;
- it isolates a platform toolchain or a substantial optional dependency;
- it owns a stable interface used across otherwise independent modules;
- it is a tool, fixture, or test harness invoked independently;
- excluding it measurably avoids compiling or linking its implementation.

A crate is not justified only because it organizes source files, is large or
small, anticipates a future implementation, or re-exports its only dependency.
Apply the deletion test: if deleting the crate moves its implementation into
one existing owner, the seam is probably shallow; if its complexity would
spread across several callers or artifact pipelines, it is earning its place.

## Workspace categories

Every member is classified in `workspace-architecture.json`:

| Category | Meaning |
|---|---|
| `product` | A user-facing binary or primary product entry point. |
| `compiler-core` | Shared compiler representation or compilation phase. |
| `codegen-adapter` | Selectable output-target implementation. |
| `runtime-core` | Runtime, ABI, stdlib, or closely coupled runtime capability. |
| `binding` | Independently linked native library binding. |
| `platform-core` | Shared UI/platform contract. |
| `platform-adapter` | OS-specific implementation of a platform contract. |
| `artifact-wrapper` | Thin crate whose purpose is producing a named artifact. |
| `tool` | Independently invoked contributor or product tool. |
| `test-support` | Shared test implementation or harness. |
| `fixture` | Compileable documentation or integration fixture. |

Its `decision` records the reviewed direction: `keep`, `merge`, `externalize`,
`remove`, or `review`. Decisions describe the architecture roadmap; they do not
authorize deleting compatibility without the relevant migration and tests.

Choose the decision by applying the same seam test consistently:

- `keep`: the crate owns a justified artifact, adapter, stable contract, or
  independently executed tool/test surface;
- `merge`: the crate is a shallow source boundary with one natural owner and no
  independently selected artifact or toolchain;
- `externalize`: the crate is an optional integration with an independent
  version/release lifecycle and can consume Perry's stable interface;
- `remove`: the crate is obsolete, duplicated, or has no supported consumer;
- `review`: evidence is insufficient; name the missing usage or migration
  evidence before choosing a destructive direction.

Re-review a decision when a crate gains or loses an artifact target, a platform
toolchain, production consumers, or a stable interface. Size alone is never a
reason to split or merge it.

## Dependency rules

- Native bindings use `perry-ffi` as their production interface to Perry.
- A production dependency from `perry-ext-*` to `perry-runtime` is forbidden.
  `perry-ext-http` is the sole recorded migration debt while its missing FFI
  capabilities are introduced.
- Test binaries may enable `perry-ffi/runtime-link`; that edge provides runtime
  symbols for tests and is not part of the binding's distributed contract.
- Runtime and stdlib functionality must have one production implementation.
  Temporary bundled/external twins require an explicit migration path.
- Workspace dependencies are declared centrally when multiple members share
  the same internal crate or third-party version.

## Default build

The default workspace member is the `perry` CLI. Cargo builds the CLI's real
dependency closure, but it does not independently build every binding, platform
backend, fixture, or release-only static archive.

Use explicit commands for broader scopes:

```bash
# Product feedback loop
cargo check -p perry
cargo clippy -p perry --bins

# Every host-compatible workspace member (Bash)
mapfile -t excluded < <(python3 scripts/workspace_architecture.py \
  --print-excluded-scope host-compatible)
cargo_args=(--workspace)
for package in "${excluded[@]}"; do cargo_args+=(--exclude "$package"); done
cargo check "${cargo_args[@]}"
cargo clippy "${cargo_args[@]}"

# Inspect or validate the workspace architecture
python3 scripts/workspace_architecture.py --check --print-summary
python3 scripts/workspace_architecture.py --markdown
python3 scripts/workspace_architecture.py --json
```

`workspace-architecture.json` is the single source for Linux host exclusions;
the Clippy, test, and coverage scopes consume it instead of copying platform
lists. Cross-platform UI adapters remain in their target-specific CI and
release matrices; changing `default-members` does not remove that coverage.

## Inventory and baseline

The audit joins `cargo metadata` with the reviewed policy. Its JSON and Markdown
views reproduce each crate's category, decision, source path, Rust LOC,
production dependencies, internal consumers, default membership, and workspace
lint inheritance. LOC is reported live and is deliberately not committed.

The committed baseline records only structural signals: the 78 reviewed member
decisions, the default dependency closure, the `perry` CLI closure, and decision
counts. Any structural change must update the policy intentionally; ordinary
Rust source edits do not churn the baseline.

## Adding or removing a crate

A crate change must update all of the following in the same pull request:

1. explicit workspace membership;
2. `workspace-architecture.json` classification and decision;
3. shared workspace dependencies, when applicable;
4. CI/release selection for its category;
5. contributor or user documentation;
6. tests at the crate's external interface.

Run the architecture audit before requesting review. CI rejects implicit or
unclassified workspace members, unexpected default members, missing workspace
lints, and new binding-to-runtime production edges.
