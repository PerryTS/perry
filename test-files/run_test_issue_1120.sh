#!/bin/bash
# Wire-level regression harness for issue #1120 — fastify Buffer /
# Uint8Array response body integrity + auto-HEAD-for-GET.
# See test_issue_1120_fastify_buffer.ts for the bug writeup.
#
# Usage:
#   PERRY_BIN=./target/release/perry ./test-files/run_test_issue_1120.sh
#
# Three assertions:
#   1. GET /buf  →  body is 8-byte PNG magic + content-type is
#      `application/octet-stream` (handler set via `reply.type(...)`).
#      Pre-fix this came back as Buffer.toJSON + `application/json`.
#   2. GET /u8   →  body is `01 02 03 04 05` + content-type defaults
#      to `application/octet-stream` (Uint8Array, no reply.type).
#      Pre-fix: `{"0":1,...}` + `application/json`.
#   3. HEAD /buf →  status 200, empty body, Content-Length: 8.
#      Pre-fix: 404 (HEAD not matched against registered GET).
#
# Always tears the server down, even on failure paths.

set -euo pipefail

PERRY_BIN="${PERRY_BIN:-./target/release/perry}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TEST_SRC="$SCRIPT_DIR/test_issue_1120_fastify_buffer.ts"
PORT=18996
EXE="${TMPDIR:-/tmp}/test_issue_1120_fastify_buffer"
EXPECTED_BUF_BODY="89504e470d0a1a0a"
EXPECTED_U8_BODY="0102030405"
EXPECTED_CT="application/octet-stream"
EXPECTED_HEAD_CL="8"

cd "$WORKSPACE_ROOT"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "FAIL: perry binary not found at $PERRY_BIN — build first via cargo build --release -p perry" >&2
    exit 1
fi

echo "[1120] compiling fixture..."
PERRY_ALLOW_PERRY_FEATURES=1 "$PERRY_BIN" "$TEST_SRC" -o "$EXE" >/dev/null 2>&1

echo "[1120] starting server on :$PORT..."
"$EXE" >/dev/null 2>&1 &
SERVER_PID=$!
trap 'kill $SERVER_PID 2>/dev/null; wait $SERVER_PID 2>/dev/null || true' EXIT

# Wait for bind.
for i in 1 2 3 4 5 6 7 8; do
    if curl -sS -o /dev/null --max-time 1 "http://127.0.0.1:$PORT/buf" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

fail=0

# Assertion 1: GET /buf
TMP_OUT="$(mktemp)"
ACTUAL_BUF_CT="$(curl -sS --max-time 2 -o "$TMP_OUT" -w "%{content_type}" "http://127.0.0.1:$PORT/buf" | tr -d '\r\n')"
ACTUAL_BUF_BODY="$(xxd -p "$TMP_OUT" | tr -d '\n')"
rm -f "$TMP_OUT"
echo "[1120] GET /buf body=$ACTUAL_BUF_BODY ct=$ACTUAL_BUF_CT"
if [[ "$ACTUAL_BUF_BODY" != "$EXPECTED_BUF_BODY" ]]; then
    echo "[1120] FAIL — Buffer body mismatch (pre-fix: Buffer.toJSON form)"
    fail=1
fi
if [[ "$ACTUAL_BUF_CT" != "$EXPECTED_CT"* ]]; then
    echo "[1120] FAIL — Buffer content-type mismatch (pre-fix: application/json overwrite)"
    fail=1
fi

# Assertion 2: GET /u8
TMP_OUT="$(mktemp)"
ACTUAL_U8_CT="$(curl -sS --max-time 2 -o "$TMP_OUT" -w "%{content_type}" "http://127.0.0.1:$PORT/u8" | tr -d '\r\n')"
ACTUAL_U8_BODY="$(xxd -p "$TMP_OUT" | tr -d '\n')"
rm -f "$TMP_OUT"
echo "[1120] GET /u8  body=$ACTUAL_U8_BODY  ct=$ACTUAL_U8_CT"
if [[ "$ACTUAL_U8_BODY" != "$EXPECTED_U8_BODY" ]]; then
    echo "[1120] FAIL — Uint8Array body mismatch (pre-fix: {\"0\":1,...} object form)"
    fail=1
fi
if [[ "$ACTUAL_U8_CT" != "$EXPECTED_CT"* ]]; then
    echo "[1120] FAIL — Uint8Array content-type default mismatch"
    fail=1
fi

# Assertion 3: HEAD /buf (auto-HEAD-for-GET)
HEAD_RESULT="$(curl -sS --max-time 2 -I "http://127.0.0.1:$PORT/buf" || true)"
HEAD_STATUS="$(printf '%s\n' "$HEAD_RESULT" | head -n 1 | awk '{print $2}')"
HEAD_CL="$(printf '%s\n' "$HEAD_RESULT" \
    | awk -F': ' 'tolower($1)=="content-length"{print $2}' \
    | tr -d '\r\n' | head -c 200)"
echo "[1120] HEAD /buf status=$HEAD_STATUS content-length=$HEAD_CL"
if [[ "$HEAD_STATUS" != "200" ]]; then
    echo "[1120] FAIL — HEAD on registered GET returned $HEAD_STATUS (pre-fix: 404)"
    fail=1
fi
if [[ "$HEAD_CL" != "$EXPECTED_HEAD_CL" ]]; then
    echo "[1120] FAIL — HEAD Content-Length=$HEAD_CL, expected $EXPECTED_HEAD_CL"
    fail=1
fi

if [[ $fail -eq 0 ]]; then
    echo "[1120] PASS"
    exit 0
else
    exit 1
fi
