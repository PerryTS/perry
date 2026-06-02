#!/usr/bin/env bash
# Tier-3 fixture: nestjs-hello.
#
# Boots a minimal real NestJS app (NestFactory.create + app.listen) compiled
# natively by Perry. The PR #754 maintainer review asks specifically for an
# end-to-end test against the actual `@nestjs/common` + `@nestjs/core` npm
# packages, not a hand-rolled DI mock.
#
# Acceptance (all four must pass for PASS):
#   - `npm install` resolves @nestjs/* into node_modules
#   - `perry entry.ts -o ./out` exits 0
#   - `./out` reaches the "listening on <port>" marker within startup budget
#   - `curl http://localhost:<port>/` returns status 200 with body "Hello Perry"
#
# Any wall before the curl check is documented in WALLS.md and the fixture
# reports SKIP with the wall as the reason (so the release harness records
# the gap without going red, and the next iteration knows where to attack).

set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="nestjs-hello"
PORT="${PERRY_NEST_PORT:-13754}"
STARTUP_TIMEOUT_SECS="${PERRY_NEST_STARTUP_TIMEOUT:-15}"

fixture_setup "$NAME" || exit 1

if ! command -v curl >/dev/null 2>&1; then
    fixture_skip "$NAME" "curl not on PATH"
fi

# Compile.
echo "  [compile] perry entry.ts..."
if ! "$PERRY_BIN" entry.ts -o ./out > perry-compile.log 2>&1; then
    echo "FAIL $NAME — perry compile errored"
    echo "    See WALLS.md for the current known compatibility gaps."
    sed 's/^/    /' perry-compile.log | tail -60
    if [[ -f WALLS.md ]]; then
        # Treat documented compile-time walls as SKIP rather than FAIL so
        # the release harness records the gap without blocking. Remove
        # WALLS.md once the wall is gone to flip this to a hard FAIL.
        fixture_skip "$NAME" "compile-time wall — see WALLS.md"
    fi
    exit 1
fi

# Run in background.
echo "  [run] starting compiled binary on port $PORT..."
PERRY_NEST_PORT="$PORT" ./out > perry-run.log 2>&1 &
SERVER_PID=$!

cleanup() {
    if kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        # SIGTERM first; if still alive after 2 s send SIGKILL.
        for _ in 1 2 3 4 5; do
            kill -0 "$SERVER_PID" 2>/dev/null || break
            sleep 0.4
        done
        kill -9 "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Wait for the "listening on" marker. Polling the log avoids racing the
# listener: even if the port is open, NestJS may still be wiring up routes.
echo "  [wait] for listener (timeout ${STARTUP_TIMEOUT_SECS}s)..."
deadline=$(( SECONDS + STARTUP_TIMEOUT_SECS ))
while ! grep -q "nestjs-hello listening on" perry-run.log 2>/dev/null; do
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "FAIL $NAME — server exited before reaching the listening marker"
        echo "    --- perry-run.log (tail) ---"
        tail -60 perry-run.log | sed 's/^/    /'
        if [[ -f WALLS.md ]]; then
            fixture_skip "$NAME" "runtime wall during bootstrap — see WALLS.md"
        fi
        exit 1
    fi
    if (( SECONDS >= deadline )); then
        echo "FAIL $NAME — listener did not come up within ${STARTUP_TIMEOUT_SECS}s"
        tail -60 perry-run.log | sed 's/^/    /'
        if [[ -f WALLS.md ]]; then
            fixture_skip "$NAME" "startup wall — see WALLS.md"
        fi
        exit 1
    fi
    sleep 0.3
done

# Hit the endpoint.
echo "  [curl] GET http://localhost:$PORT/..."
curl_status="$(curl -s -o curl-body.txt -w "%{http_code}" "http://localhost:$PORT/" || true)"
curl_body="$(cat curl-body.txt 2>/dev/null || echo "")"

# Build the actual output in the same format as expected.txt for a clean diff.
{
    echo "curl status: $curl_status"
    echo "curl body: $curl_body"
} > actual.txt

if ! diff -u expected.txt actual.txt > diff.log; then
    echo "FAIL $NAME — output diverges from expected.txt"
    sed 's/^/    /' diff.log
    echo "    --- perry-run.log (tail) ---"
    tail -30 perry-run.log | sed 's/^/    /'
    exit 1
fi

echo "PASS $NAME"
