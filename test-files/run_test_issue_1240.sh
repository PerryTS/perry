#!/bin/bash
# Wire-level regression harness for issue #1240 — fastify request.json()
# returns parsed body instead of undefined.
# See test_issue_1240_fastify_request_json.ts for the bug writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1240.sh
#
# Assertion: POST /json-test with `{"hello":"world"}` returns a JSON body
# containing `"hasHelloKey":true` and `"hello":"world"`. Pre-fix the
# response was `{"parsedIsUndefined":true,...}` (and the body was actually
# the literal string `"undefined"` because the c.json shim was running by
# mistake — the test server here would still 200 because the handler
# returns its diagnostic object instead of routing through reply.send).

set -euo pipefail

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_SRC="$SCRIPT_DIR/test_issue_1240_fastify_request_json.ts"
PORT=18997
EXE="${TMPDIR:-/tmp}/test_issue_1240_fastify_request_json"

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

echo "[1240] compiling fixture..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TEST_SRC" -o "$EXE" >/dev/null 2>&1

echo "[1240] starting server on :$PORT..."
"$EXE" >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null || true' EXIT

# Wait for bind.
for i in 1 2 3 4 5 6 7 8; do
    if curl -sS -o /dev/null --max-time 1 -X POST "http://127.0.0.1:$PORT/json-test" \
        -H 'Content-Type: application/json' -d '{"_":0}' 2>/dev/null; then
        break
    fi
    sleep 0.1
done

fail=0
RESPONSE="$(curl -sS --max-time 2 -X POST "http://127.0.0.1:$PORT/json-test" \
    -H 'Content-Type: application/json' -d '{"hello":"world"}')"

echo "[1240] response=$RESPONSE"

if [[ "$RESPONSE" != *'"hasHelloKey":true'* ]]; then
    echo "[1240] FAIL — request.json() did not parse body (pre-fix: undefined)"
    fail=1
fi
if [[ "$RESPONSE" != *'"hello":"world"'* ]]; then
    echo "[1240] FAIL — parsed body lost the 'hello' key"
    fail=1
fi
if [[ "$RESPONSE" == *'"parsedIsUndefined":true'* ]]; then
    echo "[1240] FAIL — request.json() returned undefined (regression of #1240)"
    fail=1
fi

if [[ $fail -eq 0 ]]; then
    echo "[1240] PASS"
    exit 0
else
    exit 1
fi
