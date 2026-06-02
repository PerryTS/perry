#!/usr/bin/env bash
# fastify-replay: tier-3 fixture that delegates to the existing
# scripts/run_fastify_tests.sh. That script spins up a Perry-compiled
# Fastify server, hits it with curl, and asserts response bodies + status
# codes. We don't duplicate that logic here — we just translate its
# pass/fail into the tier-3 fixture contract.
#
# This is a "wrapped" fixture, not a "compile + run + diff" one. It has
# no entry.ts / expected.txt / package.json — the test material lives in
# run_fastify_tests.sh itself. Future similar wraps (e.g. a node:http
# fixture) follow the same shape.

set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="fastify-replay"
REPO_ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
PERRY_BIN="${PERRY_BIN:-$REPO_ROOT/target/release/perry}"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL $NAME — perry not found at $PERRY_BIN"
    exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
    fixture_skip "$NAME" "curl not on PATH (needed by run_fastify_tests.sh)"
fi

# Run the existing harness; capture its summary.
SUMMARY_FILE="$(pwd)/run_fastify_summary.json"
set +e
PERRY_BIN="$PERRY_BIN" \
PERRY_TEST_SUMMARY_OUT="$SUMMARY_FILE" \
    "$REPO_ROOT/scripts/run_fastify_tests.sh" > run_fastify.log 2>&1
rc=$?
set -e

if [[ "$rc" -eq 0 ]]; then
    echo "PASS $NAME"
    exit 0
else
    echo "FAIL $NAME (run_fastify_tests.sh exit=$rc)"
    if [[ -f "$SUMMARY_FILE" ]]; then
        echo "    summary: $(cat "$SUMMARY_FILE")"
    fi
    echo "    --- last 30 lines of run_fastify.log ---"
    tail -30 run_fastify.log | sed 's/^/    /'
    exit 1
fi
