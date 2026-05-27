#!/usr/bin/env bash
# node-http-serve — #2041 live-socket smoke for `node:http`.
#
# The other compile+run+diff fixtures drive programs that run and exit; an HTTP
# server stays up, so (like fastify-replay) this fixture has a bespoke driver:
# it compiles entry.ts, launches the binary, curls the listening socket, and
# asserts a 200 "ok". This is the only node:http coverage that asserts a *live*
# socket — the test-files/test_node_http_*.ts fixtures are compile/link-only.
#
# entry.ts uses the exact shape from the issue: a chained
# `createServer(...).listen(port, host, cb)`. Pre-#2041 that compiled+linked
# but exited before binding the port (the chained `.listen` never dispatched,
# so the active-handle pump was never armed; and the host string was mis-routed
# into the callback slot). A regression there makes the curl below fail to
# connect.
set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="node-http-serve"
PORT="${NODE_HTTP_SERVE_PORT:-44599}"

fixture_setup "$NAME" || exit 1

if ! command -v curl >/dev/null 2>&1; then
    fixture_skip "$NAME" "curl not on PATH"
fi

# Compile.
if ! "$PERRY_BIN" compile entry.ts -o out > perry-compile.log 2>&1; then
    echo "FAIL $NAME — compile failed"
    tail -20 perry-compile.log | sed 's/^/    /'
    exit 1
fi

# Launch the server in the background; ensure it's reaped on every exit path.
./out > perry-run.log 2>&1 &
SRV_PID=$!
cleanup() { kill "$SRV_PID" 2>/dev/null; wait "$SRV_PID" 2>/dev/null; }
trap cleanup EXIT

# Poll the socket — the accept loop binds asynchronously after listen() returns.
BODY=""
CODE=""
for _ in $(seq 1 50); do
    if ! kill -0 "$SRV_PID" 2>/dev/null; then
        echo "FAIL $NAME — server exited before binding (the #2041 regression)"
        tail -20 perry-run.log | sed 's/^/    /'
        exit 1
    fi
    BODY="$(curl -fsS --max-time 2 "http://127.0.0.1:${PORT}/" 2>/dev/null)" || { sleep 0.2; continue; }
    CODE="$(curl -fsS -o /dev/null -w '%{http_code}' --max-time 2 "http://127.0.0.1:${PORT}/" 2>/dev/null)" || true
    break
done

if [[ "$BODY" == "ok" && "$CODE" == "200" ]]; then
    echo "PASS $NAME — GET / => 200 \"ok\""
    exit 0
fi

echo "FAIL $NAME — expected 200 \"ok\", got code='${CODE}' body='${BODY}'"
echo "    --- perry-run.log ---"
tail -20 perry-run.log | sed 's/^/    /'
exit 1
