#!/usr/bin/env bash
# Tier 8 — sim_apple
#
# Runs the generalized scripts/run_simctl_tests.sh with PLATFORM in
# {ios, tvos, visionos}. Each platform contributes its own bucket; the
# tier reports PASS iff every reachable platform passed and no SDK that
# we expected to be present was missing.
#
# Per-platform skip: if `xcrun --sdk <sdk-name> --show-sdk-path` fails,
# that platform is skipped with a clear reason. Failures from the simctl
# script itself (compile fail, install fail, runtime crash) are real
# regressions and count as FAIL.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 8)"
LOG="$TIER_DIR/sim_apple.log"
SUMMARY="$TIER_DIR/summary.json"

if ! command -v xcrun >/dev/null 2>&1; then
    sweep_tier_emit "$OUT" 8 "sim_apple" "FAIL" 0 "xcrun not on PATH (Xcode CLT not installed?)"
    exit 0
fi

# (PLATFORM, sdk-name-for-precondition-check)
PLATFORMS=(
    "ios|iphonesimulator"
    "tvos|appletvsimulator"
    "visionos|xrsimulator"
)

start="$(date +%s)"
declare -i total_pass=0
declare -i total_fail=0
declare -i total_skip=0
declare -a passed_platforms=()
declare -a failed_platforms=()
declare -a skipped_platforms=()

{
    echo "tier 8 sim_apple"
    echo
} > "$LOG"

for entry in "${PLATFORMS[@]}"; do
    IFS='|' read -r platform sdk_name <<< "$entry"
    {
        echo "=== platform: $platform (SDK $sdk_name) ==="
    } >> "$LOG"

    # Precondition: SDK installed?
    if ! xcrun --sdk "$sdk_name" --show-sdk-path >/dev/null 2>&1; then
        echo "  SKIP — $sdk_name SDK not found" >> "$LOG"
        total_skip+=1
        skipped_platforms+=("$platform")
        continue
    fi

    per_platform_summary="$TIER_DIR/${platform}-summary.json"
    set +e
    PLATFORM="$platform" \
    PERRY_TEST_SUMMARY_OUT="$per_platform_summary" \
        "$REPO_ROOT/scripts/run_simctl_tests.sh" >> "$LOG" 2>&1
    rc=$?
    set -e

    if [[ -f "$per_platform_summary" ]]; then
        p="$(sed -nE 's/.*"passed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$per_platform_summary" | head -n1)"
        f="$(sed -nE 's/.*"failed"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$per_platform_summary" | head -n1)"
        : "${p:=0}"; : "${f:=0}"
    else
        p=0
        # If no summary file, the script crashed before its emit — count as 1 failed.
        f=1
    fi

    {
        echo "  result: $p passed / $f failed (script exit=$rc)"
    } >> "$LOG"

    if [[ "$rc" -eq 0 && "$f" -eq 0 ]]; then
        total_pass+=$p
        passed_platforms+=("$platform")
    else
        total_fail+=$f
        failed_platforms+=("$platform")
    fi
done

end="$(date +%s)"
dur="$((end - start))"

{
    echo
    echo "=== sim_apple summary ==="
    echo "  platforms passed:  ${passed_platforms[@]+${passed_platforms[@]}}"
    echo "  platforms failed:  ${failed_platforms[@]+${failed_platforms[@]}}"
    echo "  platforms skipped: ${skipped_platforms[@]+${skipped_platforms[@]}}"
    echo "  total passed: $total_pass"
    echo "  total failed: $total_fail"
} >> "$LOG"

cat > "$SUMMARY" <<EOF
{"script": "tier08_sim_apple.sh", "passed": $total_pass, "failed": $total_fail, "skipped": $total_skip, "platforms_passed": [$(printf '"%s",' "${passed_platforms[@]+${passed_platforms[@]}}" | sed 's/,$//')], "platforms_failed": [$(printf '"%s",' "${failed_platforms[@]+${failed_platforms[@]}}" | sed 's/,$//')], "platforms_skipped": [$(printf '"%s",' "${skipped_platforms[@]+${skipped_platforms[@]}}" | sed 's/,$//')]}
EOF

if [[ "$total_fail" -eq 0 && "${#passed_platforms[@]}" -gt 0 ]]; then
    sweep_tier_emit "$OUT" 8 "sim_apple" "PASS" "$dur" \
        "${#passed_platforms[@]} platforms passed / ${#skipped_platforms[@]} skipped (no SDK)"
elif [[ "${#passed_platforms[@]}" -eq 0 && "${#skipped_platforms[@]}" -gt 0 && "$total_fail" -eq 0 ]]; then
    sweep_tier_emit "$OUT" 8 "sim_apple" "SKIP" "$dur" \
        "all platforms skipped (no SDK installed for any of: ios/tvos/visionos)"
else
    sweep_tier_emit "$OUT" 8 "sim_apple" "FAIL" "$dur" \
        "$total_fail failures across: ${failed_platforms[*]}"
fi
