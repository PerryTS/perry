#!/usr/bin/env bash
# Tier 0 — build_matrix
#
# Verifies the entire Rust workspace builds cleanly on the host, with the
# CLAUDE.md-specified UI exclusions for cross-host crates that won't link
# without their target SDKs.
#
# Scope intentionally narrow: this tier proves "the source compiles on
# this machine." Cross-target builds of Perry-the-CLI are not in scope —
# we ship a host-only perry binary, and the actual user-facing
# cross-compile pipeline (perry --target X) is verified by tier 12.
#
# Why a separate tier and not just `cargo test`'s implicit build step:
# tier 1 (cargo_workspace) excludes the cross-host UI crates by name from
# `cargo test`, which means a regression in perry-ui-ios on a macOS host
# would surface in tier 0 but not tier 1. Splitting the two makes the
# blast radius of each step explicit in the report.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
TIER_DIR="$(sweep_tier_dir "$OUT" 0)"
LOG="$TIER_DIR/build_matrix.log"
SUMMARY="$TIER_DIR/summary.json"

host="$(sweep_host_detect)"

# Cross-host UI exclusions per CLAUDE.md. Excluding ALL of them on every
# host is the simplest correct rule: any cross-compile of a non-host UI
# crate has its own toolchain prerequisites that don't belong here. The
# host's own UI crate (perry-ui-macos on macOS, perry-ui-gtk4 on Linux,
# perry-ui-windows on Windows) is left in.
EXCLUDES_COMMON=(
    --exclude perry-ui-ios
    --exclude perry-ui-tvos
    --exclude perry-ui-watchos
    --exclude perry-ui-visionos
    --exclude perry-ui-android
)
case "$host" in
    macos)
        EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-windows --exclude perry-ui-gtk4)
        ;;
    linux)
        EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-macos --exclude perry-ui-windows)
        ;;
    windows)
        EXCLUDES=("${EXCLUDES_COMMON[@]}" --exclude perry-ui-macos --exclude perry-ui-gtk4)
        ;;
    *)
        EXCLUDES=("${EXCLUDES_COMMON[@]}")
        ;;
esac

start="$(date +%s)"
{
    echo "tier 0 build_matrix — host=$host"
    echo "command: cargo build --release --workspace ${EXCLUDES[*]}"
    echo
} > "$LOG"

set +e
(cd "$REPO_ROOT" && cargo build --release --workspace "${EXCLUDES[@]}") >> "$LOG" 2>&1
rc=$?
set -e

end="$(date +%s)"
dur="$((end - start))"

if [[ "$rc" -eq 0 ]]; then
    cat > "$SUMMARY" <<EOF
{"script": "tier00_build_matrix.sh", "passed": 1, "failed": 0, "skipped": 0, "host": "$host"}
EOF
    sweep_tier_emit "$OUT" 0 "build_matrix" "PASS" "$dur" "workspace built ($host)"
else
    cat > "$SUMMARY" <<EOF
{"script": "tier00_build_matrix.sh", "passed": 0, "failed": 1, "skipped": 0, "host": "$host", "exit_code": $rc}
EOF
    sweep_tier_emit "$OUT" 0 "build_matrix" "FAIL" "$dur" "cargo build exited $rc — see log"
fi
