#!/usr/bin/env bash
# Native-first regression gate.
#
# Compiles and runs a curated set of TypeScript-only fixtures with
# PERRY_JSRUNTIME_PROFILE=1. Each fixture must compile with zero JavaScript
# modules and must not emit a jsruntime profile line at runtime. A profile line
# means the V8 fallback was linked and initialized.
#
# Usage:
#   scripts/run_native_no_fallback_tests.sh
#   PERRY_BIN=./target/release/perry scripts/run_native_no_fallback_tests.sh
#   scripts/run_native_no_fallback_tests.sh test-files/test_async.ts ...

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

DEFAULT_FIXTURE_NAMES=(
    async
    edge-promises
    microtask-07-promise-all-mixed
    spread
    math
    async-chain
    microtask-01
    microtask-02
    microtask-03
    microtask-04
    microtask-05
    microtask-06
    edge-arrays
    edge-objects-records
    edge-classes
    edge-closures
    edge-control-flow
    edge-destructuring
    edge-map-set
    edge-json-regex
    edge-strings
    edge-type-coercion
    optional-chain
    try-catch
    map
    set
    json
    rest-params
    multi-basic
    multi-edge
    jsruntime-stub-no-v8
    compat-core
    compat-objects-symbols
    compat-strings-regex-json
    compat-url-date-math
)

DEFAULT_FIXTURE_PATHS=(
    test-files/test_async.ts
    test-files/test_edge_promises.ts
    test-files/test_microtask_inv_07_promise_all_mixed.ts
    test-files/test_spread.ts
    test-files/test_math.ts
    test-files/test_async_chain.ts
    test-files/test_microtask_inv_01_two_fn_interleave.ts
    test-files/test_microtask_inv_02_then_vs_await_fifo.ts
    test-files/test_microtask_inv_03_chained_then_interleave.ts
    test-files/test_microtask_inv_04_await_caller_callee.ts
    test-files/test_microtask_inv_05_nested_promise_unwrap.ts
    test-files/test_microtask_inv_06_finally_after_await.ts
    test-files/test_edge_arrays.ts
    test-files/test_edge_objects_records.ts
    test-files/test_edge_classes.ts
    test-files/test_edge_closures.ts
    test-files/test_edge_control_flow.ts
    test-files/test_edge_destructuring.ts
    test-files/test_edge_map_set.ts
    test-files/test_edge_json_regex.ts
    test-files/test_edge_strings.ts
    test-files/test_edge_type_coercion.ts
    test-files/test_optional_chain.ts
    test-files/test_try_catch.ts
    test-files/test_map.ts
    test-files/test_set.ts
    test-files/test_json.ts
    test-files/test_rest_params.ts
    test-files/multi/index.ts
    test-files/multi-edge/index.ts
    test-files/test_issue_257_jsruntime_stub_no_v8.ts
    test-files/test_compat_core_surface.ts
    test-files/test_compat_objects_symbols.ts
    test-files/test_compat_strings_regex_json.ts
    test-files/test_compat_url_date_math.ts
)

EXCLUDED_FALLBACK_FIXTURES=(
    test-files/test_jsruntime_*.ts
    test-files/test_issue_248_phase2_js_interop.ts
    test-files/test_issue_248_phase2b_js_callback.ts
    test-files/test_issue_255_jsruntime_reentrancy.ts
    test-files/test_issue_678_v8_fallback.ts
    test-files/test_decorators_nest_js_common_canary.ts
)

if [[ "${#DEFAULT_FIXTURE_NAMES[@]}" -ne "${#DEFAULT_FIXTURE_PATHS[@]}" ]]; then
    echo "error: native no-fallback manifest names/paths length mismatch" >&2
    exit 2
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/perry-native-no-fallback.XXXXXX")"
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

    local bin="$TMP_DIR/$name"
    local compile_log="$TMP_DIR/$name.compile.log"
    local stdout_path="$TMP_DIR/$name.stdout"
    local stderr_path="$TMP_DIR/$name.stderr"

    if [[ ! -f "$src_path" ]]; then
        echo "FAIL $name: missing fixture $src_path"
        FAIL=$((FAIL + 1))
        return
    fi

    if ! "${PERRY_CMD[@]}" "$src_path" --no-cache -o "$bin" >"$compile_log" 2>&1; then
        echo "FAIL $name: compile failed"
        dump_file "$compile_log"
        FAIL=$((FAIL + 1))
        return
    fi

    if ! grep -Eq 'Found [0-9]+ module\(s\): [0-9]+ native, 0 JavaScript' "$compile_log"; then
        echo "FAIL $name: compile did not report zero JavaScript modules"
        grep -E 'Found [0-9]+ module\(s\):|JavaScript|V8' "$compile_log" | sed 's/^/    /' || true
        FAIL=$((FAIL + 1))
        return
    fi

    if grep -qF "Using V8 JavaScript runtime" "$compile_log"; then
        echo "FAIL $name: compile linked the V8 jsruntime fallback"
        grep -F "Using V8 JavaScript runtime" "$compile_log" | sed 's/^/    /'
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

    if grep -qF "[jsruntime-profile]" "$stderr_path"; then
        echo "FAIL $name: runtime entered jsruntime fallback"
        grep -F "[jsruntime-profile]" "$stderr_path" | sed 's/^/    /'
        FAIL=$((FAIL + 1))
        return
    fi

    echo "PASS $name"
    PASS=$((PASS + 1))
}

if [[ "$#" -gt 0 ]]; then
    for fixture in "$@"; do
        run_fixture "$(basename "${fixture%.ts}")" "$fixture"
    done
else
    echo "native-no-fallback excluded fallback fixtures:"
    for fixture in "${EXCLUDED_FALLBACK_FIXTURES[@]}"; do
        echo "  $fixture"
    done
    echo
    for i in "${!DEFAULT_FIXTURE_PATHS[@]}"; do
        run_fixture "${DEFAULT_FIXTURE_NAMES[$i]}" "${DEFAULT_FIXTURE_PATHS[$i]}"
    done
fi

echo
echo "native-no-fallback-tests: $PASS passed, $FAIL failed"

if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat >"$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_native_no_fallback_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

[[ "$FAIL" -eq 0 ]]
