#!/bin/bash
# Regression: a Fastify instance returned by a factory in ANOTHER module lost
# its native-handle tag, so `app.listen(...)` silently bound nothing.
#
# Bug history:
#   The idiomatic server layout
#       // server.ts
#       export function buildServer(): FastifyInstance { return Fastify(); }
#       // main.ts
#       async function main() {
#         const app = buildServer();
#         await app.listen({ port });   // <- resolved, but never bound
#       }
#   compiled and ran with exit code 0, printed nothing, and served nothing.
#   `listen` lowered to `dynamic_boundary:runtime_api` and hit codegen's
#   unknown-native-method arm, which returns 0.0 — so the await resolved on a
#   no-op and the process exited as if the server had started. No diagnostic
#   at any stage; `perry check` and `--type-check` were both clean.
#
#   Two independent gaps had to line up to produce it:
#     1. `lower_decl/fn_decl.rs` matched the return-type annotation against a
#        hand-rolled allowlist that had drifted behind the parameter paths and
#        the shared `native_instance_from_return_type` table — it knew Redis,
#        Pool and WebSocket but not the Fastify types, so `buildServer` was
#        never recorded in `exported_func_return_native_instances`.
#     2. `js_transform/cross_module_natives.rs` only recognised the consumer
#        shape `Stmt::Let { init: Some(Call) }`, and never traversed
#        `Labeled`/`DoWhile`. Async lowering turns `const app = buildServer()`
#        inside `async function main()` into a hoisted box plus
#        `Expr(LocalSet(0, Call(buildServer)))` nested in the generator state
#        machine (`Try > Labeled > DoWhile > If`), so the scan both failed to
#        reach the statement and failed to match its shape — even though
#        `fix_native_instance_stmt` already walked both statement kinds.
#
#   Fix (1) routes fn_decl through the shared table (+ Fastify entries);
#   fix (2) adds the Labeled/DoWhile arms and a LocalSet-from-call arm.
#
# Because gap 2 is about the *async* shape, this test MUST keep `await
# app.listen(...)` inside an async function — a non-async main passes even with
# the bug present.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "SKIP: curl not available"
  exit 0
fi

PORT=18094
TMPDIR=$(mktemp -d)
cleanup() {
  [ -n "${APP_PID:-}" ] && kill -9 "$APP_PID" 2>/dev/null
  rm -rf "$TMPDIR"
}
trap cleanup EXIT

cat > "$TMPDIR/server.ts" << 'EOF'
import Fastify, { type FastifyInstance } from 'fastify';

export function buildServer(): FastifyInstance {
  const app = Fastify({ logger: false });
  app.get('/health', async () => ({ ok: true }));
  return app;
}
EOF

cat > "$TMPDIR/main.ts" << EOF
import { buildServer } from './server';

async function main() {
  const app = buildServer();
  await app.listen({ port: $PORT });
  console.log('listen-resolved');
}
main().catch((e) => { console.error(e); process.exit(1); });
EOF

cd "$TMPDIR"
if ! "$PERRY" compile main.ts --output test_bin >compile.log 2>&1; then
  echo "FAIL: compile error"
  cat compile.log
  exit 1
fi

./test_bin >run.log 2>&1 &
APP_PID=$!

RUN_OUTPUT=""
for _ in $(seq 1 40); do
  sleep 0.25
  if RUN_OUTPUT=$(curl -sf -m 2 "http://127.0.0.1:$PORT/health" 2>/dev/null); then
    break
  fi
done

if [ "$RUN_OUTPUT" = '{"ok":true}' ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: cross-module factory server did not serve on port $PORT"
if kill -0 "$APP_PID" 2>/dev/null; then
  echo "  (process alive — bound to a different address, or route missing)"
else
  echo "  (process exited — listen() no-opped, the original bug)"
fi
echo "Expected: {\"ok\":true}"
echo "Got: ${RUN_OUTPUT:-<no response>}"
echo "--- program output ---"
cat run.log
exit 1
