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
#   - jq, python3
#
# Usage: scripts/run_gap_tests.sh [--shard N/M]
#        UPDATE_SNAPSHOT=1 scripts/run_gap_tests.sh   # accept current state
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$ROOT"

# Run-scoped temp dir — fixed /tmp names would let concurrent runs (a second
# PR, local + CI on the same box, or the future node-suite-guard alongside)
# clobber each other's failure lists and produce a false gate result.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/perry-gap.XXXXXX")"
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

# Snapshot ratchet. UPDATE_SNAPSHOT=1 accepts the current state instead of
# gating on it; commit the resulting test-parity/gap_snapshot.json diff.
if [[ "${UPDATE_SNAPSHOT:-0}" == "1" ]]; then
  exec python3 scripts/gap_snapshot.py update --report "$REPORT"
fi
exec python3 scripts/gap_snapshot.py check --report "$REPORT"
