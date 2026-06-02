# `tests/release/` — Pre-release sweep harness

This directory holds the fixtures consumed by the **release sweep**, a
local test orchestrator at `scripts/release_sweep.sh` that gates the
0.6.0 (and future) version bumps. It runs every tier-of-coverage we
care about — Rust unit tests, Perry parity, real npm packages,
cross-platform link, simulator/emulator smoke — and produces a single
aggregated report.

The release sweep is **local-only**, not part of CI. The user runs it
on their machine before tagging a release; CI runs a faster subset on
each PR.

## Quick start

```sh
# Run everything for this host
./scripts/release_sweep.sh

# Just one tier
./scripts/release_sweep.sh --tier=3

# Gate the result for a 0.6.0 bump (RED unless every tier PASSes or is
# in --allow-skip)
./scripts/release_sweep.sh --gate-0.6.0 --allow-skip=10,11

# Output goes to:
#   target/release-sweep/<timestamp>/
#     report.md              — the aggregated table
#     versions.txt           — perry / rustc / node / bun / SDK versions
#     <NN>/result.json       — orchestrator's view of each tier
#     <NN>/summary.json      — per-tier underlying script's view
#     <NN>/<name>.log        — raw output of each tier
```

On Windows: `scripts\release_sweep.ps1` is a thin wrapper that detects
Git Bash / WSL / MSYS and execs `release_sweep.sh`. Native PowerShell
port of the orchestrator is intentionally out of scope (the bash
orchestrator is the source of truth; the tier registry is one place,
not two).

## Tier registry

| ID | Name | What it verifies | Host gate |
|----|------|------------------|-----------|
| 0  | build_matrix     | `cargo build --release --workspace` on the host | all |
| 1  | cargo_workspace  | `cargo test --release --workspace` on the host | all |
| 2  | parity           | gap + edge suites byte-vs-Node                 | all |
| 3  | real_packages    | npm package fixtures (this directory)          | all |
| 4  | gc_stress        | RSS plateau under sustained alloc + aggressive GC | all |
| 5  | threading        | perry/thread spawn / parallelMap rules         | all |
| 6  | doc_tests        | every snippet under docs/examples/             | all |
| 7  | ui_host_smoke    | host UI styling matrix in sync with backends   | all |
| 8  | sim_apple        | iOS / tvOS / visionOS simulators               | macOS |
| 9  | sim_watchos      | watchOS simulator                              | macOS |
| 10 | android_emu      | Android emulator + adb                         | macOS, linux |
| 11 | windows_smoke    | native Win32 app compile + launch              | windows |
| 12 | link_smoke       | cross-compile + link a console fixture per --target | all |

Adding a new tier: drop `tierNN_<name>.sh` in
`scripts/release_sweep_tiers/`, add a row to `TIER_REGISTRY` near the
top of `scripts/release_sweep.sh`, and that's it. The tier script
emits its result via `sweep_tier_emit` (defined in
`scripts/release_sweep_lib.sh`); the orchestrator never reparses
stdout.

## Fixture contract (tier 3)

Each subdirectory of `tests/release/packages/` is a self-contained
fixture: see `tests/release/packages/README.md` for the exact shape
(package.json + entry.ts + expected.txt + fixture.sh).

## Pass / fail / skip semantics

- **PASS** — the tier ran and met its acceptance criteria.
- **FAIL** — the tier ran and at least one assertion failed. **This is
  what the sweep is for.** Tier 3 fixtures are designed to surface
  real-world bugs that the byte-for-byte gap suite misses (#585, #588,
  #589 are recent examples). A FAIL here is good news for the user — it
  exists to be filed as an issue.
- **SKIP** — the tier was intentionally not run, either because of a
  host-gate mismatch (tier 11 SKIPs on macOS) or because a precondition
  failed (no Android NDK, no MinIO binary). The reason is recorded in
  `result.json`'s `message` field. Under `--gate-0.6.0`, SKIPs only
  flag if not in `--allow-skip`.
- **NOT_IMPLEMENTED** — historical (no tiers in this state today; was
  used during stub phase).
- **ERROR** — the tier crashed before emitting result.json (rare;
  surfaced in the report so the bug can't hide).

## Per-tier specifics

### Tier 12 link_smoke
Compiles a console-only fixture for every `perry --target X` value.
PASS = artifact produced. SKIP = per-target `libperry_runtime.a` not
pre-built (precondition: `cargo build --release -p perry-runtime
--target <triple>` first) or toolchain not installed. FAIL = perry
regression.

### Tier 8/9 sim_apple / sim_watchos
Generalizes `scripts/run_simctl_tests.sh` via `PLATFORM` env var.
SKIPs per platform when its SDK isn't installed
(`xcrun --sdk <name> --show-sdk-path` failure).

### Tier 10 android_emu
Detects `ANDROID_HOME` / `ANDROID_SDK_ROOT`, the `emulator` binary,
`adb`, and at least one configured AVD. Any missing → SKIP with reason.

### Tier 11 windows_smoke
Bash entry execs `scripts/smoke_windows_app.ps1`; the .ps1 returns
0=PASS / 1=FAIL / 2=SKIP. Native Win32 GUI app launch lives in
PowerShell because the Windows console subsystem behaves
differently from bash than from `Start-Process`.
