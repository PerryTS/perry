---
name: release-readiness-test
description: Run the full release sweep on this host (macOS / Linux / Windows), aggregate the report, triage failures, draft GitHub issues for new bugs, summarize for the human. Use BEFORE /release to verify Perry is ready to ship. Takes 30–90 min; pass tier=N to run a single tier.
disable-model-invocation: true
argument-hint: [optional: tier=N to run a single tier, "quick" for fast subset, "gate" to enforce --gate-0.6.0]
allowed-tools: Bash, Read, Grep, Glob
---

# Release Readiness Test

Verify Perry is ready for a release on this host. Run the harness, read
the report, classify each FAIL, draft new bug reports, summarize for the
human.

**You do NOT bump versions, tag, or publish.** That's `/release`. This
skill is the gate that runs *before* it.

The deeper reference doc is `tests/release/RELEASE_READINESS.md` —
identical scope but written for humans. Read it once if you want
background; otherwise the steps below are self-sufficient.

## Background (skim, then act)

- Repo: `PerryTS/perry`. Always read `CLAUDE.md` first — it has the
  canonical build commands, the recent-changes log, and per-platform
  conventions you'll need to interpret tier results.
- Harness lives at `scripts/release_sweep.sh` + per-tier scripts in
  `scripts/release_sweep_tiers/` + fixtures in `tests/release/`.
  See `tests/release/README.md` for the tier registry.
- 13 tiers cover: cargo build, cargo test, parity, real npm packages,
  GC stress, threading, doc-tests, UI host smoke, Apple sims (iOS /
  tvOS / visionOS), watchOS sim, Android emulator, Windows native, and
  cross-compile link-smoke. Each tier writes `result.json` (orchestrator
  view) + `summary.json` (underlying script's view) + a raw log.
- Status values: **PASS** (good), **FAIL** (real bug — investigate),
  **SKIP** (host gate or precondition; fine), **ERROR** (crashed before
  emit; usually harness or environment), **NOT_IMPLEMENTED** (shouldn't
  appear — every tier is wired).

## Steps

### 1. Pre-flight (5 min)

```sh
git pull
cargo build --release -p perry-runtime -p perry-stdlib -p perry
test -x target/release/perry || { echo "perry binary missing"; exit 1; }
```

If `git pull` brings in changes touching `perry-runtime`, `perry-stdlib`,
`perry-codegen`, or `perry-hir`, **rebuild before sweeping** — the user's
auto-memory has a load-bearing rule (`feedback_rebuild_after_commit`)
and stale artifacts will produce misleading results.

If you have time and the user agrees, pre-build per-target runtimes for
whatever cross-target SDKs are installed. This unblocks real
verification in tier 10 (Android) and tier 12 (link-smoke):

```sh
# Examples — only run for SDKs you have:
cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-apple-ios-sim
cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-apple-ios
cargo build --release -p perry-runtime -p perry-stdlib --target aarch64-linux-android
```

Each takes ~5–10 min. Ask the user before doing this.

### 2. Run the sweep (30–90 min)

Background it so the user can do other things; tail the orchestrator
log to surface tier transitions.

**macOS / Linux:**
```sh
./scripts/release_sweep.sh > /tmp/release-sweep.log 2>&1 &
```

**Windows:**
```ps1
.\scripts\release_sweep.ps1
# (auto-detects Git Bash / WSL / MSYS and execs release_sweep.sh)
```

Argument handling — translate from `$ARGUMENTS`:
- `tier=N` or `tier=N,M` → pass as `--tier=N` / `--tier=N,M`
- `quick` → add `--quick`
- `gate` → add `--gate-0.6.0` (and figure out the right
  `--allow-skip=<ids>` after the sweep based on host)

While the sweep runs, you may set up a Monitor on `/tmp/release-sweep.log`
filtering for `release_sweep:` lines to surface each tier transition. Do
not poll. Do not start additional cargo builds (they'll race against the
sweep's tier 0).

If the sweep hangs (>10 min on the same tier with no log progress), one
fixture is likely deadlocked. Find it:
```sh
ps auxw | grep -E 'tests/release|tier[0-9]+_' | grep -v grep
```
Kill the offending `./out` or fixture process with SIGTERM. Tier 3 has a
60s per-fixture timeout (`_fixture_run_with_timeout`); tiers 6/8/9 do not
yet, so manual intervention may be needed there.

### 3. Read the report

```sh
LATEST=$(ls -dt target/release-sweep/*/ | head -1)
cat "$LATEST/report.md"
```

The summary table has every tier with status + duration + a one-line
message. Note the totals on the "Result:" line.

For every FAIL or ERROR, also read the per-tier log:
```sh
cat "$LATEST/<NN>/<name>.log"
```

For tier 3 (real_packages), each fixture has its own logs:
```sh
ls tests/release/packages/<fixture>/*.log
cat tests/release/packages/<fixture>/perry-compile.log
cat tests/release/packages/<fixture>/perry-run.log
cat tests/release/packages/<fixture>/diff.log
```

### 4. Classify each FAIL

For each FAIL or ERROR row:

#### a) Already-filed Perry bug?

Cross-reference these open issues before filing anything new (the
release-sweep fixtures map directly to several):

| # | Symptom |
|---|---------|
| #601 | `cargo test --workspace` fails — perry-ext-fetch lib.rs:1140/1179/1196 i64 vs f64 |
| #602 | drizzle-orm/better-sqlite3 link error: undefined `_js_pg_client_new` |
| #603 | hono `:id` routes + `notFound` produce no output |
| #604 | axios + node:http event loop hang on `server.close()` |
| #605 | redis `createClient(...).connect()` TypeError on undefined |
| #606 | ws `perry-ext-ws/src/lib.rs:583` tokio "runtime within a runtime" |
| #607 | `--target watchos-simulator` undefined `_perry_watchos_*` symbols |

Also check recent CLAUDE.md "Recent Changes" entries — Ralph may have
fixed something between the last sweep and this one. If a current FAIL
matches a known issue, note "still failing in v<current>" rather than
filing.

Use `gh issue list --state open --limit 50` to scan for any other
related ones.

#### b) New Perry bug

**Draft** the issue first; do NOT auto-file. Show the human the title +
body. Get explicit confirmation. Then file:

```sh
gh issue create \
    --title "[release-sweep] <component>: <one-line>" \
    --body-file /tmp/issue-<short>.md \
    --label bug,parity   # parity only if it's Node-compat
```

Body should include:
- **Summary** (one paragraph; what's broken, what should happen)
- **Reproducer** — fixture path + a minimal code excerpt
- **Expected (Node)** vs **Actual (perry)** with the actual log excerpt
- **Environment** — perry version, host OS, package version

#### c) Harness bug

Symptoms: shell errors (`syntax error`, `command not found`, `unbound
variable`), missing summary file, status=ERROR with no useful log
content. Document in your summary for the human. Don't conflate with
Perry bugs and don't file as one.

#### d) Precondition gap (not a bug)

Common ones — these are SKIPs by design:
- `Could not find libperry_runtime.a (for target X)` → user needs
  `cargo build --release -p perry-runtime --target <triple>` first.
- `ANDROID_HOME not set` → expected on hosts without the Android SDK.
- `xcrun --sdk <name>` failure → expected on Linux/Windows.

### 5. Per-platform expectations

Use this to set expectations for what should run vs SKIP.

| Tier | macOS | Linux | Windows |
|------|-------|-------|---------|
| 0 build_matrix | run | run | run |
| 1 cargo_workspace | run | run | run |
| 2 parity | run | run | run |
| 3 real_packages | run | run | run |
| 4 gc_stress | run | run | run |
| 5 threading | run | run | run |
| 6 doc_tests | run | run | run |
| 7 ui_host_smoke | run (perry-ui-macos) | run (perry-ui-gtk4) | run (perry-ui-windows) |
| 8 sim_apple | run if SDKs installed | SKIP | SKIP |
| 9 sim_watchos | run if SDK installed | SKIP | SKIP |
| 10 android_emu | run if SDK+AVD | run if SDK+AVD | SKIP (gate=macos,linux) |
| 11 windows_smoke | SKIP | SKIP | run |
| 12 link_smoke | host+macos always; cross if pre-built | host+linux always; android if NDK | host+windows; android if NDK |

Linux gotcha: tier 7 (perry-ui-gtk4) needs libshumate / gstreamer-sys
system deps. If tier 0 fails on those, install via apt/dnf or narrow
the workspace.

Windows gotcha: `redis-server` rare → redis-pubsub fixture SKIPs cleanly
(expected, not a bug).

### 6. Summarize for the human

Write a brief markdown summary directly in the chat. Suggested
structure:

```
## Release sweep result on <host> at <timestamp>

| Tier | Status | Time | Notes |
| ... copy from report.md ... |

**Result:** N PASS / M FAIL / K SKIP / E ERROR

### New bugs to file (drafted — awaiting confirmation)
- <title> — repro at <fixture path>; root cause hypothesis: ...
- ...

### Known issues still failing in v<current>
- #XXX — ...

### Harness issues encountered
- ... (if any)

### Suggested gate command
./scripts/release_sweep.sh --gate-0.6.0 --allow-skip=<ids>
  (because tier <id> is intentionally not reachable on this host)

### Recommended next moves
1. ...
2. ...
```

End-of-turn: one or two sentences. What changed, what's next. Don't
suggest the version bump — that's the human's call.

## Known harness limitations (avoid wasted triage)

- **Tier 12 false positives**: classifier looks for
  `artifact.<target>.app` but perry writes `artifact.app` for some Apple
  targets. ios-simulator / ios / tvos-simulator / tvos may report FAIL
  when the link actually succeeded. Read `12/link_smoke.log` and look
  for `Wrote * app bundle` lines to confirm before filing.
- **Tier 10 misclassification**: when per-target Android runtime isn't
  pre-built, every example FAILs with `COMPILE_FAIL` instead of one
  clean SKIP. The `run_android_emu_tests.sh` doesn't have the
  per-target SKIP routing tier 12 has.
- **No per-fixture timeout on tier 6/8/9**: a hang freezes the sweep.
  Tier 3 has `_fixture_run_with_timeout` (60s default); the others
  don't yet. Manual SIGTERM may be needed.

## What NOT to do

- Do not bump the version. `/release` does that.
- Do not push commits or tags. `/release` does that.
- Do not file bugs without showing the human first and getting
  confirmation. Drafting is fine; pushing to GitHub requires explicit
  yes.
- Do not delete or `cargo clean` the sweep output directory — the human
  may want to compare runs across hosts.
- Do not blow away `target/<triple>/release/libperry_runtime.a` to
  "force a rebuild." Pre-built per-target runtimes are valuable; if
  something looks stale, rebuild specifically (`-p perry-runtime
  --target X`), don't nuke.
- Do not auto-fire from a description-match. This skill is gated with
  `disable-model-invocation: true` because it's a 30–90 min commitment;
  only run when the human asks.
