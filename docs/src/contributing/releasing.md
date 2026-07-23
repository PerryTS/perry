# Releasing Perry

Maintainer runbook. Release cadence: patch releases (`0.5.118 → 0.5.119`) ship
weekly-ish behind the macOS CI gate. **Major releases** — any bump of the major
or minor number (e.g. `0.5.x → 0.6.0`, and the upcoming `1.0.0`) — **must be
verified on every supported platform** before the tag is pushed. Patch releases
only require the default CI gate.

## 1. Pre-release checklist (every release)

Run on macOS (the canonical dev host):

```bash
# Full rebuild — runtime/stdlib/UI libs must match the compiler version.
cargo build --release

# Core gates.
cargo test --workspace --exclude perry-ui-ios --exclude perry-ui-tvos \
  --exclude perry-ui-watchos --exclude perry-ui-gtk4 \
  --exclude perry-ui-android --exclude perry-ui-windows
./run_parity_tests.sh                       # Perry vs node stdout parity
./scripts/run_doc_tests.sh                  # Compile + run every docs/examples/*.ts
```

Then bump and tag:

```bash
# Bump [workspace.package] version in Cargo.toml AND the "Current Version"
# line in CLAUDE.md (the two must move together), then add a changelog
# entry in CHANGELOG.md.
git commit -am "release: v0.x.y"
git tag v0.x.y && git push origin v0.x.y
```

The tag push runs the test workflows, but does **not** publish packages on
its own: `release-packages.yml` triggers on a **published GitHub Release**
(or a manual `workflow_dispatch`). After pushing the tag, create and publish
the GitHub Release for `v0.x.y`; that fires the cross-platform package
matrix (see [§3](#3-what-ci-does-on-the-release)).

## 2. Major-release verification (all platforms)

Before tagging a major/minor bump, these must all pass:

| Platform | What to run | Runs in CI? |
|---|---|---|
| **macOS** (arm64 + x86_64) | `cargo test` + `run_parity_tests.sh` + `scripts/run_doc_tests.sh` | Yes, `test.yml` (arm64 only) |
| **Linux glibc** (x86_64 + aarch64) | Same, under `xvfb-run -a` for UI; `apt install libgtk-4-dev libadwaita-1-dev xvfb` first | Partial — release build only |
| **Linux musl** (x86_64 + aarch64) | Release build via `release-packages.yml`; spot-check a compiled `hello.ts` runs on Alpine | Build only |
| **Windows** (x86_64 MSVC) | `scripts/run_doc_tests.ps1`; smoke-test `perry compile hello.ts -o hello.exe && .\hello.exe` | Build only |
| **iOS Simulator** | `perry compile --target ios-simulator examples/widget_demo.ts && xcrun simctl install booted out.app` | No (Xcode required) |
| **visionOS Simulator** | `perry compile --target visionos-simulator ...`, launch in Apple Vision Pro Simulator | No (Xcode required) |
| **tvOS Simulator** | `perry compile --target tvos-simulator ...`, launch in Simulator | No (Xcode required) |
| **watchOS Simulator** | `perry compile --target watchos-simulator ...` — requires `rustup toolchain install nightly` + `cargo +nightly -Zbuild-std` | No (Xcode + nightly required) |
| **Android** | `perry compile --target android examples/widget_demo.ts`; install APK on emulator | No (NDK required) |
| **Web / WASM** | `perry compile --target web examples/wasm_ui_demo.ts`, open `out.html` in a browser | No |
| **Home-screen widgets** | `perry compile --target widgetkit ... && perry publish ios` | No |

For v1.0, expect to spend half a day spinning through the four OS VMs locally.
Only the macOS doc-tests lane currently runs in `test.yml` — the Linux (gtk4)
and Windows matrix entries are disabled pending testkit fixes (see the
commented-out entries in the `doc-tests` job), so run those manually, as with
the mobile/watch/web lanes.

### 2a. Simulator-run recipe (iOS / tvOS)

`perry-ui-ios` and `perry-ui-tvos` honor `PERRY_UI_TEST_MODE=1` — when set,
the app renders one frame, optionally writes a screenshot to
`$PERRY_UI_SCREENSHOT_PATH`, and exits cleanly. Combine with
`xcrun simctl` to verify a doc-example runs without a human:

```bash
# Compile for the simulator
perry compile --target ios-simulator docs/examples/ui/counter.ts -o counter.app

# Boot a device (one-time; reuse the UDID across runs)
xcrun simctl boot "iPhone 15"
open -a Simulator

# Install + launch with test mode
xcrun simctl install booted counter.app
PERRY_UI_TEST_MODE=1 \
  PERRY_UI_TEST_EXIT_AFTER_MS=500 \
  PERRY_UI_SCREENSHOT_PATH="$PWD/counter-ios.png" \
  xcrun simctl launch --console booted com.example.counter

# App exits 0 after rendering; screenshot lands at counter-ios.png
```

Same recipe works for `tvos-simulator` + `"Apple TV"` device. On watchOS the
Rust Tier-3 toolchain requires `+nightly -Zbuild-std` — see the
`watchos-simulator` row in the matrix above.

## 3. What CI does on the release

The `Release Packages` workflow (`.github/workflows/release-packages.yml`)
triggers on a published GitHub Release or manual `workflow_dispatch`. Matrix
runners build:

- `macos-14` / `macos-15` — arm64 + x86_64 Darwin binaries
- `ubuntu-24.04` / `ubuntu-24.04-arm` — glibc x86_64 + aarch64 (glibc 2.39 floor:
  the npm launcher and `install.sh` route older-glibc hosts to the musl build — if
  you move these runners, update `GLIBC_BUILD_FLOOR` in `npm/perry/bin/detect.cjs`)
- `ubuntu-24.04` / `ubuntu-24.04-arm` — musl x86_64 + aarch64 (fully static)
- `windows-latest` — x86_64 MSVC

Artifacts are published to:

1. **npm** (`@perryts/perry` + seven per-platform optional-deps) — via OIDC
   Trusted Publisher
2. **Homebrew** — formula auto-update
3. **APT** (Debian/Ubuntu) — GPG-signed repository
4. **winget** — manifest auto-update
5. **hub.perryts.com** — worker notification so cloud build workers refresh

A release with a failing platform build aborts the publish step for that
platform only; fix-forward with a new patch tag (e.g. `v0.6.1`) rather than
amending the existing one.

## 4. Release gates (what blocks a release)

- Parity tests must clear the threshold in `test-parity/threshold.json`
- `cargo test --workspace` (macOS excluded list as above) must be green
- `compile-smoke` must compile every file under `test-files/`
- `doc-tests` must compile + run every example under `docs/examples/`
- Benchmark regressions in `benchmark.yml` hard-fail on release tags (warn only
  on main-branch pushes)

## 5. If a release goes wrong

- **Wrong artifact published**: tag a new patch release with the fix; npm
  rejects re-publishes of the same version anyway.
- **Broken binary on one platform**: the `release-packages.yml` matrix is not
  `fail-fast: true`, so other platforms still publish. Ship a follow-up patch
  for the broken one.
- **CI hook failed after tag**: run `workflow_dispatch` with
  `publish_npm: true` to retry the npm step.
