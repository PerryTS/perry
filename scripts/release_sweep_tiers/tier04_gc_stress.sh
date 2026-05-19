#!/usr/bin/env bash
# Tier 4 — gc_stress
#
# Wraps scripts/run_memory_stability_tests.sh. Today's wiring runs the
# default GC mode (generational, generated write barriers on, policy evac).
# The underlying script also loops mark-sweep, explicit gen-GC, and forced
# evacuation verifier modes internally, but this tier wrapper currently only
# captures the aggregate pass/fail count. Per-mode breakout will land when
# the script gains a JSON-array summary (single PERRY_TEST_SUMMARY_OUT
# entry per mode would be a richer shape — left for a later iteration).

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 4 "gc_stress" "$REPO_ROOT/scripts/run_memory_stability_tests.sh"
