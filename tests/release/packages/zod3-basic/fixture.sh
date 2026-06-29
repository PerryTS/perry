#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="zod3-basic"

now_ms() {
    python3 -c 'import time; print(time.perf_counter_ns() // 1000000)'
}

fixture_setup "$NAME" || exit 1

node entry.ts > node-out.txt 2> node-run.log || {
    echo "FAIL $NAME - node reference errored"
    sed 's/^/    /' node-run.log | tail -40
    exit 1
}
if ! diff -u expected.txt node-out.txt > node-diff.log; then
    echo "FAIL $NAME - committed expected.txt differs from node output"
    sed 's/^/    /' node-diff.log
    exit 1
fi

start_ms="$(now_ms)"
echo "  [perry compile] entry.ts"
if ! "$PERRY_BIN" compile entry.ts -o ./out > perry-compile.log 2>&1; then
    echo "FAIL $NAME - perry compile errored"
    sed 's/^/    /' perry-compile.log | tail -80
    exit 1
fi
compile_done_ms="$(now_ms)"

echo "  [./out] (timeout=${PERRY_FIXTURE_TIMEOUT_SECS}s)"
set +e
_fixture_run_with_timeout "$PERRY_FIXTURE_TIMEOUT_SECS" ./out > perry-out.txt 2> perry-run.log
rc=$?
set -e
run_done_ms="$(now_ms)"

cat > zod3-perf.txt <<PERF
compile_ms=$((compile_done_ms - start_ms))
run_ms=$((run_done_ms - compile_done_ms))
total_ms=$((run_done_ms - start_ms))
PERF

if [[ "$rc" -eq 124 ]]; then
    echo "FAIL $NAME - runtime did not exit within ${PERRY_FIXTURE_TIMEOUT_SECS}s"
    sed 's/^/    /' perry-run.log | tail -20
    exit 1
fi
if [[ "$rc" -ne 0 ]]; then
    echo "FAIL $NAME - runtime exit non-zero (rc=$rc)"
    sed 's/^/    /' perry-run.log | tail -80
    echo "    --- stdout (truncated) ---"
    sed 's/^/    /' perry-out.txt | tail -20
    exit 1
fi

if ! diff -u expected.txt perry-out.txt > diff.log; then
    echo "FAIL $NAME - output diverges"
    sed 's/^/    /' diff.log
    exit 1
fi

echo "  [perf] $(tr '\n' ' ' < zod3-perf.txt)"
echo "PASS $NAME"
