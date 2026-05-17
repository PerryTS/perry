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

DEFAULT_FIXTURES=(
    test-files/test_async.ts
    test-files/test_edge_promises.ts
    test-files/test_microtask_inv_07_promise_all_mixed.ts
    test-files/test_spread.ts
    test-files/test_math.ts
)

if [[ "$#" -gt 0 ]]; then
    FIXTURES=("$@")
else
    FIXTURES=("${DEFAULT_FIXTURES[@]}")
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
    local src_path="$1"
    local name
    name="$(basename "${src_path%.ts}")"

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

for fixture in "${FIXTURES[@]}"; do
    run_fixture "$fixture"
done

echo
echo "native-no-fallback-tests: $PASS passed, $FAIL failed"

if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat >"$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_native_no_fallback_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

[[ "$FAIL" -eq 0 ]]
