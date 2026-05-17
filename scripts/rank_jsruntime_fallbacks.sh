#!/usr/bin/env bash
# Rank V8 fallback entry counters for selected Perry fixtures.
#
# Usage:
#   scripts/rank_jsruntime_fallbacks.sh
#   PERRY_BIN=./target/release/perry scripts/rank_jsruntime_fallbacks.sh test-files/foo.ts ...

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

DEFAULT_FIXTURES=(
    test-files/test_jsruntime_await_pending_promise.ts
    test-files/test_jsruntime_mixed_native_v8_promise_all.ts
    test-files/test_jsruntime_callback_returns_native_promise.ts
    test-files/test_jsruntime_module_eval_pump_liveness.ts
    test-files/test_jsruntime_module_load_cache_counter.ts
    test-files/test_jsruntime_foreign_promise_handle_stress.ts
    test-files/test_jsruntime_mixed_async_ordering_matrix.ts
    test-files/test_jsruntime_mixed_rejection_semantics_matrix.ts
    test-files/test_jsruntime_async_liveness_stress.ts
)

SELECTED_COUNTER="module_loads_after_cache_warmup"
SELECTED_SOURCE_COUNTER="module_loads"
SELECTED_FIXTURE_PATH="test-files/test_jsruntime_module_load_cache_counter.ts"
SELECTED_FIXTURE_NAME="$(basename "${SELECTED_FIXTURE_PATH%.ts}")"
SELECTED_EXPECTED_BASELINE=1

if [[ "$#" -gt 0 ]]; then
    FIXTURES=("$@")
else
    FIXTURES=("${DEFAULT_FIXTURES[@]}")
fi

COUNTERS=(
    module_loads
    export_gets
    function_calls
    v8_export_calls
    method_calls
    value_calls
    array_gets
    array_lengths
    object_property_gets
    handle_to_strings
    property_sets
    new_instances
    new_from_handles
    callback_creates
    native_function_registers
    callback_invokes
    native_module_property_loads
    typeof_probes
    handle_constructors
    should_use_runtime
    native_promise_resolves
    native_promise_rejects
    foreign_promise_adapters
    legacy_blocking_await_entries
)

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/perry-jsruntime-rank.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

ROWS="$TMP_DIR/rows.tsv"
: >"$ROWS"

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

for fixture in "${FIXTURES[@]}"; do
    name="$(basename "${fixture%.ts}")"
    bin="$TMP_DIR/$name"
    compile_log="$TMP_DIR/$name.compile.log"
    stdout_path="$TMP_DIR/$name.stdout"
    stderr_path="$TMP_DIR/$name.stderr"
    profile_path="$TMP_DIR/$name.profile"

    if [[ ! -f "$fixture" ]]; then
        echo "FAIL $name: missing fixture $fixture"
        FAIL=$((FAIL + 1))
        continue
    fi

    if ! "${PERRY_CMD[@]}" "$fixture" --no-cache -o "$bin" >"$compile_log" 2>&1; then
        echo "FAIL $name: compile failed"
        dump_file "$compile_log"
        FAIL=$((FAIL + 1))
        continue
    fi

    if [[ -n "$TIMEOUT_BIN" ]]; then
        PERRY_JSRUNTIME_PROFILE=1 "$TIMEOUT_BIN" 30 "$bin" >"$stdout_path" 2>"$stderr_path"
    else
        PERRY_JSRUNTIME_PROFILE=1 "$bin" >"$stdout_path" 2>"$stderr_path"
    fi
    exit_code=$?
    if [[ "$exit_code" -ne 0 ]]; then
        echo "FAIL $name: runtime exited $exit_code"
        dump_file "$stdout_path"
        dump_file "$stderr_path"
        FAIL=$((FAIL + 1))
        continue
    fi

    grep -F "[jsruntime-profile]" "$stderr_path" >"$profile_path" || true
    if [[ ! -s "$profile_path" ]]; then
        echo "FAIL $name: missing jsruntime profile line"
        dump_file "$stderr_path"
        FAIL=$((FAIL + 1))
        continue
    fi

    legacy_blocking="$(profile_value "$profile_path" legacy_blocking_await_entries)"
    if [[ "${legacy_blocking:-0}" -ne 0 ]]; then
        echo "FAIL $name: legacy_blocking_await_entries=$legacy_blocking"
        dump_file "$profile_path"
        FAIL=$((FAIL + 1))
        continue
    fi

    for counter in "${COUNTERS[@]}"; do
        value="$(profile_value "$profile_path" "$counter")"
        value="${value:-0}"
        printf '%s\t%s\t%s\n' "$counter" "$value" "$name" >>"$ROWS"
    done
done

if [[ "$FAIL" -ne 0 ]]; then
    exit 1
fi

selected_fixture_seen=0
selected_counter_value=0
while IFS=$'\t' read -r counter value fixture_name; do
    if [[ "$counter" == "$SELECTED_SOURCE_COUNTER" && "$fixture_name" == "$SELECTED_FIXTURE_NAME" ]]; then
        selected_fixture_seen=1
        selected_counter_value=$((selected_counter_value + value))
    fi
done <"$ROWS"

if [[ "$selected_fixture_seen" -eq 1 ]]; then
    selected_counter_observed=$((selected_counter_value - SELECTED_EXPECTED_BASELINE))
    if [[ "$selected_counter_observed" -ne 0 ]]; then
        echo "FAIL selected fallback proof: counter=$SELECTED_COUNTER fixture=$SELECTED_FIXTURE_PATH observed=$selected_counter_observed source_counter=$SELECTED_SOURCE_COUNTER source_observed=$selected_counter_value expected_baseline=$SELECTED_EXPECTED_BASELINE"
        exit 1
    fi
fi

echo "fallback-counter-ranking:"
awk -F '\t' '{ sum[$1] += $2 } END { for (counter in sum) print sum[counter] "\t" counter }' "$ROWS" \
    | sort -nr \
    | awk -F '\t' 'BEGIN { printf "%10s  %s\n", "count", "counter" } { printf "%10d  %s\n", $1, $2 }'

echo
if [[ "$selected_fixture_seen" -eq 1 ]]; then
    echo "selected-fallback-proof: counter=$SELECTED_COUNTER fixture=$SELECTED_FIXTURE_PATH observed=0 source_counter=$SELECTED_SOURCE_COUNTER source_observed=$selected_counter_value expected_baseline=$SELECTED_EXPECTED_BASELINE"
    echo
fi

echo
echo "fallback-counter-ranking-jsonl:"
awk -F '\t' '{ sum[$1] += $2 } END { for (counter in sum) print sum[counter] "\t" counter }' "$ROWS" \
    | sort -nr \
    | awk -F '\t' '{ printf "{\"counter\":\"%s\",\"count\":%d}\n", $2, $1 }'
