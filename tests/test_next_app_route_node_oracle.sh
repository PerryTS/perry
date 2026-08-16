#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$REPO_ROOT/tests/fixtures/next-app-route"
PORT="${PERRY_NEXT_ORACLE_PORT:-3100}"
WORK="$(mktemp -d)"
SERVER_PID=""

cleanup() {
    if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

node - "$FIXTURE/package-lock.json" <<'NODE'
const lock = require(process.argv[2]);
const expected = {
  "node_modules/next": "16.3.0",
  "node_modules/react": "19.2.4",
  "node_modules/react-dom": "19.2.4",
};
for (const [entry, version] of Object.entries(expected)) {
  const actual = lock.packages?.[entry]?.version;
  if (actual !== version) {
    throw new Error(`${entry}: lockfile has ${actual}, expected ${version}`);
  }
}
console.log("PASS: pinned Next 16.3.0 / React 19.2.4 lockfile");
NODE

(
    cd "$FIXTURE"
    npm ci
    npm run build
)

ROUTE_BUNDLE="$FIXTURE/.next/server/app/api/benchmark/route.js"
node - "$ROUTE_BUNDLE" <<'NODE'
const routePath = require("node:path").resolve(process.argv[2]);
const generated = require(routePath);
if (typeof generated.routeModule?.handle !== "function") {
  throw new Error("generated production bundle lacks routeModule.handle");
}
if (generated.routeModule?.definition?.pathname !== "/api/benchmark") {
  throw new Error(
    `unexpected generated pathname: ${generated.routeModule?.definition?.pathname}`,
  );
}
if (typeof generated.handler !== "function") {
  throw new Error("generated production bundle lacks its server handler");
}
console.log("PASS: production bundle exports routeModule.handle for /api/benchmark");
NODE

(
    cd "$FIXTURE"
    PORT="$PORT" HOSTNAME=127.0.0.1 npm start >"$WORK/node.log" 2>&1
) &
SERVER_PID=$!

ready=false
for _ in $(seq 1 120); do
    if curl --fail --silent --output /dev/null "http://127.0.0.1:$PORT/api/benchmark?id=ready&iterations=1"; then
        ready=true
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        echo "FAIL: Next oracle server exited before readiness" >&2
        sed -n '1,240p' "$WORK/node.log" >&2
        exit 1
    fi
    sleep 0.25
done
if [[ "$ready" != "true" ]]; then
    echo "FAIL: Next oracle server did not become ready" >&2
    sed -n '1,240p' "$WORK/node.log" >&2
    exit 1
fi

(
    cd "$FIXTURE"
    BASE_URL="http://127.0.0.1:$PORT" npm run verify
)

echo "PASS: pinned production Next App Route Node oracle"
