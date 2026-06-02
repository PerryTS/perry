#!/usr/bin/env bash
# Tier 9 — sim_watchos
#
# Same pattern as tier 8, but watchOS lives on its own tier because:
#   - It uses a different SDK (watchsimulator, not paired with iphonesimulator).
#   - It often shows divergent behavior (the watchOS frameworks subset of
#     SwiftUI / UIKit is narrower than iOS), so isolating its bucket makes
#     regressions easier to spot in the report.
#
# SKIP cleanly if the watchsimulator SDK isn't installed.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 9)"
LOG="$TIER_DIR/sim_watchos.log"
SUMMARY="$TIER_DIR/summary.json"

if ! command -v xcrun >/dev/null 2>&1; then
    sweep_tier_emit "$OUT" 9 "sim_watchos" "FAIL" 0 "xcrun not on PATH"
    exit 0
fi

if ! xcrun --sdk watchsimulator --show-sdk-path >/dev/null 2>&1; then
    sweep_tier_emit "$OUT" 9 "sim_watchos" "SKIP" 0 "watchsimulator SDK not installed"
    exit 0
fi

start="$(date +%s)"

set +e
PLATFORM="watchos" \
PERRY_TEST_SUMMARY_OUT="$SUMMARY" \
    "$REPO_ROOT/scripts/run_simctl_tests.sh" > "$LOG" 2>&1
rc=$?
set -e

end="$(date +%s)"
dur="$((end - start))"

if [[ -f "$SUMMARY" ]]; then
    passed="$(sed -nE 's/.*"passed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$SUMMARY" | head -n1)"
    failed="$(sed -nE 's/.*"failed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$SUMMARY" | head -n1)"
    msg="${passed:-?} passed / ${failed:-?} failed"
else
    msg="no summary file written"
    passed=0
    failed=1
fi

if [[ "$rc" -eq 0 && "${failed:-0}" -eq 0 ]]; then
    sweep_tier_emit "$OUT" 9 "sim_watchos" "PASS" "$dur" "$msg"
else
    sweep_tier_emit "$OUT" 9 "sim_watchos" "FAIL" "$dur" "$msg (script exit=$rc)"
fi
