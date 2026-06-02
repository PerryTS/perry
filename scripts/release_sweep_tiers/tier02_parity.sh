#!/usr/bin/env bash
# Tier 2 — parity
#
# Wraps ./run_parity_tests.sh. The patched script writes a flat summary to
# PERRY_TEST_SUMMARY_OUT; sweep_tier_run_summary parses it and emits the
# tier result. Standalone parity runs are unaffected — env var unset → no-op.
#
# Notes:
#   - Parity has its own 80%-threshold exit-1 gate. If the suite drops below
#     that, the tier reports FAIL (rc=1) but the summary still records the
#     real passed/failed counts so the report shows what regressed.
#   - --quick mode is not yet plumbed into run_parity_tests.sh; the suite
#     runs in full each tier invocation (~minutes on this branch). Future
#     work: thread PERRY_RELEASE_SWEEP_QUICK through to limit to gap suite.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 2 "parity" "$REPO_ROOT/run_parity_tests.sh"
