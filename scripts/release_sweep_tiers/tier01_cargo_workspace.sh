#!/usr/bin/env bash
# Tier 1 — cargo_workspace
#
# Runs `cargo test --release --workspace` with the CLAUDE.md UI exclusions.
# Per the canonical command in that file:
#
#   cargo test --release --workspace \
#     --exclude perry-ui-ios --exclude perry-ui-tvos --exclude perry-ui-watchos \
#     --exclude perry-ui-visionos --exclude perry-ui-android \
#     --exclude perry-ui-windows --exclude perry-ui-gtk4
#
# Linux/Windows hosts swap the host's UI crate back in (so perry-ui-gtk4 is
# tested on Linux but not macOS, etc.) — same logic as tier 0.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 1)"
LOG="$TIER_DIR/cargo_workspace.log"
SUMMARY="$TIER_DIR/summary.json"

host="$(sweep_host_detect)"

EXCLUDES_COMMON=(
    --exclude perry-ui-ios
    --exclude perry-ui-tvos
    --exclude perry-ui-watchos
    --exclude perry-ui-visionos
    --exclude perry-ui-android
)
case "$host" in
    macos)   EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-windows --exclude perry-ui-gtk4) ;;
    linux)   EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-macos --exclude perry-ui-windows) ;;
    windows) EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-macos --exclude perry-ui-gtk4) ;;
    *)       EXCLUDES=("${EXCLUDES_COMMON[@]}") ;;
esac

start="$(date +%s)"
{
    echo "tier 1 cargo_workspace — host=$host"
    echo "command: cargo test --release --workspace ${EXCLUDES[*]}"
    echo
} > "$LOG"

set +e
(cd "$REPO_ROOT" && cargo test --release --workspace "${EXCLUDES[@]}") >> "$LOG" 2>&1
rc=$?
set -e

# Try to extract per-crate test counts from the log.
# `cargo test` prints lines like "test result: ok. 12 passed; 0 failed; 0 ignored ..."
# at the end of each crate's run. We sum those.
#
# Defensive parsing: `grep -c PATTERN` exits 1 (and prints "0") on no match,
# so the naive `$(grep -c ... || echo 0)` produces multi-line output ("0\n0")
# that breaks downstream arithmetic. Capture, then validate integer.
total_passed=$(grep -cE 'test result: ok\.' "$LOG" 2>/dev/null || true)
total_failed=$(grep -cE 'test result: FAILED' "$LOG" 2>/dev/null || true)
[[ "$total_passed" =~ ^[0-9]+$ ]] || total_passed=0
[[ "$total_failed" =~ ^[0-9]+$ ]] || total_failed=0

end="$(date +%s)"
dur="$((end - start))"

cat > "$SUMMARY" <<EOF
{"script": "tier01_cargo_workspace.sh", "passed": $total_passed, "failed": $total_failed, "skipped": 0, "host": "$host", "exit_code": $rc}
EOF

if [[ "$rc" -eq 0 ]]; then
    sweep_tier_emit "$OUT" 1 "cargo_workspace" "PASS" "$dur" "$total_passed crate-suites passed"
else
    sweep_tier_emit "$OUT" 1 "cargo_workspace" "FAIL" "$dur" \
        "cargo test exited $rc ($total_failed crate-suites failed of $((total_passed + total_failed)))"
fi
