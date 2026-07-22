# Building from Source

## Prerequisites

- Rust toolchain (stable): [rustup.rs](https://rustup.rs/)
- System C compiler (`cc` on macOS/Linux, MSVC on Windows)

## Build

```bash
git clone https://github.com/PerryTS/perry.git
cd perry

# Build the default product and its dependency closure
cargo build --release
```

The binary is at `target/release/perry`.

The workspace deliberately defaults to the `perry` CLI. Bindings, platform
adapters, test support, and release-only archives are selected explicitly by
their CI/release jobs. Workspace-wide host commands must use the centralized
platform exclusions described in the [crate policy](crate-policy.md).

## Build taxonomy (dev / release / dist)

Perry has three build profiles, each tuned for a different job (#5422):

| Goal | Command | Profile |
|------|---------|---------|
| Fastest correctness feedback | `cargo check -p perry` | — |
| Optimized **local** development | `cargo build --profile perry-dev -p perry` | `perry-dev` |
| Release-compatible build | `cargo build --release` | `release` |
| Official distribution artifacts | `cargo build --profile dist ...` | `dist` |

- **`perry-dev`** inherits `release` but disables the expensive distribution
  settings (`lto = false`, `codegen-units = 16`, `opt-level = 1`,
  `incremental = true`, no strip) so the edit/build loop stays short. Output is
  at `target/perry-dev/perry`.
- **`dist`** mirrors `release` exactly (ThinLTO, `codegen-units = 1`,
  `opt-level = 3`, strip) and is the explicit, named profile the release
  workflows use for shipped artifacts. Output is at `target/dist/`.

After a `--timings` build, `scripts/cargo_timing_summary.py` prints the slowest
units so build-time regressions are visible.

## Build Specific Crates

```bash
# Runtime only (must rebuild stdlib too!)
cargo build --release -p perry-runtime -p perry-stdlib

# The .a static archives are emitted by separate wrapper crates (#5422), so a
# plain `cargo build` no longer produces them as a side effect. Build them
# explicitly when you need libperry_runtime.a / libperry_stdlib.a (e.g. to link
# compiled programs without the auto-optimize rebuild):
cargo build --release -p perry-runtime-static -p perry-stdlib-static

# Codegen only
cargo build --release -p perry-codegen
```

> **Important**: When rebuilding `perry-runtime`, you must also rebuild `perry-stdlib` because `libperry_stdlib.a` embeds perry-runtime as a static dependency.

## Slim developer CLI

The default build is the full official CLI. For compiler work you can build a
slimmer CLI that omits the publish / mobile / updater / native / audit commands
and the non-native codegen backends (#5422):

```bash
cargo build -p perry --no-default-features --features dev-cli
```

`dev-cli` keeps `compile` / `run` / `check` / `types` / `cache` / `dev`. Disabled
commands drop out of `--help`, and disabled `--target` backends report a clear
"built without the `<feature>` feature" error. See `crates/perry/Cargo.toml` for
the full feature list (`full-cli`, `publish-cli`, `backend-wasm`, …).

## Run Tests

```bash
# Product unit targets
cargo test -p perry --bins

# Inspect the nightly CI test scope (all Linux-compatible test crates)
python3 scripts/ci_test_scope.py --full </dev/null

# Specific crate
cargo test -p perry-hir
cargo test -p perry-codegen
```

## Compile and Run TypeScript

```bash
# Compile a TypeScript file
cargo run --release -- hello.ts -o hello
./hello

# Debug: print HIR
cargo run --release -- hello.ts --print-hir
```

## Development Workflow

1. Make changes to the relevant crate
2. `cargo check -p perry` for fast product feedback
3. Run tests for the crates affected by the change
4. Test with a real TypeScript file: `cargo run --release -- test.ts -o test && ./test`

## Project Structure

```
perry/
├── crates/
│   ├── perry/              # CLI driver
│   ├── perry-parser/       # SWC TypeScript parser
│   ├── perry-hir/          # HIR types, data structures, and lowering
│   ├── perry-transform/    # IR passes
│   ├── perry-codegen/      # LLVM native codegen
│   ├── perry-codegen-wasm/ # WebAssembly codegen (--target web / --target wasm)
│   ├── perry-codegen-js/   # JS minifier (formerly the web target's codegen)
│   ├── perry-codegen-swiftui/ # Widget codegen
│   ├── perry-runtime/      # Runtime library
│   ├── perry-stdlib/       # npm package implementations
│   ├── perry-ui/           # Shared UI types
│   ├── perry-ui-macos/     # macOS AppKit UI
│   ├── perry-ui-ios/       # iOS UIKit UI
│   └── perry-ext-*/        # Selectively linked native bindings
├── docs/                   # This documentation (mdBook)
├── CLAUDE.md               # Detailed implementation notes
└── CHANGELOG.md            # Version history
```

## Next Steps

- [Architecture](architecture.md) — Crate map and pipeline overview
- See `CLAUDE.md` for detailed implementation notes and pitfalls
