#!/usr/bin/env bash
# perry/ui styling-matrix CI gate (Phase A of issue #185).
#
# Three checks:
#   1. The matrix in `crates/perry-ui/src/styling_matrix.rs` matches the
#      actual `perry_ui_*` exports in every backend's `lib.rs`. Cells
#      claiming Wired/Stub for a missing symbol — or claiming Missing
#      while the symbol is exported — both fail.
#   2. `docs/src/ui/styling-matrix.md` is up-to-date relative to the
#      source-of-truth. `--gen` rewrites it; CI runs `git diff --exit-code`
#      after this script to catch uncommitted regenerations.
#   3. `cargo test -p perry-ui` passes (matrix invariants).
#
# Usage:
#   scripts/run_ui_styling_matrix.sh

set -uo pipefail

cd "$(dirname "$0")/.."

# release_sweep.sh hook — emit a 1-test summary on every exit path.
# Standalone runs are unaffected (env var unset → no-op trap).
_summary_phase=""
_summary_rc=0
_emit_summary() {
    _summary_rc=$?
    if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
        local passed=0 failed=0
        if [[ "$_summary_rc" -eq 0 ]]; then passed=1; else failed=1; fi
        cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_ui_styling_matrix.sh", "passed": $passed, "failed": $failed, "skipped": 0, "exit_code": $_summary_rc, "phase": "$_summary_phase"}
EOF
    fi
}
trap _emit_summary EXIT

echo "[1/3] Building styling-matrix binary"
_summary_phase="build"
cargo build --quiet -p perry-ui --bin styling-matrix
status=$?
if [[ $status -ne 0 ]]; then
    echo "FAIL: cargo build -p perry-ui --bin styling-matrix failed"
    exit 1
fi

echo "[2/3] Verifying matrix vs lib.rs reality (--check)"
_summary_phase="check"
./target/debug/styling-matrix --check
status=$?
if [[ $status -ne 0 ]]; then
    echo
    echo "FAIL: matrix drift detected. Either:"
    echo "  - update crates/perry-ui/src/styling_matrix.rs to match the lib.rs files, or"
    echo "  - update the affected backend's lib.rs to match the matrix's promise."
    exit 1
fi

echo "[3/3] Regenerating docs/src/ui/styling-matrix.md (--gen)"
_summary_phase="gen"
./target/debug/styling-matrix --gen
status=$?
if [[ $status -ne 0 ]]; then
    echo "FAIL: matrix generation errored"
    exit 1
fi

echo "[4/4] Running matrix unit tests"
_summary_phase="test"
cargo test --quiet -p perry-ui
status=$?
if [[ $status -ne 0 ]]; then
    echo "FAIL: cargo test -p perry-ui"
    exit 1
fi

_summary_phase="ok"
echo "OK: styling matrix in sync with all backends"
