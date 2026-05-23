#!/bin/bash
# Regression for issue #1425: Fastify + ws listeners must not leave the
# process inside GC_UNSAFE_ZONES. The compiled program starts both server
# adapters, calls manual gc() several times, and PERRY_GC_TRACE must show
# manual GC cycles without the old "gc() skipped" unsafe-zone warning.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY:-$REPO_ROOT/target/release/perry}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

run_with_timeout() {
    local secs="$1"
    shift
    if command -v timeout >/dev/null 2>&1; then
        timeout "$secs" "$@"
        return $?
    fi
    if command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$secs" "$@"
        return $?
    fi
    "$@" &
    local pid=$!
    ( sleep "$secs" && kill -TERM "$pid" 2>/dev/null && sleep 1 && kill -KILL "$pid" 2>/dev/null ) &
    local watcher=$!
    if wait "$pid" 2>/dev/null; then
        kill -TERM "$watcher" 2>/dev/null
        wait "$watcher" 2>/dev/null || true
        return 0
    fi
    local rc=$?
    kill -TERM "$watcher" 2>/dev/null
    wait "$watcher" 2>/dev/null || true
    [[ "$rc" == "143" ]] && return 124
    return "$rc"
}

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

SRC="$REPO_ROOT/test-files/test_issue_1425_gc_unsafe_zones.ts"
BIN="$TMPDIR/issue_1425_gc_unsafe_zones"

"$PERRY" compile --no-cache "$SRC" -o "$BIN" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -60
    exit 1
}

fastify_port=$((31000 + RANDOM % 10000))
ws_port=$((41000 + RANDOM % 10000))

set +e
PERRY_GC_TRACE=1 "$BIN" "$fastify_port" "$ws_port" >"$TMPDIR/run.log" 2>&1 &
server_pid=$!
set -e

cleanup_server() {
    if [[ -n "${server_pid:-}" ]] && kill -0 "$server_pid" 2>/dev/null; then
        kill -TERM "$server_pid" 2>/dev/null || true
        for _ in $(seq 1 10); do
            if ! kill -0 "$server_pid" 2>/dev/null; then
                wait "$server_pid" 2>/dev/null || true
                return
            fi
            sleep 0.1
        done
        kill -KILL "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
}
trap 'cleanup_server; rm -rf "$TMPDIR"' EXIT

ready=0
for _ in $(seq 1 100); do
    if grep -q "issue1425:ready" "$TMPDIR/run.log"; then
        ready=1
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

if [[ "$ready" -ne 1 ]]; then
    echo "FAIL: runtime did not report ready servers"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

python3 - "$fastify_port" >"$TMPDIR/http.log" 2>&1 <<'PY' || {
import json
import sys
import urllib.request

port = int(sys.argv[1])
with urllib.request.urlopen(f"http://127.0.0.1:{port}/ping", timeout=5) as response:
    body = json.loads(response.read().decode("utf-8"))
if body != {"ok": True}:
    raise SystemExit(f"unexpected HTTP body: {body!r}")
print("http=ok")
PY
    echo "FAIL: HTTP client probe failed"
    sed 's/^/    /' "$TMPDIR/http.log" | tail -80
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

python3 - "$ws_port" >"$TMPDIR/ws.log" 2>&1 <<'PY' || {
import base64
import os
import socket
import sys

port = int(sys.argv[1])

sock = socket.create_connection(("127.0.0.1", port), timeout=5)
sock.settimeout(5)
key = base64.b64encode(os.urandom(16)).decode("ascii")
request = (
    "GET / HTTP/1.1\r\n"
    f"Host: 127.0.0.1:{port}\r\n"
    "Upgrade: websocket\r\n"
    "Connection: Upgrade\r\n"
    f"Sec-WebSocket-Key: {key}\r\n"
    "Sec-WebSocket-Version: 13\r\n"
    "\r\n"
)
sock.sendall(request.encode("ascii"))
response = b""
while b"\r\n\r\n" not in response:
    response += sock.recv(4096)
if b" 101 " not in response.split(b"\r\n", 1)[0]:
    raise SystemExit(response.decode("latin1", "replace"))

sock.close()
print("ws=ok")
PY
    echo "FAIL: WebSocket client probe failed"
    sed 's/^/    /' "$TMPDIR/ws.log" | tail -80
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

sentinel=0
for _ in $(seq 1 350); do
    if grep -q "issue1425:manual-gc-done" "$TMPDIR/run.log"; then
        sentinel=1
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        break
    fi
    sleep 0.1
done

if [[ "$sentinel" -ne 1 ]]; then
    echo "FAIL: runtime did not reach the manual-GC sentinel"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

cleanup_server
server_pid=""

if grep -q "gc() skipped" "$TMPDIR/run.log"; then
    echo "FAIL: manual gc() was still blocked by a GC unsafe zone"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

manual_cycles="$(python3 - "$TMPDIR/run.log" <<'PY'
import json
import sys

count = 0
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("event") == "gc_cycle" and event.get("trigger", {}).get("kind") == "manual":
            count += 1
print(count)
PY
)"

if [[ "$manual_cycles" -lt 4 ]]; then
    echo "FAIL: expected at least 4 manual GC trace cycles, saw $manual_cycles"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: issue #1425 Fastify/ws manual GC regression ($manual_cycles manual cycles)"
