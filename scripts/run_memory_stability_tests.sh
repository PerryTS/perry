#!/usr/bin/env bash
# Memory-stability regression suite.
#
# Two failure modes this catches that microbenchmarks miss:
#   1. Slow RSS accumulation in long-running programs (a real "2 GB
#      after an hour" leak that a 300 ms bench wouldn't surface).
#   2. Crashes when GC fires aggressively during sensitive ops
#      (parse, recursion, closure init, write barriers).
#
# How it works:
#   - test_memory_*.ts run a sustained allocate-and-discard loop
#     for 100k-200k iterations. RSS must stay under a per-test limit
#     (set ~50% above the current baseline). If a future change
#     pins blocks, leaks the parse-key cache, or breaks tenuring,
#     RSS climbs and the test fails.
#   - test_gc_*.ts force aggressive GC scheduling during sensitive
#     operations. Test passes ⟺ exit code 0 + correct stdout.
#
# Each test runs under THREE GC mode combos:
#   - default (now generational GC as of Phase D, v0.5.237)
#   - mark-sweep (PERRY_GEN_GC=0 — bisection escape hatch)
#   - PERRY_GEN_GC=1 PERRY_WRITE_BARRIERS=1
# so a regression in any mode is caught.
#
# Usage:  scripts/run_memory_stability_tests.sh
# Exit:   0 on all pass, 1 on any failure.

set -euo pipefail

cd "$(dirname "$0")/.."

cargo build --release -p perry-runtime -p perry-stdlib -p perry --quiet

PERRY=./target/release/perry
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# Globals set by run_one. Bash makes it painful to return multiple
# values cleanly; globals beat parsing a single-line string.
LAST_RSS_MB=0
LAST_EXIT=0
LAST_STDOUT_FILE=""

# Run a compiled binary under /usr/bin/time. Cross-platform RSS read
# (macOS reports bytes, Linux reports KB).
run_one() {
    local bin="$1"
    shift  # remaining args are env VAR=val pairs

    LAST_STDOUT_FILE="$TMPDIR/stdout.$$.$RANDOM"
    local stderr_file="$TMPDIR/stderr.$$.$RANDOM"
    LAST_EXIT=0

    if [[ "$(uname)" == "Darwin" ]]; then
        env "$@" /usr/bin/time -l "$bin" >"$LAST_STDOUT_FILE" 2>"$stderr_file" \
            || LAST_EXIT=$?
        local b
        b=$(awk '/maximum resident set size/ {print $1}' "$stderr_file")
        LAST_RSS_MB=$((b / 1024 / 1024))
    else
        env "$@" /usr/bin/time -v "$bin" >"$LAST_STDOUT_FILE" 2>"$stderr_file" \
            || LAST_EXIT=$?
        local kb
        kb=$(awk '/Maximum resident set size/ {print $NF}' "$stderr_file")
        LAST_RSS_MB=$((kb / 1024))
    fi
}

# Compile once per .ts file; run multiple modes against the same binary.
PASS=0
FAIL=0

run_test() {
    local ts="$1"
    local rss_limit_mb="$2"
    local expect_substr="$3"

    local bin="$TMPDIR/$(basename "${ts%.ts}")"
    if ! $PERRY compile --no-cache "$ts" -o "$bin" >/dev/null 2>&1; then
        printf "  FAIL [%-12s] %-40s compile failed\n" "compile" "$(basename "$ts")"
        FAIL=$((FAIL + 1))
        return
    fi

    local mode_specs=(
        "default|"
        "mark-sweep|PERRY_GEN_GC=0"
        "gen-gc+wb|PERRY_GEN_GC=1 PERRY_WRITE_BARRIERS=1"
    )

    for spec in "${mode_specs[@]}"; do
        local mode_label="${spec%%|*}"
        local env_str="${spec#*|}"

        # Split env_str on spaces into argv tokens (an empty string
        # gives env zero args, which is fine).
        local env_args=()
        if [[ -n "$env_str" ]]; then
            # shellcheck disable=SC2206
            env_args=($env_str)
        fi

        # `"${env_args[@]+"${env_args[@]}"}"` is the safe-expand
        # idiom under `set -u`: empty array → no args, non-empty →
        # quoted expansion.
        run_one "$bin" "${env_args[@]+"${env_args[@]}"}"

        local status="PASS"
        local reason=""

        if [[ "$LAST_EXIT" -ne 0 ]]; then
            status="FAIL"
            reason="exit=$LAST_EXIT"
        elif [[ "$LAST_RSS_MB" -gt "$rss_limit_mb" ]]; then
            status="FAIL"
            reason="rss=${LAST_RSS_MB}MB > limit=${rss_limit_mb}MB"
        elif [[ -n "$expect_substr" ]] && ! grep -qF "$expect_substr" "$LAST_STDOUT_FILE"; then
            status="FAIL"
            reason="stdout missing: $expect_substr"
        fi

        printf "  %s [%-12s] %-40s rss=%3dMB / limit=%3dMB %s\n" \
            "$status" "$mode_label" "$(basename "$ts")" \
            "$LAST_RSS_MB" "$rss_limit_mb" "$reason"

        if [[ "$status" == "PASS" ]]; then
            PASS=$((PASS + 1))
        else
            FAIL=$((FAIL + 1))
        fi
    done
}

echo "=== Memory-leak regression tests (RSS plateau under sustained alloc) ==="
# Limits ~50-70% above measured baseline on macOS arm64. CI runners
# may differ slightly; loosen a limit here rather than in the .ts.
run_test test-files/test_memory_long_lived_loop.ts 100 "done, lastId=199999"
run_test test-files/test_memory_json_churn.ts      250 "done, checksum=637747500"
run_test test-files/test_memory_string_churn.ts    100 "done, total=9577780"
run_test test-files/test_memory_closure_churn.ts    50 "done, sum=15004649874"

echo ""
echo "=== GC-aggression regression tests (no crash + correct result) ==="
run_test test-files/test_gc_aggressive_forced.ts    50 "done, acc=8022890"
run_test test-files/test_gc_deep_recursion.ts       30 "done, result=320400"

echo ""
echo "=== Summary ==="
echo "  PASS: $PASS"
echo "  FAIL: $FAIL"

# release_sweep.sh hook — see comment in run_parity_tests.sh.
if [[ -n "${PERRY_TEST_SUMMARY_OUT:-}" ]]; then
    cat > "$PERRY_TEST_SUMMARY_OUT" <<EOF
{"script": "run_memory_stability_tests.sh", "passed": $PASS, "failed": $FAIL, "skipped": 0}
EOF
fi

if [[ $FAIL -ne 0 ]]; then
    exit 1
fi
