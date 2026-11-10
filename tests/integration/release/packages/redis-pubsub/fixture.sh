#!/usr/bin/env bash
# redis-pubsub: needs a real redis-server binary on PATH. The backend is
# launched on a non-default port to avoid colliding with a system redis,
# and shut down via trap on exit (success OR failure paths).

set -uo pipefail
cd "$(dirname "$0")"
. "$(dirname "$0")/../_fixture_lib.sh"

NAME="redis-pubsub"
PORT=18903

if ! command -v redis-server >/dev/null 2>&1; then
    fixture_skip "$NAME" "redis-server not on PATH"
fi

fixture_setup "$NAME" || exit 1

# Launch redis-server in the background. --save '' disables RDB snapshots
# (we don't want a dump.rdb landing in the fixture dir on shutdown).
redis-server --port "$PORT" --bind 127.0.0.1 --save '' --dir /tmp \
    > redis-server.log 2>&1 &
REDIS_PID=$!

trap '[[ -n "${REDIS_PID:-}" ]] && kill "$REDIS_PID" 2>/dev/null; wait "$REDIS_PID" 2>/dev/null || true' EXIT

# Wait for redis to be ready (max 5s).
deadline=$(( $(date +%s) + 5 ))
while [[ $(date +%s) -lt $deadline ]]; do
    if redis-cli -p "$PORT" ping 2>/dev/null | grep -q PONG; then
        break
    fi
    sleep 0.2
done
if ! redis-cli -p "$PORT" ping 2>/dev/null | grep -q PONG; then
    echo "FAIL $NAME — redis-server didn't come up on port $PORT"
    sed 's/^/    /' redis-server.log | tail -10
    exit 1
fi

export REDIS_URL="redis://127.0.0.1:$PORT"
fixture_compile_run_diff "$NAME"
