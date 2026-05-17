#!/usr/bin/env bash
# V8-backed jsruntime promise-surface regression tests.
#
# Compiles and runs the five native/V8 promise fixtures with profiling enabled.
# Each run must print the expected stdout marker, emit a jsruntime profile line,
# and keep legacy_blocking_awaits at exactly zero.
#
# Usage:
#   scripts/run_jsruntime_promise_tests.sh
#   PERRY_BIN=./target/debug/perry scripts/run_jsruntime_promise_tests.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

if [[ -n "${PERRY_BIN:-}" ]]; then
    if [[ ! -x "$PERRY_BIN" ]]; then
        echo "error: PERRY_BIN is not executable: $PERRY_BIN" >&2
        exit 2
    fi
    PERRY_CMD=("$PERRY_BIN")
else
    PERRY_CMD=(cargo run --quiet --bin perry --)
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/perry-jsruntime-promises.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

PASS=0
FAIL=0

dump_file() {
    local path="$1"
    if [[ -s "$path" ]]; then
        sed 's/^/    /' "$path"
    fi
}

run_fixture() {
    local name="$1"
    local src_path="$2"
    local expected_stdout="$3"

    local bin="$TMP_DIR/$name"
    local compile_log="$TMP_DIR/$name.compile.log"
    local stdout_path="$TMP_DIR/$name.stdout"
    local stderr_path="$TMP_DIR/$name.stderr"
    local profile_path="$TMP_DIR/$name.profile"

    if ! "${PERRY_CMD[@]}" "$src_path" -o "$bin" >"$compile_log" 2>&1; then
        echo "FAIL $name: compile failed"
        dump_file "$compile_log"
        FAIL=$((FAIL + 1))
        return
    fi

    PERRY_JSRUNTIME_PROFILE=1 "$bin" >"$stdout_path" 2>"$stderr_path"
    local exit_code=$?
    if [[ "$exit_code" -ne 0 ]]; then
        echo "FAIL $name: runtime exited $exit_code"
        echo "  stdout:"
        dump_file "$stdout_path"
        echo "  stderr:"
        dump_file "$stderr_path"
        FAIL=$((FAIL + 1))
        return
    fi

    if ! grep -qF "$expected_stdout" "$stdout_path"; then
        echo "FAIL $name: stdout missing expected snippet"
        echo "  expected: $expected_stdout"
        echo "  actual stdout:"
        dump_file "$stdout_path"
        FAIL=$((FAIL + 1))
        return
    fi

    grep -F "[jsruntime-profile]" "$stderr_path" >"$profile_path" || true
    if [[ ! -s "$profile_path" ]]; then
        echo "FAIL $name: missing jsruntime profile line"
        echo "  stderr:"
        dump_file "$stderr_path"
        FAIL=$((FAIL + 1))
        return
    fi

    if grep -Ev '(^|[[:space:]])legacy_blocking_awaits=0([[:space:]]|$)' "$profile_path" >/dev/null; then
        echo "FAIL $name: legacy_blocking_awaits was not zero"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        return
    fi

    if ! grep -Eq '(^|[[:space:]])v8_entries_total=[1-9][0-9]*([[:space:]]|$)' "$profile_path"; then
        echo "FAIL $name: v8_entries_total was missing or zero"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        return
    fi

    echo "PASS $name"
    PASS=$((PASS + 1))
}

run_fixture await_pending_promise \
    test-files/test_jsruntime_await_pending_promise.ts \
    "awaited: v8-pending"

run_fixture rejection_propagation \
    test-files/test_jsruntime_rejection_propagation.ts \
    "caught: v8-reject"

run_fixture mixed_native_v8_promise_all \
    test-files/test_jsruntime_mixed_native_v8_promise_all.ts \
    "all: native-first v8-second"

run_fixture callback_returns_native_promise \
    test-files/test_jsruntime_callback_returns_native_promise.ts \
    "callback: callback:inner-native"

run_fixture module_eval_async \
    test-files/test_jsruntime_module_eval_async.ts \
    "module: module-ready"

echo
echo "jsruntime-promise-tests: $PASS passed, $FAIL failed"

if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat >"$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_jsruntime_promise_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

[[ "$FAIL" -eq 0 ]]
