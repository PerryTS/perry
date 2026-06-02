# Release Readiness Instructions

**Audience:** humans wanting to understand the release-sweep workflow.
For a Claude Code session, invoke the
[`release-readiness-test`](../../.claude/skills/release-readiness-test/SKILL.md)
skill instead — it's the same content but action-oriented and gated against
auto-firing.

**Goal:** run the release sweep, read the report, file any new bugs,
summarize. Don't bump the version — only the human does that.

---

## Background (what you need to know)

- Perry is a native TypeScript compiler in Rust → LLVM. Repo:
  `github.com/PerryTS/perry`. Read `CLAUDE.md` first — it's the
  source of truth for build commands, recent changes, and per-platform
  conventions.
- The release-sweep harness lives at `scripts/release_sweep.sh` +
  `scripts/release_sweep_tiers/` + `tests/release/`. See
  `tests/release/README.md` for the tier registry and design.
- The sweep runs **13 tiers** of coverage — cargo build, cargo test,
  parity, real npm packages, GC stress, threading, doc-tests, UI host
  smoke, Apple sims (iOS/tvOS/visionOS), watchOS sim, Android emulator,
  Windows native smoke, cross-compile link-smoke. Each tier emits
  structured JSON; the orchestrator aggregates to a single `report.md`.
- Status values: **PASS** (good), **FAIL** (real bug — file it), **SKIP**
  (precondition / host gate, fine), **ERROR** (crashed before emit;
  harness or environment bug), **NOT_IMPLEMENTED** (should not appear —
  every tier is wired).

---

## Step 1 — Pre-flight

Always do these first:

```sh
# Pull latest — Ralph commits in parallel sessions; main moves fast
git pull

# Make sure perry is built fresh (the sweep needs target/release/perry)
cargo build --release -p perry-runtime -p perry-stdlib -p perry

# Optional but recommended: pre-build per-target runtimes so tier 12
# (link_smoke) and tier 10 (android_emu) can do real cross-compile
# verification instead of SKIPping. Run for whatever SDKs you have:
#   cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-apple-ios-sim
#   cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-apple-ios
#   cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-apple-tvos-sim
#   cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-linux-android
# (etc. — see CLAUDE.md for the full list)
```

Confirm before proceeding:
- `target/release/perry --version` works
- `git status` is clean (no surprise local edits — if there are, ask the
  human whether to include them)

---

## Step 2 — Run the sweep

The sweep takes **30–90 minutes** depending on host and cache state. Run
it in the background and let the user know to come back later, OR run it
synchronously and stay attentive.

**macOS / Linux:**
```sh
./scripts/release_sweep.sh
```

**Windows (any of):**
```powershell
.\scripts\release_sweep.ps1     # auto-detects Git Bash / WSL / MSYS
# or, from Git Bash:
bash ./scripts/release_sweep.sh
```

**Useful flags:**
- `--tier=N` or `--tier=N,M,P` — run only specified tiers
- `--skip=N` — run everything except listed tiers
- `--quick` — sets `PERRY_RELEASE_SWEEP_QUICK=1` (each tier may shorten
  workload; not all tiers honor it yet)
- `--gate-0.6.0` — exit non-zero unless every tier PASSes
- `--allow-skip=<ids>` — under `--gate-0.6.0`, permit specific tier SKIPs

Output lands in `target/release-sweep/<YYYYMMDD-HHMMSS>/`:
```
report.md              ← human-readable table
versions.txt           ← perry / rustc / node / SDK versions used
<NN>/result.json       ← orchestrator's verdict per tier
<NN>/summary.json      ← underlying script's per-tier numbers
<NN>/<name>.log        ← raw output for that tier
```

If the sweep stalls (>10 minutes on the same tier with no new lines in
the per-tier log), one fixture is probably hanging. Find the hung
process with `ps aux | grep tests/release` and kill it. The harness has
a 60s per-fixture timeout for tier 3 but not for tier 6/8/9 yet —
hangs in those tiers will need manual SIGTERM.

---

## Step 3 — Read the report

Open `target/release-sweep/<timestamp>/report.md`. The summary table
shows every tier with status + duration + a one-line message. The
"Result" line tallies pass/fail/skip/error/not-implemented.

For every **FAIL** or **ERROR**:
1. Read the tier's per-tier log (e.g. `03/real_packages.log`).
2. For tier 3 (real_packages), each fixture has its own logs in
   `tests/release/packages/<fixture>/perry-{compile,run}.log` plus
   `diff.log`.
3. Decide: is this a real Perry bug, or a harness / environment issue?

---

## Step 4 — Triage findings

For each FAIL, classify:

### a) Real Perry bug — file it

Use `gh issue create` against `PerryTS/perry`. Title prefix
`[release-sweep]` is helpful for tracking. Body should include:
- Summary (one paragraph)
- Reproducer (the fixture path + a minimal code excerpt)
- Expected (Node output) vs Actual (perry output, with the actual log
  excerpt)
- Environment block: perry version, npm package version, host OS

**Do not file a bug autonomously.** Draft the issue, show the title and
body to the human, get confirmation, then create it.

### b) Already-filed bug — don't re-file

Cross-reference open Perry issues before filing. The release-sweep
fixtures already correspond to known issues:
- **#601** perry-ext-fetch lib.rs i64 vs f64 (cargo test gate)
- **#602** drizzle-orm/better-sqlite3 link: undefined `_js_pg_client_new`
- **#603** hono `:id` routes + `notFound` produce no output
- **#604** axios + node:http event loop hang on `server.close()`
- **#605** redis `createClient(...).connect()` TypeError
- **#606** ws perry-ext-ws/lib.rs:583 tokio "runtime within runtime"
- **#607** `--target watchos-simulator` undefined `_perry_watchos_*`
  symbols

If a current FAIL matches one of those, note "still failing in
v<current>" rather than filing a new issue.

### c) Harness bug — fix or note

If the FAIL's log shows a shell error (`syntax error`,
`command not found`, `unbound variable`) or a missing summary file, the
harness itself is broken. Document the symptom in the human's summary.
Don't conflate with Perry bugs.

### d) Precondition gap — document, don't fail

Common ones:
- `Could not find libperry_runtime.a (for target "X")` → user needs
  `cargo build --release -p perry-runtime --target <triple>` first.
- `ANDROID_HOME not set` → expected on hosts without the Android SDK.
- `xcrun --sdk <name>` failure → expected on Linux/Windows.

These are by-design SKIPs. Note them but don't classify as failures.

---

## Step 5 — Per-platform expectations

The host gate decides which tiers run. Use this section to set
expectations.

### macOS
- Tiers 0–7 always run.
- Tier 8 (sim_apple): runs iOS / tvOS / visionOS sims if the
  corresponding SDKs are installed via Xcode. SDKs not installed →
  per-platform SKIP.
- Tier 9 (sim_watchos): runs if watchsimulator SDK installed.
- Tier 10 (android_emu): runs if `ANDROID_HOME` + emulator + AVD
  configured.
- Tier 11: SKIPs (Windows host gate).
- Tier 12 (link_smoke): host + macos always; ios/tvos/visionos/watchos
  variants if per-target runtime is pre-built (see Step 1); otherwise
  SKIP.

### Linux
- Tiers 0–7 always run. Tier 7 will exercise `perry-ui-gtk4` instead of
  `perry-ui-macos`. CLAUDE.md notes libshumate / gstreamer-sys system
  deps are required for that crate to build — if `cargo build` (tier 0)
  fails on those, install the system deps via apt/dnf or narrow the
  workspace.
- Tiers 8/9: SKIP (Apple-host only).
- Tier 10 (android_emu): runs if Android NDK installed.
- Tier 11: SKIPs.
- Tier 12: host + linux + android (if NDK).

### Windows
- Run via `release_sweep.ps1` or `bash release_sweep.sh` from Git Bash.
- Tiers 0–7 run (with `perry-ui-windows`).
- Tiers 8/9/10: SKIP (currently). Tier 10's gate is `macos,linux` —
  Windows + Android NDK is supported in principle but not yet wired.
- Tier 11: **actually runs** — invokes `scripts/smoke_windows_app.ps1`,
  which compiles a tiny perry/ui fixture for `--target windows`,
  launches under `PERRY_UI_TEST_MODE`, asserts a clean exit.
- Tier 12: host + windows + android (if NDK).
- Common gotcha: `redis-server` rare on Windows → redis-pubsub fixture
  SKIPs cleanly. That's expected.

---

## Step 6 — Summarize for the human

When the sweep is done, write a brief markdown summary directly in the
chat. Format:

```
## Release sweep result on <host> at <timestamp>

| Tier | Status | Time | Notes |
... (copy from report.md)

**Result:** N PASS / M FAIL / K SKIP / E ERROR

### New bugs to file
- [draft title] — short summary, refs <fixture>
- [draft title] — ...

### Already-known regressions still failing
- #601 — still failing
- ...

### Suggested gate command
./scripts/release_sweep.sh --gate-0.6.0 --allow-skip=<ids>
  (because tier <id> is intentionally not reachable on this host)

### Recommended next moves
1. ...
2. ...
```

Hand the summary to the human. Do not bump the version. Do not push
commits unless the human explicitly asks.

---

## Known harness limitations (avoid wasted triage)

- **Tier 12 classifier false positives**: the link-smoke tier checks for
  `artifact.<target>.app` but perry writes `artifact.app` for iOS / tvOS
  app bundles. ios-simulator / ios / tvos-simulator / tvos may show as
  FAIL when the link actually succeeded — read `12/link_smoke.log` and
  look for `Wrote * app bundle` to confirm. **Fix in progress.**
- **Tier 10 misclassification**: when per-target runtime isn't
  pre-built, every Android example FAILs with COMPILE_FAIL instead of
  SKIPping. Same root cause as tier 12's per-target SKIP routing — the
  underlying `run_android_emu_tests.sh` doesn't have the same classifier
  yet.
- **No per-fixture timeout on tier 6/8/9**: a hang in those tiers will
  freeze the sweep. Tier 3's harness uses `_fixture_run_with_timeout` (60s
  default). Manual SIGTERM may be needed for the others.

---

## When everything is green

Once the sweep produces:
- 0 FAIL
- 0 ERROR
- 0 NOT_IMPLEMENTED
- All SKIPs covered by `--allow-skip` (host gates / unreachable
  toolchains)

Run with `--gate-0.6.0`. If it prints `release_sweep: --gate-0.6.0 →
GREEN`, the human can bump:

```
[workspace.package]
version = "0.6.0"
```

That's their call, not yours. End-of-turn summary should be one or two
sentences as usual: what changed, what's next.
