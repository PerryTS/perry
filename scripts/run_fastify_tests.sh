#!/usr/bin/env bash
# Fastify end-to-end integration test (issue #174).
#
# The docs/src/stdlib/http.md Fastify example is marked no-test because
# `app.listen()` enters a blocking event loop that never exits. This
# script tests the full HTTP path anyway by running the Perry-compiled
# server as a background process, issuing real curl requests, checking
# the JSON responses, then killing the process. Covers routing, path
# params, JSON body parsing, and reply.code().
#
# Usage:
#   scripts/run_fastify_tests.sh                   # uses ./target/release/perry
#   PERRY_BIN=/path/to/perry scripts/run_fastify_tests.sh

set -uo pipefail

PERRY_BIN="${PERRY_BIN:-$(pwd)/target/release/perry}"
if [[ ! -x "$PERRY_BIN" ]]; then
    echo "perry binary not found at $PERRY_BIN; set PERRY_BIN or run 'cargo build --release -p perry'"
    exit 2
fi

TEST_SRC="$(cd "$(dirname "$0")/.." && pwd)/test-files/test_fastify_integration.ts"
if [[ ! -f "$TEST_SRC" ]]; then
    echo "test source not found at $TEST_SRC"
    exit 2
fi

# Pick a high random port to avoid colliding with whatever's on 3000/8080.
# Retry a couple of times if the port happens to be busy when we bind.
TMP_DIR="$(mktemp -d)"
SERVER_PID=""
cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
    fi
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

BIN="$TMP_DIR/fastify_bin"
# --no-cache: object cache keys on source hash + CompileOptions, not on
# codegen dispatch tables — so a stale `.perry-cache` from an earlier
# perry build can silently mask Fastify dispatch fixes. Always re-codegen.
if ! "$PERRY_BIN" compile --no-cache "$TEST_SRC" -o "$BIN" >"$TMP_DIR/compile.log" 2>&1; then
    echo "FAIL: compile of $TEST_SRC failed:"
    sed 's/^/    /' "$TMP_DIR/compile.log"
    exit 2
fi

# Up to 3 port attempts: if the server fails to bind, try again.
start_server() {
    local port="$1"
    LOG="$TMP_DIR/server.log"
    : > "$LOG"
    "$BIN" "$port" > "$LOG" 2>&1 &
    SERVER_PID=$!
    # Wait up to 5s for "ready port=<N>" sentinel.
    for _ in $(seq 50); do
        if ! kill -0 "$SERVER_PID" 2>/dev/null; then
            return 1  # server died
        fi
        if grep -q "ready port=$port" "$LOG" 2>/dev/null; then
            # Give the server loop one more tick to enter accept().
            sleep 0.05
            return 0
        fi
        sleep 0.1
    done
    return 1
}

PORT=0
for attempt in 1 2 3; do
    candidate=$((30000 + RANDOM % 10000))
    if start_server "$candidate"; then
        PORT=$candidate
        break
    fi
    # Clean up the dead process before the next attempt.
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null
        wait "$SERVER_PID" 2>/dev/null
        SERVER_PID=""
    fi
done

if [[ "$PORT" -eq 0 ]]; then
    echo "FAIL: server did not start after 3 attempts"
    [[ -f "$TMP_DIR/server.log" ]] && sed 's/^/    /' "$TMP_DIR/server.log"
    exit 2
fi

pass=0
fail=0

check() {
    local name="$1"
    local expected="$2"
    local actual="$3"
    if [[ "$expected" == "$actual" ]]; then
        echo "PASS $name"
        pass=$((pass+1))
    else
        echo "FAIL $name"
        echo "  expected: $expected"
        echo "  actual:   $actual"
        fail=$((fail+1))
    fi
}

# Test 1: simple GET returning a JSON object.
actual="$(curl -s --max-time 5 "http://127.0.0.1:$PORT/hello" || true)"
check "GET /hello" '{"hello":"world"}' "$actual"

# Test 2: path parameter extraction (:id).
actual="$(curl -s --max-time 5 "http://127.0.0.1:$PORT/users/42" || true)"
check "GET /users/:id" '{"id":"42","name":"User 42"}' "$actual"

# Test 3: POST with JSON body, reply.code(201) should set status, body echoed.
code_and_body="$(curl -s --max-time 5 -o "$TMP_DIR/post_body" -w '%{http_code}' \
    -X POST "http://127.0.0.1:$PORT/echo" \
    -H "content-type: application/json" \
    -d '{"key":"value"}' || true)"
post_body="$(cat "$TMP_DIR/post_body" 2>/dev/null || true)"
check "POST /echo status"  "201"                           "$code_and_body"
check "POST /echo body"    '{"received":{"key":"value"}}'  "$post_body"

# Test 4: unknown route returns 404.
code="$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' \
    "http://127.0.0.1:$PORT/does-not-exist" || true)"
check "GET /does-not-exist -> 404" "404" "$code"

echo
echo "fastify-tests: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
