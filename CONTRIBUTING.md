# Contributing to Perry

Thanks for your interest in Perry. This document covers everything you need to land a PR — what to build, how to test, and what the review flow looks like.

For building and installing Perry as a user, see [README.md](README.md#installation). For runtime/compiler architecture, see [CLAUDE.md](CLAUDE.md). This file is about *contributing*, not using.

## Ways to contribute

All of these are welcome and none are ranked:

- **Fix a bug.** Check the [issue tracker](https://github.com/PerryTS/perry/issues), especially [`good first issue`](https://github.com/PerryTS/perry/labels/good%20first%20issue) and [`help wanted`](https://github.com/PerryTS/perry/labels/help%20wanted).
- **Close a TypeScript parity gap.** The [gap test suite](test-files/) tracks divergences from `node --experimental-strip-types`; CLAUDE.md's "TypeScript Parity Status" table is the current scoreboard.
- **Add a platform backend or widget.** See the "To add a new widget" and "Native UI" sections in [CLAUDE.md](CLAUDE.md).
- **Improve docs or examples.** Everything under [`docs/src/`](docs/src/) and [`docs/examples/`](docs/examples/) is fair game.
- **Report an issue.** File one even if you don't have a fix — reproducers are themselves a contribution.

Not sure where to start? Open a [Discussion](https://github.com/PerryTS/perry/discussions) and describe what you'd like to work on; we'll help you scope it.

## Building from source

Prerequisites match what CI installs — see [`.github/workflows/test.yml`](.github/workflows/test.yml) for the authoritative list. In short:

| Component | Version | Needed for |
|---|---|---|
| Rust | stable | Everything |
| Rust nightly + `rust-src` | latest | tvOS / watchOS cross-compile only (`-Zbuild-std`) |
| Node.js | 22 | Parity tests (`run_parity_tests.sh`) |
| C linker | any | Linking compiled binaries (`xcode-select --install` / `build-essential` / MSVC) |

Platform-specific extras — only required if you're touching that backend:

- **macOS UI** (`perry-ui-macos`): Xcode Command Line Tools
- **iOS / tvOS / watchOS**: Xcode + matching SDKs; `rustup target add aarch64-apple-ios-sim`
- **Android** (`perry-ui-android`): Android NDK (`$ANDROID_NDK_HOME` or `$ANDROID_HOME/ndk/*`); `rustup target add aarch64-linux-android`
- **Linux UI** (`perry-ui-gtk4`): `libgtk-4-dev libadwaita-1-dev libpulse-dev pkg-config` (Xvfb for headless UI tests)
- **Windows UI** (`perry-ui-windows`): MSVC (`ilammy/msvc-dev-cmd` or a `vcvars64.bat` session)

Build and test the default (platform-independent) workspace:

```bash
cargo build --release
cargo test --workspace \
  --exclude perry-ui-ios --exclude perry-ui-tvos --exclude perry-ui-watchos \
  --exclude perry-ui-gtk4 --exclude perry-ui-android --exclude perry-ui-windows
```

For a faster inner loop, `cargo check -p perry` (correctness only) or
`cargo build --profile perry-dev -p perry` (optimized local dev build). The
`.a` static archives now come from dedicated wrapper crates — build them with
`cargo build --release -p perry-runtime-static -p perry-stdlib-static` when you
need `libperry_{runtime,stdlib}.a`. See `docs/src/contributing/building.md` for
the full dev/release/dist taxonomy and the slim `--features dev-cli` CLI (#5422).

The full README [Development](README.md#development) section has more `cargo run` recipes (HIR dumps, per-crate rebuilds).

## Making changes

### What goes in a PR

- One logical change. Small PRs land faster. If you find yourself writing "also, I fixed …", split.
- Tests. New behavior needs a test; a bug fix needs a regression test. For compiler changes, drop a `.ts` file under [`test-files/`](test-files/) exercising the path. For runtime/stdlib, `#[test]` in the relevant crate.
- Docs where user-visible. New CLI flags, new perry.toml fields, new stdlib APIs → update [`docs/src/`](docs/src/).

### What does NOT go in a PR (maintainer handles these at merge)

- **Do not bump `[workspace.package] version`** in `Cargo.toml`.
- **Do not edit `**Current Version:**`** at the top of `CLAUDE.md`.
- **Do not add a "Recent Changes" entry** to `CLAUDE.md` or `CHANGELOG.md`.

Perry releases frequently — often several patch versions between a PR being opened and merged. If contributors bump the version on their branch, those bumps conflict on merge day through no fault of the contributor. The maintainer folds version + changelog metadata in when landing your PR, so it always matches the actually-shipped release.

### Commit messages

We loosely follow [Conventional Commits](https://www.conventionalcommits.org/) — not enforced, but you'll see `feat:` / `fix:` / `docs:` / `chore:` / `refactor:` prefixes in the log. Match that style and your PR reads better in the changelog. A good commit message answers "why" more than "what"; the diff already shows what.

### Claiming an issue

Comment on the issue saying you'd like to take it. We'll assign it to you. If you go quiet for a week or two, we may un-assign to let someone else pick it up — no hard feelings, just keeping the board moving.

## Running the full CI suite locally

Mirroring CI before pushing saves a round trip:

```bash
cargo build --release                               # All crates
./run_parity_tests.sh                               # Perry vs Node parity (needs Node 22)
./scripts/run_doc_tests.sh                          # Compile + run every docs/examples/*.ts
```

UI doc-tests launch real windows. On headless hosts, wrap in `xvfb-run -a` (Linux) or rely on `PERRY_UI_TEST_MODE=1` which auto-exits after one frame.

## Asking questions

- **Bug**: [open an issue](https://github.com/PerryTS/perry/issues/new/choose) — the template asks for Perry version, target, host OS, and a minimal repro.
- **Feature request**: [open an issue](https://github.com/PerryTS/perry/issues/new/choose) with the "feature request" template.
- **Open-ended question, design discussion, "is Perry right for my use case"**: [start a Discussion](https://github.com/PerryTS/perry/discussions).
- **Security issue**: email `ralph@skelpo.com` directly; please don't file a public issue for anything exploitable.

## Code of Conduct

Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md) — tl;dr: be kind, assume good faith, disagree in the open on technical merits. Reports go to `ralph@skelpo.com`.

## License

Perry is MIT-licensed. By contributing, you agree your contributions are licensed under the same terms. We do not require a CLA or DCO sign-off.
