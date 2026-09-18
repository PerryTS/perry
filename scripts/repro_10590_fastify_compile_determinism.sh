#!/usr/bin/env bash
# Reproduction harness for #10590: `perry.compilePackages: ["fastify"]`
# compiled twice from identical source can produce two binaries that behave
# differently on `GET /` (build-to-build nondeterminism in package
# compilation, not a runtime flake).
#
# Usage:
#   PERRY_BIN=/path/to/perry PERRY_RUNTIME_DIR=/path/to/runtime-dir \
#     ./scripts/repro_10590_fastify_compile_determinism.sh [N]
#
# Compiles the same fastify app N times (default 3) into ./repro-10590-work/runN,
# hashes each binary, and does a GET / smoke check against each one. Prints a
# verdict: identical hashes across all N builds, or which ones diverge.
#
# Requires: a `perry` binary built with `-p perry -p perry-runtime-static
# -p perry-stdlib-static` (see CLAUDE.md Build Commands), `npm`, `curl`.
set -euo pipefail

N="${1:-3}"
PORT="${REPRO_10590_PORT:-8073}"
WORK="$(pwd)/repro-10590-work"

: "${PERRY_BIN:?set PERRY_BIN to a built perry binary}"
: "${PERRY_RUNTIME_DIR:?set PERRY_RUNTIME_DIR to the dir holding libperry_runtime.a / libperry_stdlib.a}"

rm -rf "$WORK"
mkdir -p "$WORK"
cd "$WORK"

cat > package.json <<JSON
{
  "name": "repro-10590",
  "version": "1.0.0",
  "type": "module",
  "dependencies": { "fastify": "^5.0.0" },
  "perry": {
    "compilePackages": ["*", "fastify"],
    "allow": { "compilePackages": ["*"] }
  }
}
JSON

cat > main.ts <<TS
import Fastify from "fastify";

const app = Fastify({ logger: false });

app.get("/", async (request, reply) => {
  return { hello: "world" };
});

app.listen({ port: ${PORT}, host: "0.0.0.0" }, (err) => {
  if (err) {
    console.error(err);
    process.exit(1);
  }
  console.log("listening");
});
TS

npm install --no-audit --no-fund --silent

for i in $(seq 1 "$N"); do
  rm -rf .perry-cache
  echo "== compile $i/$N =="
  "$PERRY_BIN" main.ts -o "run$i" > "build$i.log" 2>&1 || {
    echo "compile $i FAILED, see $WORK/build$i.log"
    exit 1
  }
done

echo
echo "== sha256 of each binary =="
sha256sum run* | tee hashes.txt

UNIQUE=$(awk '{print $1}' hashes.txt | sort -u | wc -l)
echo
if [ "$UNIQUE" -eq 1 ]; then
  echo "VERDICT: all $N binaries are byte-identical."
else
  echo "VERDICT: binaries DIFFER across builds ($UNIQUE distinct hash(es) across $N builds)."
fi

echo
echo "== GET / smoke check per binary =="
for i in $(seq 1 "$N"); do
  ./"run$i" > "server$i.log" 2>&1 &
  PID=$!
  sleep 1
  RESP=$(curl -sS -m 3 "http://127.0.0.1:${PORT}/" 2>&1 || true)
  kill -9 "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
  echo "run$i: $RESP"
done
