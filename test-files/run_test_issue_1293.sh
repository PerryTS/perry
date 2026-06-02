#!/bin/bash
# Wire-level regression harness for issue #1293 — fastify
# `(request as any).json()` / `(request as any).body` returned NaN /
# undefined (silent 400) under the well-known-flipped perry-ext-fastify
# backend. See test_issue_1293_fastify_request_json_as_any.ts for the writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1293.sh
#
# Assertion: POST `{"hello":"world"}` to /typed, /any-json and /any-body each
# returns a JSON body containing `"hello":"world"` and `"typeofBody":"object"`.
# Pre-fix /any-json returned `"body":"falsy"` (json() was a bare NaN) and
# /any-body returned `"body":"falsy"` (.body was undefined).

set -euo pipefail

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_SRC="$SCRIPT_DIR/test_issue_1293_fastify_request_json_as_any.ts"
PORT=18993
EXE="${TMPDIR:-/tmp}/test_issue_1293_fastify_request_json_as_any"

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

echo "[1293] compiling fixture..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TEST_SRC" -o "$EXE" >/dev/null 2>&1

echo "[1293] starting server on :$PORT..."
"$EXE" >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null || true' EXIT

# Wait for bind.
for i in 1 2 3 4 5 6 7 8; do
    if curl -sS -o /dev/null --max-time 1 -X POST "http://127.0.0.1:$PORT/typed" \
        -H 'Content-Type: application/json' -d '{"_":0}' 2>/dev/null; then
        break
    fi
    sleep 0.1
done

fail=0
for ROUTE in typed any-json any-body; do
    RESPONSE="$(curl -sS --max-time 2 -X POST "http://127.0.0.1:$PORT/$ROUTE" \
        -H 'Content-Type: application/json' -d '{"hello":"world"}')"
    echo "[1293] $ROUTE response=$RESPONSE"

    if [[ "$RESPONSE" == *'"body":"falsy"'* ]]; then
        echo "[1293] FAIL ($ROUTE) — handler took the if(!body) 400 path (body lost)"
        fail=1
    fi
    if [[ "$RESPONSE" != *'"hello":"world"'* ]]; then
        echo "[1293] FAIL ($ROUTE) — parsed body lost the 'hello' key"
        fail=1
    fi
    if [[ "$RESPONSE" != *'"typeofBody":"object"'* ]]; then
        echo "[1293] FAIL ($ROUTE) — body was not an object (pre-fix: number/undefined)"
        fail=1
    fi
done

if [[ $fail -eq 0 ]]; then
    echo "[1293] PASS"
    exit 0
else
    exit 1
fi
