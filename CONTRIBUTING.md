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
| Rust | stable (≥ 1.94) | Everything. The workspace pins no toolchain, so an older `stable` fails deep in the dependency graph with a `sqlx@0.9.0 requires rustc 1.94.0` MSRV error rather than a clear message — `rustup update stable` fixes it. |
| Rust nightly + `rust-src` | latest | tvOS / watchOS cross-compile only (`-Zbuild-std`) |
| Node.js | see [`.node-version`](.node-version) | Parity tests (`run_parity_tests.sh`). Use that exact version — an older Node makes the harness classify tests `node_fail` and silently DROP them from the gate instead of failing. |
| C linker | any | Linking compiled binaries (`xcode-select --install` / `build-essential` / MSVC) |
| **libclang** | any | `bindgen`, via the `libsqlite3-sys` build script. Without it the build dies with `Unable to find libclang`. Debian/Ubuntu: `libclang-dev`; Fedora: `clang-devel`; Arch: `clang`. If it lives somewhere non-standard, point at it with `LIBCLANG_PATH=/path/to/dir` (and, if bindgen then can't find `stdarg.h`, `BINDGEN_EXTRA_CLANG_ARGS="-isystem /path/to/clang/<ver>/include"`). |
| clang | ≥ 15 | Perry's own LLVM codegen shells out to `clang -c`. Separate from the linker above — see the [installation guide](docs/src/getting-started/installation.md). `perry doctor` verifies it. |

Platform-specific extras — only required if you're touching that backend:

- **macOS UI** (`perry-ui-macos`): Xcode Command Line Tools
- **iOS / tvOS / watchOS**: Xcode + matching SDKs; `rustup target add aarch64-apple-ios-sim`
- **Android** (`perry-ui-android`): Android NDK (`$ANDROID_NDK_HOME` or `$ANDROID_HOME/ndk/*`); `rustup target add aarch64-linux-android`
- **Linux UI** (`perry-ui-gtk4`): `libgtk-4-dev libadwaita-1-dev libpulse-dev pkg-config` (Xvfb for headless UI tests)
- **Windows UI** (`perry-ui-windows`): MSVC (`ilammy/msvc-dev-cmd` or a `vcvars64.bat` session)

Build the default (platform-independent) workspace and run the affected crates'
fast unit tests:

```bash
cargo build --release
./scripts/test_affected_crates.sh --base origin/main
```

The runner includes committed, staged, and unstaged paths relative to the base
revision; stage new files before running it because untracked paths are ignored.
Use `--dry-run` to inspect its per-crate Cargo commands, or set
`PERRY_TEST_BASE` to change the default base. It mirrors CI's
`scripts/ci_test_scope.py` selection and unit-target filtering, including the
single-threaded `perry-runtime` run. It deliberately skips integration tests
under `crates/*/tests/`; CI runs those through its scoped and nightly jobs.
Never replace it with `cargo test --workspace`: Perry's per-crate feature sets
must remain isolated.

For a faster inner loop, `cargo check -p perry` (correctness only) or
`cargo build --profile perry-dev -p perry` (optimized local dev build). The
`.a` static archives now come from dedicated wrapper crates — build them with
`cargo build --release -p perry-runtime-static -p perry-stdlib-static` when you
need `libperry_{runtime,stdlib}.a`. See `docs/src/contributing/building.md` for
the full dev/release/dist taxonomy and the slim `--features dev-cli` CLI (#5422).

The full README [Development](README.md#development) section has more `cargo run` recipes (HIR dumps, per-crate rebuilds).

For fixture-only parity reruns, `PERRY_SKIP_BUILD=1 ./run_parity_tests.sh`
reuses `PERRY_BIN` and the matching runtime/stdlib archives from
`PERRY_RUNTIME_DIR` (or the binary's directory) with auto-optimization disabled.
Rebuild instead whenever Rust or Cargo inputs, or compiler, runtime, stdlib, or
required extension sources differ; reuse does not prove that prebuilt artifacts
match the checkout.

## Making changes

### What goes in a PR

- One logical change. Small PRs land faster. If you find yourself writing "also, I fixed …", split.
- Tests. New behavior needs a test; a bug fix needs a regression test. For compiler changes, drop a `.ts` file under [`test-files/`](test-files/) exercising the path. For runtime/stdlib, `#[test]` in the relevant crate.
- **A new test file must be registered in its suite's registry, or it will not run.** Most suites glob their inputs, but four read an explicit list — a `test_gap_gc_*` witness needs a line in [`test-parity/gc_repsel_corpus.txt`](test-parity/gc_repsel_corpus.txt), a feature probe needs an entry in `test-features/feature_matrix.toml`, a compiler-output fixture needs an entry in `benchmarks/compiler_output/workloads.toml`, and a Rust test file below a suite root needs a `mod` declaration. An unregistered file is not a failing test, it is *no test at all*: your PR goes green having run nothing. `python3 scripts/check_test_registration.py` catches this in `lint` in under a second; `--list` names every registry. Full page: [`docs/src/testing/test-registration.md`](docs/src/testing/test-registration.md).
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

## Running CI's checks locally

Most red PRs come from gates that `cargo build` never looks at — a file
that crossed the 2000-line cap, fmt drift, or an unbarriered GC store site.
Run the fast, non-compiling subset with:

```bash
./scripts/pre-tag-check.sh --quick
```

That is every gate in the `lint` job that doesn't compile anything, so it
costs seconds. Drop `--quick` to add the docs linter, `cargo check`, and
Clippy; it also runs `cargo deny` when installed. The script prints every
failure in one pass rather than one per push.

Behavioral changes (HIR, codegen, runtime) also need the conformance suite,
which diffs compiled programs byte-for-byte against Node:

```bash
./scripts/run_gap_tests.sh                          # ~560 tests; the PR gate runs this in 6 shards
./scripts/run_doc_tests.sh                          # Compile + run every docs/examples/*.ts
```

What CI runs on a PR versus after a merge versus before a release is one
policy file, `scripts/ci_plan.py` (`python3 scripts/ci_plan.py --table`);
[docs/src/testing/ci-tiers.md](docs/src/testing/ci-tiers.md) explains the
three tiers. The single required status check is `pr-gate`. Apply the
`run-extended-tests` label to a PR to run the full tier and the satellite GC /
perf gates on it before merge.

**Use the Node version pinned in `.node-version`, not whatever you have.**
Node is the oracle the suite diffs against, so its version is a correctness
input: on a Node too old for a test, *node* fails, the harness classifies the
test `node_fail`, and it is dropped from the run instead of going red. CI sat
on Node 22 while the suite grew Node 24/26 features and hid 14 tests that way.

Any PR touching `crates/` also needs a `changelog.d/<PR-number>-<slug>.md`
fragment; see [changelog.d/README.md](changelog.d/README.md).

**Never cancel the CI run of the PR you are about to merge.** Cancelling other
runs to free capacity is fine and expected on a 20-slot org. Cancelling the run
of the PR being merged is not, because a cancelled job is neither a pass nor a
failure, and two separate protections go quiet at once:

* **`pr-gate` never reports**, so the single required context is absent rather
  than red. That is what invites the admin bypass, and the bypass is what lands
  the change.
* **The changelog fragment stops being required.** That check is a step inside
  the `lint` job, conditioned on `github.event_name == 'pull_request'`, so a
  cancelled `lint` skips it silently — the PR merges with no fragment, and the
  omission is invisible until the release notes are cut weeks later and the
  change simply is not in them.

The gate logic itself is correct and does not need changing: `gate` in
`.github/workflows/test.yml` runs `if: always()` and treats `failure` **and**
`cancelled` as failures, precisely so a cancelled dependency cannot read as
green. Every incident here has been a bypass of a working gate, so resist the
urge to "fix" the gate.

Both failure modes were observed on the same day. #9169 merged with `lint`
failing and five jobs cancelled; its gap shards never ran and it broke method
dispatch and property lookup on `main` for four and a half hours (#9247).
Separately, #9215, #9230 and #9235 each merged with `lint` **cancelled** — all
three touched `crates/`, none carried a fragment, and the work is consequently
absent from its release notes.

If you are reviewing status before a merge, note that "no failing checks" is
not "all checks ran": `CANCELLED` is neither, and tooling that filters for
`FAILURE` will report a clean bill of health for a PR whose gate never
executed.

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
