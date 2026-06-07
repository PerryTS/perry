#!/bin/bash
# Regression: a `class X extends Request` instance must (1) expose the body
# methods (`text`/`json`/`formData`/...) as READABLE callable values — both
# `r.text` and the COMPUTED `r["text"]` (@hono/node-server reads the body via
# `this[getRequestCache]()[k]()`, a computed member call), and (2) read a
# `ReadableStream` body, not just a string body.
#
# Bug (this PR): the property READ of a body method on a fetch-subclass forwarded
# to the native handle id as if it were an ObjectHeader and returned `undefined`,
# so `r["text"]` was not a function ("text is not a function"). And a
# `ReadableStream` request body was coerced as a string — the stream HANDLE got
# stringified to its numeric id, so `await r.text()` resolved to a bogus number.
#
# Fix: body-method reads on a fetch-subclass return a bound method that
# re-dispatches through the native handle; the Request thunk drains a
# ReadableStream body (buffered chunks) into bytes via the registered
# `js_response_body_init_ptr`.

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

cat > "$TMPDIR/main.ts" << "EOF"
const GlobalRequest = (globalThis as any).Request;
class Req extends GlobalRequest {
  constructor(input: any, init?: any) { super(input, init); }
}
async function main() {
  // String body via literal + computed body-method read.
  const r = new Req("http://x/y", { method: "POST", body: "HI" });
  console.log("typeof r.text:", typeof r.text);
  const k = "text";
  console.log("typeof r[k]:", typeof (r as any)[k]);
  console.log("literal text:", await r.text());
  // ReadableStream body (single synchronously-enqueued chunk) — drained.
  const r2 = new Req("http://x/z", {
    method: "POST",
    body: new ReadableStream({
      start(c: any) { c.enqueue(new TextEncoder().encode("STREAMED")); c.close(); },
    }),
  });
  const fn = (r2 as any)["text"];
  console.log("computed stream text:", await fn.call(r2));
}
main();
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts -o test_bin >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="typeof r.text: function
typeof r[k]: function
literal text: HI
computed stream text: STREAMED"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi
echo "FAIL: Request subclass stream/computed body methods regressed"
echo "Expected:"; echo "$EXPECTED"
echo ""; echo "Got:"; echo "$RUN_OUTPUT"
exit 1
