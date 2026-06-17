#!/bin/bash
# Regression: subclassing the native global `Request` (or `Response`) must
# produce instances that still expose the body-reading methods
# (`text`/`json`/`arrayBuffer`/`blob`/`formData`) and the inherited property
# getters (`url`/`method`/...).
#
# Bug: a Web-Fetch Request in Perry is a native registry handle (a small id
# NaN-boxed as a pointer), not a heap JS object — its methods are resolved by
# native handle dispatch, not via a JS prototype chain. `class X extends
# Request {}` produced a plain JS object with NO link to a native handle, so
# `sub.text` was `undefined` and `sub instanceof Request` was `false`. This
# broke `@hono/node-server`, which does `class Request extends GlobalRequest`,
# making every `c.req.text()`/`c.req.json()` throw on POST/PUT routes.
#
# Fix: `super(...)` on Request/Response allocates the underlying native handle
# and stashes its id on `this` under `__perry_fetch_handle__`. Inherited body
# methods, property getters, and `instanceof` are forwarded to that handle at
# runtime.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cat > "$TMPDIR/main.ts" << 'EOF'
// No-own-constructor subclass (default ctor forwards args to super()).
const Sub = class extends Request {};
// Explicit-constructor subclass calling super() — mirrors @hono/node-server.
class Req2 extends Request {
  tag: string;
  constructor(input: string, init?: RequestInit) {
    super(input, init);
    this.tag = "req2";
  }
}

async function main() {
  const sub = new Sub("http://x/y", { method: "POST", body: "hello" });
  console.log("sub.text type:", typeof sub.text);
  console.log("sub instanceof Request:", sub instanceof Request);
  console.log("sub.text body:", await sub.text());

  const r2 = new Req2("http://x/z", { method: "POST", body: '{"n":42}' });
  console.log("r2.tag:", r2.tag);
  console.log("r2 instanceof Request:", r2 instanceof Request);
  console.log("r2.method:", r2.method);
  const j = await r2.json();
  console.log("r2.json.n:", j.n);
}
main();
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="sub.text type: function
sub instanceof Request: true
sub.text body: hello
r2.tag: req2
r2 instanceof Request: true
r2.method: POST
r2.json.n: 42"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: Request subclass body methods regressed"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
