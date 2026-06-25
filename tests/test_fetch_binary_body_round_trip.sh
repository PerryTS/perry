#!/bin/bash
# Regression: a fetch/Blob/Response body that is NOT valid UTF-8 (e.g. a
# fetched PNG/protobuf) must survive `arrayBuffer()` byte-exact, and a
# UTF-8 body must still decode losslessly through `text()`/`json()`.
#
# Bug: `js_response_array_buffer` / `js_blob_array_buffer` round-tripped the
# body through `from_utf8_unchecked` → String → a STRING_TAG JsValue. That
# was UB on non-UTF-8 bytes, and the JS `new Uint8Array(value)` dispatch keys
# on the Buffer POINTER_TAG — a STRING_TAG value reads as an EMPTY buffer. So
# `new Uint8Array(await res.arrayBuffer())` came back empty / corrupted for
# any binary payload.
#
# Fix: resolve a real Buffer (`alloc_buffer` + POINTER_TAG), byte-exact —
# the same shape `bytes()` already used.

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
async function main() {
  // Non-UTF-8 bytes: 0xFF 0xFE are invalid UTF-8, 0x00 is an embedded NUL.
  const bin = new Uint8Array([0xFF, 0xFE, 0x00, 0x80, 0x50, 0x4E, 0x47]);

  // Blob.arrayBuffer() — binary must round-trip byte-exact.
  const ab = await new Blob([bin]).arrayBuffer();
  console.log("blob.arrayBuffer:", Array.from(new Uint8Array(ab)).join(","));

  // Response.text() — UTF-8 body (incl. a multi-byte char) preserved.
  const res = new Response('{"msg":"héllo","n":42}');
  console.log("res.text:", await res.text());

  // Response.json() — body parsed to an object.
  const j = await new Response('{"msg":"héllo","n":42}').json();
  console.log("res.json:", (j as any).msg, (j as any).n);

  // Response.arrayBuffer() — bytes of a (UTF-8) body, byte-exact.
  const ab2 = await new Response("ABC").arrayBuffer();
  console.log("res.arrayBuffer:", Array.from(new Uint8Array(ab2)).join(","));
}
main().catch((e) => console.log("ERR:", e?.message ?? e));
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="blob.arrayBuffer: 255,254,0,128,80,78,71
res.text: {\"msg\":\"héllo\",\"n\":42}
res.json: héllo 42
res.arrayBuffer: 65,66,67"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: fetch/Blob binary body round-trip regressed"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
