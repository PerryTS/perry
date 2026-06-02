#!/usr/bin/env bash
# Tier 3 — real_packages
#
# Wraps tests/release/packages/_harness.sh, which iterates every fixture/
# subdirectory and runs its fixture.sh. The harness emits a flat JSON
# summary that sweep_tier_run_summary parses for the tier result.
#
# This is the load-bearing "real npm packages" gate for the 0.6.0 bump.
# Per the user's preference, fixtures use mock/embedded backends (sqlite
# for drizzle, redis-server / MinIO binaries for backends, no Docker).
# Fixtures whose backend isn't reachable are expected to SKIP cleanly,
# not FAIL.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"

# Sanity: the harness requires target/release/perry. Surface a clearer
# error than the harness's own one if it's missing.
if [[ ! -x "$REPO_ROOT/target/release/perry" ]]; then
    sweep_tier_emit "$OUT" 3 "real_packages" "FAIL" 0 \
        "target/release/perry not found — run cargo build --release -p perry first"
    exit 0
fi

sweep_tier_run_summary "$OUT" 3 "real_packages" \
    "$REPO_ROOT/tests/release/packages/_harness.sh"
