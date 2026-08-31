#!/usr/bin/env bash
# Committed runner for the Perry "gap" suite.
#
# Every test-files/test_gap_*.ts is AOT-compiled by Perry and diffed
# byte-for-byte against `node --experimental-strip-types`. This is a thin
# wrapper over run_parity_tests.sh --filter test_gap_ so it reuses the ONE
# canonical normalizer, the skip-list, the per-test output cap, and the JSON
# report (this shared-normalizer reuse is the seed of roadmap initiative I-14).
#
# Replaces the out-of-repo /tmp/run_gap_tests.sh that CLAUDE.md used to point
# at — the gap suite is the highest-signal-per-second test Perry has and was
# previously dark in CI.
#
# Regression-gate semantics: exits non-zero on ANY divergence from the
# committed snapshot in test-parity/gap_snapshot.json — a test that started
# failing, a test that started passing, or one the oracle stopped covering.
# (run_parity_tests.sh's own exit code only trips below 80% AGGREGATE parity,
# which is far too loose to catch a single-feature regression — exactly the
# "a module silently went to 0 behind a green build" class.)
#
# Accept the current state with `UPDATE_SNAPSHOT=1 ./scripts/run_gap_tests.sh`
# and commit the snapshot diff. The old known_failures.json gate only looked
# for NEW failures, so a fixed test kept its entry forever (#797); checking
# both directions makes an orphaned entry impossible.
#
# Requirements:
#   - a Rust toolchain (the wrapped run_parity_tests.sh builds target/release/perry)
#   - node with --experimental-strip-types, at the .node-version pin
#   - root npm dependencies installed with npm ci (package-backed Node oracles)
#   - jq, Python 3 (`python3` or `python`)
#
# Usage: scripts/run_gap_tests.sh [--shard N/M]
#        UPDATE_SNAPSHOT=1 scripts/run_gap_tests.sh   # accept current state
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# #9273: the gap snapshot is meaningful only for one exact oracle environment.
# A local run used Node 26.8.1 instead of the 26.5.1 pin and had not run
# `npm ci`. Five package-backed fixtures therefore compared Node's
# ERR_MODULE_NOT_FOUND with valid Perry output and were labelled regressions.
# Refuse that experiment before spending minutes building/running the suite.
if ! command -v node >/dev/null 2>&1; then
  echo "ERROR: node is required by the gap suite." >&2
  exit 2
fi
pinned_node="$(tr -d '[:space:]' < .node-version)"
running_node="$(node --version 2>/dev/null | sed 's/^v//')"
if [[ -z "$pinned_node" || -z "$running_node" ]]; then
  echo "ERROR: could not determine the Node oracle version." >&2
  exit 2
fi
if [[ "$running_node" != "$pinned_node" ]]; then
  echo "ERROR: Node oracle mismatch: running v$running_node, but .node-version pins v$pinned_node." >&2
  echo "       Gap output is compared byte-for-byte, so even a patch release is a" >&2
  echo "       different correctness input. Install/select the pinned version." >&2
  exit 2
fi

# package.json is the manifest for package-backed gap oracles. Checking the
# whole top-level install avoids a second hand-maintained package list that
# would go stale when a new fixture starts importing a dependency. npm ls
# catches both a missing node_modules tree and versions that do not satisfy the
# committed manifest; CI establishes the same state with npm ci.
if ! command -v npm >/dev/null 2>&1; then
  echo "ERROR: npm is required to verify the gap suite's Node oracle dependencies." >&2
  echo "       Install npm, then run: npm ci --ignore-scripts --no-audit --no-fund" >&2
  exit 2
fi
if ! npm ls --depth=0 --silent >/dev/null 2>&1; then
  echo "ERROR: root npm dependencies are missing or do not match package.json." >&2
  echo "       The Node oracle must be able to import package-backed gap fixtures;" >&2
  echo "       otherwise ERR_MODULE_NOT_FOUND is misreported as a Perry regression." >&2
  echo "       Run: npm ci --ignore-scripts --no-audit --no-fund" >&2
  exit 2
fi

if [[ -n "${PERRY_HOST_PLATFORM:-}" ]]; then
  host_platform="$PERRY_HOST_PLATFORM"
else
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*) host_platform=windows ;;
    Darwin) host_platform=macos ;;
    Linux) host_platform=linux ;;
    *) host_platform=other ;;
  esac
fi
case "$host_platform" in
  windows|macos|linux|other) ;;
  *)
    echo "Invalid PERRY_HOST_PLATFORM '$host_platform'" >&2
    exit 2
    ;;
esac
if command -v python3 &>/dev/null; then
  PYTHON_CMD=python3
elif command -v python &>/dev/null; then
  PYTHON_CMD=python
else
  echo "Python 3 is required by the gap snapshot checker" >&2
  exit 2
fi
TEMP_ROOT="${TMPDIR:-${TEMP:-${TMP:-/tmp}}}"
if [[ "$host_platform" == "windows" ]] && command -v cygpath &>/dev/null; then
  TEMP_ROOT="$(cygpath -u "$TEMP_ROOT")"
fi
mkdir -p "$TEMP_ROOT"

# The committed legacy snapshot is the Linux baseline used by required CI.
# Other hosts get their own file so accepting a Windows-only divergence never
# changes Linux's ratchet (and vice versa). The first run on a new platform is
# expected to request UPDATE_SNAPSHOT=1 to establish that platform's baseline.
if [[ "$host_platform" == "linux" ]]; then
  default_snapshot="test-parity/gap_snapshot.json"
else
  default_snapshot="test-parity/gap_snapshot.${host_platform}.json"
fi
if [[ -n "${GAP_SNAPSHOT:-}" ]]; then GAP_SNAPSHOT_EXPLICIT=1; fi
GAP_SNAPSHOT="${GAP_SNAPSHOT:-$default_snapshot}"

# #7566: a platform baseline that does not exist must SAY SO, not die in a
# FileNotFoundError whose exit 1 is indistinguishable from a regression list.
# On macOS there is no `gap_snapshot.macos.json`, so every local run of this
# script ended in a traceback after doing all ~490 tests' worth of work — the
# gate could neither pass nor fail, and the useful per-test results were only
# recoverable by reading the log by hand.
#
# Falling back to the shared Linux baseline is the honest default: it is the
# one the required CI gate uses, so a local run is measured against the same
# bar. It is announced loudly rather than silently, because the two platforms
# genuinely differ (macOS carries known host-local failures), and a run that
# silently compared against another platform's expectations would be its own
# kind of lie.
if [[ ! -f "$GAP_SNAPSHOT" ]]; then
  if [[ -n "${GAP_SNAPSHOT_EXPLICIT:-}" ]]; then
    echo "ERROR: GAP_SNAPSHOT='$GAP_SNAPSHOT' does not exist." >&2
    exit 2
  fi
  echo "NOTE: no $host_platform baseline at '$GAP_SNAPSHOT'; comparing against" >&2
  echo "      test-parity/gap_snapshot.json (the shared baseline required CI uses)." >&2
  echo "      Establish a $host_platform baseline with UPDATE_SNAPSHOT=1 if its" >&2
  echo "      expected-failure set genuinely differs." >&2
  GAP_SNAPSHOT="test-parity/gap_snapshot.json"
fi

# Run-scoped temp dir — fixed /tmp names would let concurrent runs (a second
# PR, local + CI on the same box, or the future node-suite-guard alongside)
# clobber each other's failure lists and produce a false gate result.
WORK="$(mktemp -d "$TEMP_ROOT/perry-gap.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

echo "==> Running gap suite (test-files/test_gap_*.ts) via run_parity_tests.sh --filter test_gap_"
# run_parity_tests.sh exits 1 when AGGREGATE parity < 80%. We gate on the
# snapshot diff instead (below), so don't let its aggregate exit abort us.
# Forward extra args (notably --shard N/M) so CI can fan the gap suite out
# across parallel runners; with no args this is the full serial gap suite.
REPORT="test-parity/reports/latest.json"
rm -f "$REPORT"
if ./run_parity_tests.sh --filter test_gap_ "$@"; then
  parity_status=0
else
  parity_status=$?
fi

if [[ "$parity_status" -ne 0 && "$parity_status" -ne 1 ]]; then
  echo "ERROR: run_parity_tests.sh failed with exit status $parity_status" >&2
  exit "$parity_status"
fi

if [[ ! -f "$REPORT" ]]; then
  echo "ERROR: parity report not found at $REPORT (did run_parity_tests.sh run?)" >&2
  exit 2
fi

# Crashes (SIGSEGV/SIGABRT/timeout) are hard defects, never cosmetic gaps.
# Surface them ALWAYS — including when they are already accepted in the
# snapshot — so a segfault can never be filed away as a routine "output
# mismatch" (that is precisely how the #6271 zlib SIGSEGV stayed invisible
# while it red-flagged the required conformance gate on ~13 open PRs).
jq -r '(.failures.crash // []) | .[] | select(. != "")' "$REPORT" | sort -u > "$WORK/crashes.txt"
if [[ -s "$WORK/crashes.txt" ]]; then
  echo "" >&2
  echo "*** CRASHES — Perry died from a signal or timed out (NOT an output nit): ***" >&2
  sed 's/^/  - /' "$WORK/crashes.txt" >&2
  echo "" >&2
  echo "A crash is a hard defect. Even when the snapshot already accepts it," >&2
  echo "it is reported here every run. Reproduce on LINUX — several crash classes" >&2
  echo "(e.g. handle-band derefs, #6271) are masked on macOS by its 2 TB heap floor." >&2
  echo "" >&2
fi

# #7526: a crash may never be PARKED in the snapshot. The block above reports
# crashes every run, but reporting is not gating -- a SIGSEGV sat in
# `gap_snapshot.json` as `status: "crash"` for a month, against two CLOSED
# issues (#5433, #5917), laundered through the channel meant for cosmetic
# output gaps. The policy is stated four comments up; this enforces it.
#
# The snapshot is for known *output* divergences. A crash is a hard defect and
# must be fixed or tracked as one, never accepted here. This starts green (the
# one offending entry was removed with the fix in #7530), so it can only go red
# on a NEW attempt to park a crash.
parked_crashes="$("$PYTHON_CMD" - "$GAP_SNAPSHOT" <<'PYEOF'
import json, sys
snap = json.load(open(sys.argv[1]))
print(" ".join(sorted(
    name for name, e in (snap.get("tests") or {}).items()
    if isinstance(e, dict) and e.get("status") == "crash"
)))
PYEOF
)"
if [[ -n "$parked_crashes" ]]; then
  echo "" >&2
  echo "ERROR: the gap snapshot accepts a CRASH, which it must never do:" >&2
  for t in $parked_crashes; do echo "  - $t" >&2; done
  echo "" >&2
  echo "A crash is a hard defect (SIGSEGV/SIGABRT/timeout), not a cosmetic gap." >&2
  echo "Fix it, or track it as an OPEN defect and remove the test from the" >&2
  echo "snapshot -- do not launder it through the expected-output channel." >&2
  exit 2
fi

# Snapshot ratchet. UPDATE_SNAPSHOT=1 accepts the current state instead of
# gating on it; commit the resulting test-parity/gap_snapshot.json diff.
if [[ "${UPDATE_SNAPSHOT:-0}" == "1" ]]; then
  exec "$PYTHON_CMD" scripts/gap_snapshot.py update --report "$REPORT" --snapshot "$GAP_SNAPSHOT"
fi
exec "$PYTHON_CMD" scripts/gap_snapshot.py check --report "$REPORT" --snapshot "$GAP_SNAPSHOT"
