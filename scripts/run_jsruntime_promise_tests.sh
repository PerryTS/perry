#!/usr/bin/env bash
# V8-backed jsruntime promise-surface regression tests.
#
# Compiles and runs the native/V8 promise fixtures with profiling enabled.
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

TIMEOUT_BIN=""
if command -v timeout >/dev/null 2>&1; then
    TIMEOUT_BIN="$(command -v timeout)"
elif command -v gtimeout >/dev/null 2>&1; then
    TIMEOUT_BIN="$(command -v gtimeout)"
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

profile_value() {
    local profile_path="$1"
    local key="$2"
    tr ' ' '\n' <"$profile_path" | sed -n "s/^${key}=//p" | tail -n 1
}

require_profile_nonzero() {
    local name="$1"
    local profile_path="$2"
    local key="$3"
    if ! grep -Eq "(^|[[:space:]])${key}=[1-9][0-9]*([[:space:]]|$)" "$profile_path"; then
        echo "FAIL $name: $key was missing or zero"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        return 1
    fi
    return 0
}

require_profile_value() {
    local name="$1"
    local profile_path="$2"
    local key="$3"
    local expected="$4"
    local value
    value="$(profile_value "$profile_path" "$key")"
    if [[ -z "$value" || "$value" -ne "$expected" ]]; then
        echo "FAIL $name: expected $key=$expected, saw ${value:-missing}"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        return 1
    fi
    return 0
}

stdout_occurrences() {
    local path="$1"
    local needle="$2"
    grep -oF "$needle" "$path" | wc -l | tr -d '[:space:]'
}

require_stdout_once() {
    local name="$1"
    local stdout_path="$2"
    local needle="$3"
    local count
    count="$(stdout_occurrences "$stdout_path" "$needle")"
    if [[ "$count" -ne 1 ]]; then
        echo "FAIL $name: expected exactly one stdout occurrence of '$needle', saw $count"
        dump_file "$stdout_path"
        FAIL=$((FAIL + 1))
        return 1
    fi
    return 0
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

    if ! "${PERRY_CMD[@]}" "$src_path" --no-cache -o "$bin" >"$compile_log" 2>&1; then
        echo "FAIL $name: compile failed"
        dump_file "$compile_log"
        FAIL=$((FAIL + 1))
        return
    fi

    if [[ -n "$TIMEOUT_BIN" ]]; then
        PERRY_JSRUNTIME_PROFILE=1 "$TIMEOUT_BIN" 30 "$bin" >"$stdout_path" 2>"$stderr_path"
    else
        PERRY_JSRUNTIME_PROFILE=1 "$bin" >"$stdout_path" 2>"$stderr_path"
    fi
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

    if [[ "$name" == "mixed_rejection_semantics_matrix" ]]; then
        if grep -Eq 'missing|undefined|duplicate|wrong' "$stdout_path"; then
            echo "FAIL $name: stdout contained forbidden rejection marker"
            dump_file "$stdout_path"
            FAIL=$((FAIL + 1))
            return
        fi
        require_stdout_once "$name" "$stdout_path" "v8-catch:v8-direct" || return
        require_stdout_once "$name" "$stdout_path" "native-callback-caught:native-callback" || return
        require_stdout_once "$name" "$stdout_path" "all-catch:v8-all" || return
        require_stdout_once "$name" "$stdout_path" "late:v8-late:v8-late" || return
        require_stdout_once "$name" "$stdout_path" "counts: 1,1,1,1" || return
        if grep -Eiq 'Unhandled|unhandled|\[jsruntime_pump\] event loop error' "$stderr_path"; then
            echo "FAIL $name: stderr contained unintended unhandled rejection output"
            dump_file "$stderr_path"
            FAIL=$((FAIL + 1))
            return
        fi
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

    if grep -Ev '(^|[[:space:]])legacy_blocking_await_entries=0([[:space:]]|$)' "$profile_path" >/dev/null; then
        echo "FAIL $name: legacy_blocking_await_entries was not zero"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        return
    fi

    if grep -Ev '(^|[[:space:]])blocking_module_evals=0([[:space:]]|$)' "$profile_path" >/dev/null; then
        echo "FAIL $name: blocking_module_evals was not zero"
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

    if [[ "$name" == "module_eval_pump_liveness" ]]; then
        require_profile_nonzero "$name" "$profile_path" pump_ticks || return
        require_profile_nonzero "$name" "$profile_path" module_evals_started || return
        require_profile_nonzero "$name" "$profile_path" module_evals_resolved || return
    fi

    if [[ "$name" == "foreign_promise_handle_stress" || "$name" == "async_liveness_stress" ]]; then
        local created resolved rejected retained released
        created="$(profile_value "$profile_path" adapters_created)"
        resolved="$(profile_value "$profile_path" adapters_resolved)"
        rejected="$(profile_value "$profile_path" adapters_rejected)"
        retained="$(profile_value "$profile_path" foreign_promise_handles_retained)"
        released="$(profile_value "$profile_path" foreign_promise_handles_released)"
        if [[ -z "$created" || -z "$resolved" || -z "$rejected" || -z "$retained" || -z "$released" ]]; then
            echo "FAIL $name: missing adapter/handle balance counters"
            dump_file "$profile_path"
            FAIL=$((FAIL + 1))
            return
        fi
        if [[ "$created" -ne $((resolved + rejected)) ]]; then
            echo "FAIL $name: adapter settlement imbalance created=$created resolved=$resolved rejected=$rejected"
            dump_file "$profile_path"
            FAIL=$((FAIL + 1))
            return
        fi
        if [[ "$retained" -ne 0 ]]; then
            echo "FAIL $name: foreign promise handles retained=$retained"
            dump_file "$profile_path"
            FAIL=$((FAIL + 1))
            return
        fi
        if [[ "$released" -le 0 ]]; then
            echo "FAIL $name: no foreign promise handles were released"
            dump_file "$profile_path"
            FAIL=$((FAIL + 1))
            return
        fi
    fi

    if [[ "$name" == "mixed_async_ordering_matrix" ]]; then
        require_profile_nonzero "$name" "$profile_path" foreign_promise_adapters || return
        require_profile_nonzero "$name" "$profile_path" callback_invokes || return
        require_profile_nonzero "$name" "$profile_path" native_promise_resolves || return
    fi

    if [[ "$name" == "mixed_rejection_semantics_matrix" ]]; then
        require_profile_nonzero "$name" "$profile_path" adapters_rejected || return
        require_profile_nonzero "$name" "$profile_path" callback_invokes || return
        require_profile_nonzero "$name" "$profile_path" native_promise_rejects || return
    fi

    if [[ "$name" == "module_load_cache_counter" ]]; then
        require_profile_value "$name" "$profile_path" module_loads 1 || return
        require_profile_nonzero "$name" "$profile_path" function_calls || return
        require_profile_nonzero "$name" "$profile_path" foreign_promise_adapters || return
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

run_fixture module_eval_pump_liveness \
    test-files/test_jsruntime_module_eval_pump_liveness.ts \
    "module-pump: ready"

run_fixture module_load_cache_counter \
    test-files/test_jsruntime_module_load_cache_counter.ts \
    "module-cache-counter: 20"

run_fixture foreign_promise_handle_stress \
    test-files/test_jsruntime_foreign_promise_handle_stress.ts \
    "foreign-stress: 1000 same same same|same"

run_fixture mixed_async_ordering_matrix \
    test-files/test_jsruntime_mixed_async_ordering_matrix.ts \
    "ordering: sync>after-native-await>before-v8>native-micro>v8:one>before-timer>timer:done>callback:callback:inner>all:native|v8|timer"

run_fixture mixed_rejection_semantics_matrix \
    test-files/test_jsruntime_mixed_rejection_semantics_matrix.ts \
    "rejections: v8-catch:v8-direct | native-callback-caught:native-callback | all-catch:v8-all | late:v8-late:v8-late | counts: 1,1,1,1"

run_fixture async_liveness_stress \
    test-files/test_jsruntime_async_liveness_stress.ts \
    "liveness-stress: 800"

echo
echo "jsruntime-promise-tests: $PASS passed, $FAIL failed"

if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat >"$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_jsruntime_promise_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

[[ "$FAIL" -eq 0 ]]
