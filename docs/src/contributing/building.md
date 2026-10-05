# Building from Source

## Prerequisites

- Rust toolchain: install [rustup](https://rustup.rs/); the workspace pins
  `nightly-2026-10-04` in `rust-toolchain.toml` because its exception transport
  needs nightly unwind-table flags. Running Cargo from the checkout installs
  or selects the pinned toolchain automatically.
- System C compiler (`cc` on macOS/Linux, MSVC on Windows)
- **libclang** — the `libsqlite3-sys` build script runs `bindgen`. Missing it
  aborts the build with `Unable to find libclang`. Install `libclang-dev`
  (Debian/Ubuntu), `clang-devel` (Fedora) or `clang` (Arch). For a
  non-standard location set `LIBCLANG_PATH` to the directory holding
  `libclang.so`; if bindgen then reports `'stdarg.h' file not found`, also set
  `BINDGEN_EXTRA_CLANG_ARGS="-isystem <clang-resource-dir>/include"`.
- **LLVM 22 development files** — required by Perry's default in-process
  codegen backend. `llvm-config --version` must report LLVM 22. If
  `llvm-config` is not on `PATH`, set `LLVM_SYS_221_PREFIX` to the LLVM 22
  prefix. On Windows this variable is mandatory and must point at the
  extracted LLVM 22 development archive used by the build.
- For the cached Make targets, install the pinned Mr Boxington once with
  `make mbx-deps`.

An external clang is not part of the normal in-process codegen path. Install a
matching clang only when working on a path that explicitly invokes it (for
example Windows host `--embed`) or when building with
`--no-default-features` to bisect the textual-IR backend.

## Build

```bash
git clone https://github.com/PerryTS/perry.git
cd perry

# Build the fully optimized product and its dependency closure
make build-prod
```

The binary is at `target/prod/perry`.

The workspace deliberately defaults to the `perry` CLI. Bindings, platform
adapters, test support, and release-only archives are selected explicitly by
their CI/release jobs. Workspace-wide host commands must use the centralized
platform exclusions described in the [crate policy](crate-policy.md).

## Build taxonomy (dev / prod, with compatibility profiles)

Use `dev` and `prod` for new builds. Cargo's `debug` output directory is the
artifact directory for the `dev` profile; it is not a separate Cargo profile.
The legacy `perry-dev`, `release`, and `dist` profile names remain for existing
CI, tooling, and reproducible investigations.

| Goal | Command | Profile |
|------|---------|---------|
| Fastest correctness feedback | `cargo check -p perry` | — |
| Fast, debuggable local development | `make build-dev` (or `cargo build --profile dev -p perry`) | `dev` |
| Optimized local development / CI | `cargo build --profile perry-dev -p perry` | `perry-dev` |
| Fully optimized local production build | `make build-prod` | `prod` |
| Release-compatible build | `cargo build --release` | `release` |
| Official distribution artifacts | `cargo build --profile prod ...` | `prod` |

- **`dev`** keeps debug info and prioritizes compilation speed (`opt-level = 0`,
  `codegen-units = 256`, debug assertions on, and no LTO). Direct Cargo builds
  use Cargo incremental compilation; `make build-dev` lets mbx manage learned
  incremental state while preserving shared-cache reuse for matching outputs.
  It keeps `panic = "abort"` so locally built Perry runtime archives preserve
  the production exception transport contract. `make build-dev`
  routes through Mr Boxington. Output is at `target/debug/perry`.
- **`perry-dev`** inherits `release` but disables the expensive distribution
  settings (`lto = false`, `codegen-units = 16`, `opt-level = 1`,
  `incremental = true`, no strip) so the edit/build loop stays short. Output is
  at `target/perry-dev/perry`.
- **`prod`** inherits the optimized distribution settings (ThinLTO,
  `opt-level = 3`, strip) and uses 16 codegen units to parallelize compilation.
  ThinLTO still optimizes across those units. Release workflows and
  `make build-prod` use Mr Boxington and write to `target/prod/`.
- **`dist`** is retained as a compatibility profile with the same optimized
  settings. New build commands should use `prod`.

After a `--timings` build, `scripts/cargo_timing_summary.py` prints the slowest
units so build-time regressions are visible.

### Compiler cache for ephemeral worktrees

Short-lived worktrees and coding agents can opt into a compiler cache shared
outside the repository. Install `sccache` in your user environment, then run
Cargo through the wrapper:

```bash
./scripts/cargo_cached.sh check -p perry

# Slim, optimized developer CLI
./scripts/cargo_cached.sh build --profile perry-dev -p perry \
  --no-default-features --features dev-cli
```

The wrapper disables Cargo incremental compilation because `sccache` cannot
cache incremental artifacts. It stores compiler objects under
`${XDG_CACHE_HOME:-$HOME/.cache}/perry/sccache` by default, not in the worktree;
set `SCCACHE_DIR` or `SCCACHE_CACHE_SIZE` to override its `12G` cache policy.
It does not install or configure `sccache` globally.

In a long-lived worktree, use ordinary `cargo` commands instead so the local
incremental cache remains available. Cache benefit depends on the compiler,
flags, dependencies, and how much prior work the cache can reuse.

## Build Specific Crates

```bash
# Runtime only (must rebuild stdlib too!)
cargo build --profile prod -p perry-runtime -p perry-stdlib

# The .a static archives are emitted by separate wrapper crates (#5422), so a
# plain `cargo build` no longer produces them as a side effect. Build them
# explicitly when you need libperry_runtime.a / libperry_stdlib.a (e.g. to link
# compiled programs without the auto-optimize rebuild):
cargo build --profile prod -p perry-runtime-static -p perry-stdlib-static

# Codegen only
cargo build --profile prod -p perry-codegen
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

For dedicated Linux agent hosts, use [compressed ZFS build storage](build-storage.md)
and `make build-agent` to check storage before building.
