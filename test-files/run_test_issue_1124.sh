#!/bin/bash
# Wire-level regression harness for issue #1124 — server-side Buffer body
# integrity. See test_issue_1124_http_buffer_body.ts for the bug writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1124.sh
#
# Boots the server fixture in the background, curls it, asserts the body
# is the 8-byte PNG magic (the test's intended bytes, NOT all zeros which
# was the pre-fix wire output). Always tears the server down, even on
# failure paths.

set -euo pipefail

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_SRC="$SCRIPT_DIR/test_issue_1124_http_buffer_body.ts"
PORT=18993
EXE="${TMPDIR:-/tmp}/test_issue_1124_http_buffer_body"
EXPECTED="89504e470d0a1a0a"

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

echo "[1124] compiling fixture..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TEST_SRC" -o "$EXE" >/dev/null 2>&1

echo "[1124] starting server on :$PORT..."
"$EXE" >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null || true' EXIT

# Wait for bind. The server prints LISTENING on the user callback and
# auto-closes 750ms later (so the parity runner can match its expected
# stdout). We need to land curl inside that 750ms window — probe a few
# times in quick succession.
for i in 1 2 3 4 5; do
    if curl -sS -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

ACTUAL="$(curl -sS --max-time 2 "http://127.0.0.1:$PORT/" | xxd -p | tr -d '\n')"
echo "[1124] received: $ACTUAL"
echo "[1124] expected: $EXPECTED"

if [[ "$ACTUAL" == "$EXPECTED" ]]; then
    echo "[1124] PASS"
    exit 0
else
    echo "[1124] FAIL — body mismatch"
    exit 1
fi
