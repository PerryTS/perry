#!/usr/bin/env bash
# Tier 7 — ui_host_smoke
#
# Wraps scripts/run_ui_styling_matrix.sh. This tier specifically checks
# that the styling matrix is in sync with every backend's lib.rs — a
# catch-net for backend regressions that don't surface as compile errors.
#
# Per-host launch of each docs/examples/ui/* under PERRY_UI_TEST_MODE=1 is
# a richer follow-on (the existing scripts only validate the matrix). For
# the first 0.6.0 sweep the matrix check is the load-bearing assertion.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 7 "ui_host_smoke" "$REPO_ROOT/scripts/run_ui_styling_matrix.sh"
