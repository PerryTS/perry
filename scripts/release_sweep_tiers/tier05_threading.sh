#!/usr/bin/env bash
# Tier 5 — threading
#
# Wraps scripts/run_thread_tests.sh. The script exercises the perry/thread
# borrow/aliasing rules and parallelMap/parallelFilter happy paths.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 5 "threading" "$REPO_ROOT/scripts/run_thread_tests.sh"
