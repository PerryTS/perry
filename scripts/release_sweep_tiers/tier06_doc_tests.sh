#!/usr/bin/env bash
# Tier 6 — doc_tests
#
# Wraps scripts/run_doc_tests.sh on macOS / Linux. Windows uses
# run_doc_tests.ps1 — Windows-host wiring is bundled with step 7 of the
# rollout (smoke_windows_app.ps1 + release_sweep.ps1).

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 6 "doc_tests" "$REPO_ROOT/scripts/run_doc_tests.sh"
