#!/usr/bin/env bash
# Tier 10 — android_emu
#
# Wraps scripts/run_android_emu_tests.sh. The underlying script handles
# AVD boot, install, launch, logcat scraping, teardown. This tier wrapper
# adds host-gating and clean SKIP detection so a Mac/Linux host without
# the Android SDK reports SKIP with a clear reason rather than a noisy
# fail.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 10)"
LOG="$TIER_DIR/android_emu.log"
SUMMARY="$TIER_DIR/summary.json"

# Precondition: SDK + emulator + adb + at least one AVD
SDK="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$SDK" ]]; then
    sweep_tier_emit "$OUT" 10 "android_emu" "SKIP" 0 "ANDROID_HOME / ANDROID_SDK_ROOT not set"
    exit 0
fi
if [[ ! -x "$SDK/emulator/emulator" ]] && ! command -v emulator >/dev/null 2>&1; then
    sweep_tier_emit "$OUT" 10 "android_emu" "SKIP" 0 "emulator binary not found in \$ANDROID_HOME/emulator/ or PATH"
    exit 0
fi
if [[ ! -x "$SDK/platform-tools/adb" ]] && ! command -v adb >/dev/null 2>&1; then
    sweep_tier_emit "$OUT" 10 "android_emu" "SKIP" 0 "adb not found"
    exit 0
fi

start="$(date +%s)"
set +e
PERRY_TEST_SUMMARY_OUT="$SUMMARY" \
    "$REPO_ROOT/scripts/run_android_emu_tests.sh" > "$LOG" 2>&1
rc=$?
set -e
end="$(date +%s)"
dur="$((end - start))"

# Distinguish "the script failed because preconditions were missed
# (exit 2)" — which is also a SKIP — from real test failures (exit 1).
if [[ "$rc" -eq 2 ]]; then
    reason="$(grep -m1 -E '^android-emu:' "$LOG" 2>/dev/null | sed 's/android-emu: //')"
    sweep_tier_emit "$OUT" 10 "android_emu" "SKIP" "$dur" "${reason:-precondition not met}"
    exit 0
fi

if [[ -f "$SUMMARY" ]]; then
    passed="$(sed -nE 's/.*"passed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$SUMMARY" | head -n1)"
    failed="$(sed -nE 's/.*"failed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$SUMMARY" | head -n1)"
    msg="${passed:-?} passed / ${failed:-?} failed"
else
    passed=0
    failed=1
    msg="no summary file written"
fi

if [[ "$rc" -eq 0 && "${failed:-0}" -eq 0 ]]; then
    sweep_tier_emit "$OUT" 10 "android_emu" "PASS" "$dur" "$msg"
else
    sweep_tier_emit "$OUT" 10 "android_emu" "FAIL" "$dur" "$msg (script exit=$rc)"
fi
