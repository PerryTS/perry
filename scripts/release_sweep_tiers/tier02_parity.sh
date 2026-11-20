#!/usr/bin/env bash
# Tier 2 — parity
#
# Builds the release compiler and archives, then runs every program case with
# scripts/parity.py in auto mode. The tier fails on a harness error or a pass
# rate below 80%: sweep hosts have no snapshot of their own, so parity.py's
# snapshot verdict would fail on every known divergence.

set -uo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
. "$SCRIPT_DIR/../release_sweep_lib.sh"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

run_parity() {
    local report="${PERRY_TEST_SUMMARY_OUT%/*}/report.json" rc
    # The instrumented archives serve the cases whose parity-env sets a GC knob;
    # without them those cases fail to compile, which the pass rate never counts.
    (cd "$REPO_ROOT" && cargo build-perry --release --quiet \
        && scripts/gc_call_effects/sidecars.sh target/release \
        && scripts/build_instr_runtime.sh \
        && npm ci --ignore-scripts --no-audit --no-fund --silent) || return 2
    rm -f "$report"
    python3 "$REPO_ROOT/scripts/parity.py" run --mode auto --report "$report"
    [[ $? -eq 2 || ! -f "$report" ]] && return 2
    python3 - "$report" "$PERRY_TEST_SUMMARY_OUT" <<'PY'
import json, sys
s = json.load(open(sys.argv[1]))["summary"]
summary = {"script": "scripts/parity.py", "passed": s["parity_pass"],
           "failed": s["parity_fail"] + s["compile_fail"] + s["crash_fail"],
           "skipped": s["skipped"] + s["node_fail"], "total": s["total_run"],
           "rate_pct": s["parity_percentage"]}
open(sys.argv[2], "w").write(json.dumps(summary) + "\n")
sys.exit(0 if s["parity_percentage"] >= 80 else 1)
PY
}

OUT="${PERRY_RELEASE_SWEEP_OUTPUT:?PERRY_RELEASE_SWEEP_OUTPUT not set}"
sweep_tier_run_summary "$OUT" 2 "parity" run_parity
