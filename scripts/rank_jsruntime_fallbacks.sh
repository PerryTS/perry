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
    test-files/test_issue_248_phase2_js_interop.ts
    test-files/test_issue_248_phase2b_js_callback.ts
    test-files/test_issue_255_jsruntime_reentrancy.ts
    test-files/test_decorators_nest_js_common_canary.ts
    test-files/test_jsruntime_export_hardening.ts
    test-files/test_issue_678_v8_fallback.ts
)

SELECTED_COUNTERS=(
    module_loads_after_cache_warmup
    js_export_plain_object_property_gets
)
SELECTED_SOURCE_COUNTERS=(
    module_loads
    object_property_gets
)
SELECTED_FIXTURE_PATHS=(
    test-files/test_jsruntime_module_load_cache_counter.ts
    test-files/test_decorators_nest_js_common_canary.ts
)
SELECTED_EXPECTED_BASELINES=(
    1
    0
)

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

counter_classification() {
    local counter="$1"
    case "$counter" in
        module_loads)
            printf 'intentional\tReal JS module cache misses; selected warmup proof gates excess loads.'
            ;;
        export_gets)
            printf 'intentional\tJS-hosted module export boundary; values are classified by conversion hardening.'
            ;;
        function_calls|v8_export_calls|method_calls|value_calls)
            printf 'intentional\tCallable JS exports and V8-owned methods stay in the V8 runtime.'
            ;;
        callback_creates|callback_invokes)
            printf 'intentional\tNative callback handles are exercised by mixed native/V8 callback fixtures.'
            ;;
        native_promise_resolves|native_promise_rejects|foreign_promise_adapters)
            printf 'intentional\tMixed native/V8 promise fixtures require the unified adapter and pump path.'
            ;;
        object_property_gets)
            printf 'unsafe-to-replace\tRemaining reads are V8-owned dynamic/reentry objects; safe frozen export data is separately proven native.'
            ;;
        array_gets|array_lengths)
            printf 'unsafe-to-replace\tV8-owned arrays retain JS identity and prototype behavior.'
            ;;
        handle_to_strings|property_sets|new_instances|new_from_handles|native_function_registers|native_module_property_loads|typeof_probes|handle_constructors|should_use_runtime)
            printf 'intentional\tInterop support counter for JS handles or runtime boundary plumbing.'
            ;;
        legacy_blocking_await_entries)
            printf 'forbidden\tLegacy blocking await entry must remain zero; the promise gate also enforces this.'
            ;;
        *)
            printf 'unclassified\tCounter needs semantic review before it can be treated as accepted fallback.'
            ;;
    esac
}

require_stdout_line() {
    local name="$1"
    local stdout_path="$2"
    local expected="$3"
    if ! grep -Fxq "$expected" "$stdout_path"; then
        echo "FAIL $name: missing stdout line: $expected"
        dump_file "$stdout_path"
        return 1
    fi
}

check_fixture_semantics() {
    local name="$1"
    local stdout_path="$2"
    case "$name" in
        test_jsruntime_export_hardening)
            require_stdout_line "$name" "$stdout_path" "safe: safe 7 true nested" || return 1
            require_stdout_line "$name" "$stdout_path" "mutable: after" || return 1
            require_stdout_line "$name" "$stdout_path" "accessor: accessor:1,accessor:2" || return 1
            require_stdout_line "$name" "$stdout_path" "custom-proto: own:from-proto" || return 1
            require_stdout_line "$name" "$stdout_path" "proxy: proxy:1,proxy:2" || return 1
            require_stdout_line "$name" "$stdout_path" "array: 2:a:b" || return 1
            require_stdout_line "$name" "$stdout_path" "function: function:ok" || return 1
            require_stdout_line "$name" "$stdout_path" "promise: promise:ok" || return 1
            require_stdout_line "$name" "$stdout_path" "symbol: symbol:ok" || return 1
            require_stdout_line "$name" "$stdout_path" "deep: deep-leaf" || return 1
            require_stdout_line "$name" "$stdout_path" "tampered-intrinsics: own:tampered-proto:added" || return 1
            ;;
    esac
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

    # #499: this script intentionally pulls in V8 to rank what falls
    # through to jsruntime; passes `--enable-js-runtime` so the gate
    # honors that choice.
    if ! "${PERRY_CMD[@]}" "$fixture" --enable-js-runtime --no-cache -o "$bin" >"$compile_log" 2>&1; then
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

    if ! check_fixture_semantics "$name" "$stdout_path"; then
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

TOTALS="$TMP_DIR/totals.tsv"
awk -F '\t' '{ sum[$1] += $2 } END { for (counter in sum) print sum[counter] "\t" counter }' "$ROWS" \
    | sort -nr >"$TOTALS"

SELECTED_PROOF_LINES=()
for proof_idx in "${!SELECTED_COUNTERS[@]}"; do
    selected_counter="${SELECTED_COUNTERS[$proof_idx]}"
    selected_source_counter="${SELECTED_SOURCE_COUNTERS[$proof_idx]}"
    selected_fixture_path="${SELECTED_FIXTURE_PATHS[$proof_idx]}"
    selected_fixture_name="$(basename "${selected_fixture_path%.ts}")"
    selected_expected_baseline="${SELECTED_EXPECTED_BASELINES[$proof_idx]}"

    selected_fixture_seen=0
    selected_counter_value=0
    while IFS=$'\t' read -r counter value fixture_name; do
        if [[ "$counter" == "$selected_source_counter" && "$fixture_name" == "$selected_fixture_name" ]]; then
            selected_fixture_seen=1
            selected_counter_value=$((selected_counter_value + value))
        fi
    done <"$ROWS"

    if [[ "$selected_fixture_seen" -eq 1 ]]; then
        selected_counter_observed=$((selected_counter_value - selected_expected_baseline))
        if [[ "$selected_counter_observed" -ne 0 ]]; then
            echo "FAIL selected fallback proof: counter=$selected_counter fixture=$selected_fixture_path observed=$selected_counter_observed source_counter=$selected_source_counter source_observed=$selected_counter_value expected_baseline=$selected_expected_baseline"
            exit 1
        fi
        SELECTED_PROOF_LINES+=("selected-fallback-proof: counter=$selected_counter fixture=$selected_fixture_path observed=0 source_counter=$selected_source_counter source_observed=$selected_counter_value expected_baseline=$selected_expected_baseline")
    fi
done

echo "fallback-counter-ranking:"
awk -F '\t' 'BEGIN { printf "%10s  %s\n", "count", "counter" } { printf "%10d  %s\n", $1, $2 }' "$TOTALS"

echo
if [[ "${#SELECTED_PROOF_LINES[@]}" -gt 0 ]]; then
    printf '%s\n' "${SELECTED_PROOF_LINES[@]}"
    echo
fi

echo
echo "fallback-counter-classification:"
printf '%10s  %-32s  %-18s  %s\n' "count" "counter" "classification" "reason"
CLASSIFICATION_FAIL=0
while IFS=$'\t' read -r count counter; do
    if [[ "$count" -eq 0 ]]; then
        continue
    fi
    classification_line="$(counter_classification "$counter")"
    classification="${classification_line%%$'\t'*}"
    reason="${classification_line#*$'\t'}"
    if [[ "$classification" == "unclassified" ]]; then
        CLASSIFICATION_FAIL=$((CLASSIFICATION_FAIL + 1))
    fi
    printf '%10d  %-32s  %-18s  %s\n' "$count" "$counter" "$classification" "$reason"
done <"$TOTALS"

if [[ "$CLASSIFICATION_FAIL" -ne 0 ]]; then
    echo "FAIL fallback classification: $CLASSIFICATION_FAIL nonzero counter(s) are unclassified"
    exit 1
fi

echo
echo "fallback-fixture-matrix:"
printf '%10s  %-44s  %-30s  %s\n' "count" "fixture" "counter" "classification"
sort -t $'\t' -k2,2nr "$ROWS" | while IFS=$'\t' read -r counter value fixture_name; do
    if [[ "$value" -eq 0 ]]; then
        continue
    fi
    classification_line="$(counter_classification "$counter")"
    classification="${classification_line%%$'\t'*}"
    printf '%10d  %-44s  %-30s  %s\n' "$value" "$fixture_name" "$counter" "$classification"
done

echo
echo "fallback-counter-ranking-jsonl:"
awk -F '\t' '{ printf "{\"counter\":\"%s\",\"count\":%d}\n", $2, $1 }' "$TOTALS"
