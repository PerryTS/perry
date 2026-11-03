#!/usr/bin/env bash
# The parity harness must refuse to run when it cannot normalize.
#
# Both comparison call sites in `run_parity_tests.sh` run the normalizer inside
# `$( … )`. If that process fails to start, its stdout is empty on BOTH sides,
# the two sides compare equal, and every test in the suite reports PASS — a gate
# that cannot fail. The harness therefore probes the normalizer once at startup
# and exits 1 instead. This test drives that probe in a sandbox, because the
# failure it prevents is invisible in a green run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Same override the compile-budget test uses, so a mutation of the guard can be
# driven from a scratch copy instead of editing the tree under test.
SCRIPT="${PARITY_SCRIPT_UNDER_TEST:-$ROOT/run_parity_tests.sh}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail() { echo "ASSERTION FAILED: $*" >&2; [ -f "$WORK/out" ] && cat "$WORK/out" >&2; exit 1; }

# A hermetic copy of the harness: the script resolves the module as
# "$SCRIPT_DIR/scripts/parity_normalize.py", so staging decides the outcome.
stage() {
    rm -rf "$WORK/run"
    mkdir -p "$WORK/run/scripts" "$WORK/run/test-files" \
        "$WORK/run/test-parity/output/node" "$WORK/run/test-parity/output/perry" \
        "$WORK/run/test-parity/reports"
    cp "$SCRIPT" "$WORK/run/run_parity_tests.sh"
    chmod +x "$WORK/run/run_parity_tests.sh"
}

run_harness() {
    set +e
    PERRY_SKIP_BUILD=1 "$WORK/run/run_parity_tests.sh" --filter no-such-test >"$WORK/out" 2>&1
    status=$?
    set -e
}

# 1. The module is absent. Every comparison would be empty-vs-empty.
stage
run_harness
[ "$status" -eq 1 ] || fail "missing module: expected exit 1, got $status"
grep -qF "FATAL:" "$WORK/out" || fail "missing module: no FATAL message"
grep -qF "is missing" "$WORK/out" || fail "missing module: message does not name the cause"
# Liveness, not just the message: a guard that prints and then keeps going is a
# gate that cannot fail. Reaching the banner proves the harness continued.
if grep -qF "Perry Parity Test Runner (LLVM)" "$WORK/out"; then
    fail "missing module: the harness continued past the startup guard"
fi

# 2. The module is present but cannot normalize. A file on disk is not a
#    working normalizer, and this is the case a "does the file exist" check
#    would wave through.
stage
printf 'import sys\nsys.exit(1)\n' > "$WORK/run/scripts/parity_normalize.py"
run_harness
[ "$status" -eq 1 ] || fail "broken module: expected exit 1, got $status"
grep -qF "FATAL:" "$WORK/out" || fail "broken module: no FATAL message"
grep -qF "cannot normalize" "$WORK/out" || fail "broken module: message does not name the cause"
if grep -qF "Perry Parity Test Runner (LLVM)" "$WORK/out"; then
    fail "broken module: the harness continued past the startup guard"
fi

# 3. The guard must not be a wall: a working module gets through. The run fails
#    afterwards for the sandbox's own reasons (no archives), so the assertion is
#    only that the guard did not reject a healthy normalizer.
stage
cp "$ROOT/scripts/parity_normalize.py" "$WORK/run/scripts/parity_normalize.py"
run_harness
if grep -qF "FATAL:" "$WORK/out"; then
    fail "a healthy normalizer was rejected by the startup probe"
fi
# And it must proceed: the guard is a probe, not a wall.
grep -qF "Perry Parity Test Runner (LLVM)" "$WORK/out" \
    || fail "a healthy normalizer did not let the harness start"

echo "PASS"
