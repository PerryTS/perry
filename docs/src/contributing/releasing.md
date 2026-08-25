# Releasing Perry

Maintainer runbook. Every release, including a patch, is gated on the exact
release-candidate commit by the full Tests workflow, Simulator Tests, and the
complete package-build matrix. A PR-tier or ordinary push-to-main run is not a
release gate.

## 1. Pre-release checklist (every release)

Start from a clean checkout that contains current `origin/main`. Do not release
from a detached HEAD or from a moving `main` branch. The npm-first pipeline
pins the candidate branch and commit SHA, retains the exact npm tarballs, and
refuses to tag if the branch moves or any public registry shasum differs.

```bash
# Confirm the checkout is current and clean.
git fetch origin
git rev-list --count HEAD..origin/main       # must print 0
git status --short                          # must print nothing

# Fast policy and script checks.
python3 scripts/ci_plan.py --self-test
BASE_SHA=origin/main scripts/run_lint_gates.sh
npm ci --ignore-scripts --no-audit --no-fund
npm run test:scripts
./scripts/regen_api_docs.sh
git diff --exit-code -- docs/src/api/reference.md docs/api/perry.d.ts

# Host-runnable behavioral checks. These improve turnaround, but do not
# replace the required cross-platform CI jobs.
cargo test --workspace --exclude perry-ui-ios --exclude perry-ui-tvos \
  --exclude perry-ui-watchos --exclude perry-ui-gtk4 \
  --exclude perry-ui-android --exclude perry-ui-windows
./run_parity_tests.sh
./scripts/run_doc_tests.sh
```

Prepare the release candidate. Perry's normal merges already advance the
workspace version, so use the version at the chosen commit; do not add another
bump merely to release it. The version must exist in the source before builds
start because it is embedded in Cargo and npm artifacts. `Cargo.toml` and
`CLAUDE.md` must agree, release-note fragments must exist, and the Git tag must
not exist locally or on origin. If that version was already tagged, land a new
version bump through the normal merge process first.

```bash
# Substitute the version already present in Cargo.toml.
VERSION=0.x.y
grep -m1 '^version' Cargo.toml
grep -F "**Current Version:** $VERSION" CLAUDE.md
find changelog.d -maxdepth 1 -type f -name '[0-9]*.md' | grep .
git ls-remote --tags origin "refs/tags/v$VERSION"  # must print nothing
git switch -c "release/v$VERSION"
git push -u origin "release/v$VERSION"
```

Run the two exact-SHA CI gates on that pinned branch and wait for both to pass:

```bash
gh workflow run test.yml --ref "release/v$VERSION" -f tier=full
gh workflow run simctl-tests.yml --ref "release/v$VERSION"
```

Freeze that candidate SHA. Unrelated merges to `main` after the branch is cut do
not invalidate successful gates and do not require another test cycle. Refresh
the candidate only for a release-blocking fix or a required version change.

Then use the npm-first pipeline. npm publication happens only in GitHub Actions
through Trusted Publisher/OIDC. There is no `npm login`, npm account session,
2FA approval, or long-lived npm token in CI or on the maintainer machine. The
local command uses GitHub authentication to dispatch/watch the workflow and
anonymous registry reads to verify the published bytes.

Socket is an optional pre-publish tarball scan, not an npm credential. When
enabled, Actions submits each of the exact nine `.tgz` files as a temporary
Socket full scan and evaluates the results against the `perryts` Socket
organization's security policy. Policy actions configured as `error` block
publication; `warn` findings are recorded but do not block. The same tarballs
that pass are then sent to npm. For the current release, omit `--socket-scan`;
the workflow records an explicit skipped receipt and continues.

To enable the gate later, store `SOCKET_API_TOKEN` as a repository or
organization Actions secret with permission to create/read full scans, read the
organization policy, and read quota. No GitHub Environment is required. The
value stays only in GitHub and must never be committed or exported locally:

```bash
gh secret set SOCKET_API_TOKEN --repo PerryTS/perry
```

Before the first nine-package release, an npm organization owner must confirm
that every name in `scripts/publish/constants.mts` exists and has this single
Trusted Publisher configuration:

- provider: GitHub Actions
- organization/repository: `PerryTS/perry`
- workflow filename: `release-packages.yml`
- environment: none
- allowed action: **`npm publish`**

npm permits only one trusted publisher per package. The canonical identity is
the existing `release-packages.yml` workflow above, with direct **`npm
publish`** enabled. Do not configure `npm-stage-publish.yml` as a second
publisher and do not add an environment name that the other packages do not
use.

In particular, verify the ARM64 Windows package:

```bash
npm view @perryts/perry-win32-arm64 name
```

If that returns `E404`, stop: npm does not allow a Trusted Publisher to be
configured for a package that does not exist, so OIDC cannot perform its first
publish. An `@perryts` owner must provision that package name once before this
nine-package OIDC-only flow can work, then configure the Trusted Publisher
fields above. The release pipeline intentionally refuses a partial set.

```bash
npm run publish:release     # one Release Packages run: exact-SHA gates/builds,
                            # publish + verify all 9 via OIDC, then create
                            # v0.x.y + the GitHub Release last
# Later, once the repository/org Socket secret exists:
npm run publish:release -- --socket-scan
npm run publish:status      # inspect the commit/run/package/Socket receipt
```

Publication sends the eight platform packages first and the wrapper last. If a
network or registry error interrupts it before the tag exists, rerun the failed
jobs (or `publish:release`) on the same candidate. Actions skips an
already-public package only when its immutable registry shasum matches the exact
CI tarball; otherwise it stops and requires a new version.

If the accumulated changelog fragments exceed the inline release-note budget,
the publisher keeps the GitHub Release body concise and uploads the complete
notes as the checksummed `release-notes-full.md` asset. No fragment is dropped.

Do not run `npm login`, `git tag`, or manually publish a GitHub Release. The
local command dispatches `release-packages.yml` with `cut_release=true`; that
workflow creates the tag only after all nine npm versions are public and their
registry shasums match the exact CI tarballs.

## 2. Additional major-release verification

The automated gates above apply to every release. For a major/minor release,
also perform the product-level platform checks that are not fully represented
by the automated suites:

| Platform | What to run | Runs in CI? |
|---|---|---|
| **macOS** (arm64 + x86_64) | Smoke-test installed archives on both architectures | Builds in release matrix; full tests on macOS arm64 |
| **Linux glibc** (x86_64 + aarch64) | Smoke-test the packaged binary on the oldest supported glibc | Builds in release matrix |
| **Linux musl** (x86_64 + aarch64) | Spot-check a compiled `hello.ts` on Alpine | Builds in release matrix |
| **Windows** (x86_64 + ARM64 MSVC) | Smoke-test installed archives on both architectures | Build plus full-tier Windows checks |
| **iOS Simulator** | Exercise a representative app with `xcrun simctl` | Required `simctl-tests.yml` |
| **visionOS Simulator** | `perry compile --target visionos-simulator ...`, launch in Apple Vision Pro Simulator | No (Xcode required) |
| **tvOS Simulator** | `perry compile --target tvos-simulator ...`, launch in Simulator | No (Xcode required) |
| **watchOS Simulator** | `perry compile --target watchos-simulator ...` — requires `rustup toolchain install nightly` + `cargo +nightly -Zbuild-std` | No (Xcode + nightly required) |
| **Android** | `perry compile --target android examples/widget_demo.ts`; install APK on emulator | No (NDK required) |
| **Web / WASM** | `perry compile --target web examples/wasm_ui_demo.ts`, open `out.html` in a browser | No |
| **Home-screen widgets** | `perry compile --target widgetkit ... && perry publish ios` | No |

Record the manual results in the release issue. These checks supplement CI;
they never waive a red required workflow.

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
- `ubuntu-24.04` / `ubuntu-24.04-arm` — glibc x86_64 + aarch64; the compiler,
  runtime, stdlib and extension archives build inside architecture-matched
  Debian 11 (glibc 2.31) containers, while GTK4 builds on the noble host and is added
  afterward (glibc 2.31 compiler floor; keep `GLIBC_BUILD_FLOOR` in
  `npm/perry/bin/detect.cjs` synchronized)
- `ubuntu-24.04` / `ubuntu-24.04-arm` — musl x86_64 + aarch64 (fully static)
- `windows-latest` / `windows-11-arm` — x86_64 + ARM64 MSVC

Artifacts are published to:

1. **npm** (`@perryts/perry` + eight per-platform optional-deps) — via OIDC
   Trusted Publisher
2. **Homebrew** — formula auto-update
3. **APT** (Debian/Ubuntu) — GPG-signed repository
4. **winget** — manifest auto-update
5. **hub.perryts.com** — worker notification so cloud build workers refresh

In the canonical npm-first flow, any failing host or cross build prevents npm
publication. The wrapper is not published when a platform package fails. Once a
version has become public, fix-forward with a new patch version rather than
amending an existing tag.

## 4. Release gates (what blocks a release)

`release-packages.yml` rejects a cut release unless `test.yml` has a successful
**`full-suite-gate`** and `simctl-tests.yml` has a successful run on the exact
candidate SHA. A green PR-tier or push-to-main sweep does *not* count. See [CI
tiers](../testing/ci-tiers.md). The full tier is:

- everything the PR gate and the post-merge sweep run (`lint`, `check`, `warnings`,
  `cargo test --workspace`, the gap suite, `gc-stress`, Windows x64 + ARM64 builds,
  compiler-output gates, `repsel-census`, `harmonyos-smoke`, `security-audit`),
  plus `binary-size` and
- `parity` — must clear the threshold in `test-parity/threshold.json` and add no
  new / stale known-failure entries
- `compile-smoke` — must compile every file under `test-files/`, plus the UI
  styling matrix, Fastify integration and memory-stability tests
- the gap suite in its 8-shard **auto-optimize** mode (the PR/sweep tiers use the
  prebuilt-runtime `fast` mode)
- `doc-tests` (macOS + Windows) — must compile + run every example under
  `docs/examples/`
- the package smokes (`drizzle-mysql-smoke`, `ink-link-smoke`, `effect-basic-smoke`)
  and `native-abi-evidence-packet`
- `full-suite-gate`, the fan-in which proves every required full-tier job above
  succeeded

None of these carries `continue-on-error` any more: a red suite in the full tier
blocks the release. If a suite is red for a reason that is not the release
candidate's fault, fix it on `main` first (or open an issue and consciously
re-add a job-level `continue-on-error: true` with that issue number) — do not
publish past it.

The release workflow then requires every host/cross package build, all nine
exact npm publishes, and public-registry sha1 verification. Socket is an
optional pre-publish gate and is currently skipped. `benchmark.yml`, docs,
container tests, Homebrew, APT, winget, and worker refresh are tag riders or
distribution steps: monitor them after the GitHub Release is created, but do
not mistake them for pre-tag gates.

## 4a. What tells you a release is overdue

Nothing in the sections above fires if a release simply never happens. That is
[#7491](https://github.com/PerryTS/perry/issues/7491): npm served a month-old
`latest` while the linker fix users were hitting had been on `main` for weeks. Every
gate was green, and all of them were right — they measure `main`, and `main` is not
what `npm install @perryts/perry` gives you. The only detector was a user reading the
versions tab.

`npm-publish-freshness.yml` runs daily and calls
`scripts/check_npm_publish_freshness.py`, which reads the full packument for every
package under `npm/` and compares it against `[workspace.package] version`:

| signal | budget | why |
|---|---|---|
| age of the published `latest` | 14 days | counted **only while the tree is ahead**, so a quiet week with nothing to release is not a failure. This is the signal that would have caught #7491 on day 15. |
| patch distance | 500 | every merge bumps the workspace patch, so this is a commit count in disguise. A backstop for a cadence spike inside an unexpired age budget — not a release-cadence rule. |
| platform packages match the launcher | none | `npm/perry/package.json.tmpl` pins its optionalDependencies to its own exact version, so a partial publish breaks installs while both halves sit inside their budgets. |

Budgets live in `scripts/npm_publish_freshness.json`. A failing run files one sticky
issue and updates it in place; it closes itself once the registry has caught up.

```bash
python3 scripts/check_npm_publish_freshness.py --self-test       # proves it can fail
python3 scripts/check_npm_publish_freshness.py --check-manifest  # offline
python3 scripts/check_npm_publish_freshness.py --dry-run         # real registry, no issue writes
```

An unreachable or unparseable registry is **red, not a skip** — this detector exists
because a silence read as health for a month, and a skip that exits 0 is that same
silence with a green badge. It is deliberately not a required status context: whether
a release has been cut is not something a PR author can fix.

## 5. If a release goes wrong

- **Wrong artifact published**: tag a new patch release with the fix; npm
  rejects re-publishes of the same version anyway.
- **Broken build before npm publication**: fix it and rebuild the complete
  nine-package set; the canonical flow will not tag a partial set.
- **Broken binary discovered after publication**: ship a follow-up patch version;
  neither npm versions nor release tags are mutable.
- **A post-tag distribution hook failed**: re-run the failed workflow. To retry
  the legacy release-packages distribution legs, dispatch it with
  `existing_tag=vX.Y.Z`; add `publish_npm=true` only when the idempotent npm leg
  itself also needs retrying.
