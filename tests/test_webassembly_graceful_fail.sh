#!/bin/bash
# #6558 — graceful-fail baseline for the WebAssembly global (perry-only
# assertions; the node-parity side lives in
# test-files/test_gap_6558_webassembly_graceful_fail.ts and
# test-parity/node-suite/globals/webassembly-graceful-degradation.ts).
#
# What node CANNOT parity-check: in the default perry build (no wasmi host
# linked), even a perfectly VALID wasm module must be answered honestly —
# `validate` → false, `compile` → rejection whose CompileError message names
# the API and points at issue #6558 — while the program continues and exits 0.
#
# Uses the ALIASED namespace spelling: the literal `WebAssembly.compile(...)`
# form lowers to the wasmi host intrinsics (issue #76) and auto-links
# libperry_wasm_host.a, which is exactly the path this test must avoid.

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

cat > "$TMPDIR/graceful.ts" << 'EOF'
const WA: any = (globalThis as any).WebAssembly;

// The canonical valid i32 add module (~41 bytes):
//   (module (func (export "add") (param i32 i32) (result i32)
//                  local.get 0 local.get 1 i32.add))
const validAdd = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
  0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01,
  0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x0a, 0x09,
  0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a,
  0x0b,
]);

// Honest feature detection: a VALID module is still not runnable here.
console.log("validate valid:", WA.validate(validAdd));

// compile(valid) rejects with a CompileError pointing at #6558.
try {
  await WA.compile(validAdd);
  console.log("compile: resolved");
} catch (e: any) {
  console.log("compile rejected:", e instanceof WA.CompileError, e.name);
  console.log("mentions api:", e.message.includes("WebAssembly.compile"));
  console.log("mentions issue:", e.message.includes("6558"));
}

// new Module(valid) throws a CompileError pointing at #6558.
try {
  new WA.Module(validAdd);
  console.log("Module: constructed");
} catch (e: any) {
  console.log("Module threw:", e instanceof WA.CompileError, e.name);
  console.log("Module mentions issue:", e.message.includes("6558"));
}

// instantiate(valid) rejects; the loader pattern degrades to null and the
// program CONTINUES (photon-node shape).
async function lazyLoad(bytes: Uint8Array): Promise<unknown | null> {
  try {
    return await WA.instantiate(bytes);
  } catch {
    return null;
  }
}
const loaded = await lazyLoad(validAdd);
console.log("loader result:", loaded === null ? "degraded" : "loaded");
console.log("program continues");
EOF

OUT="$TMPDIR/out.txt"
"$PERRY" compile "$TMPDIR/graceful.ts" -o "$TMPDIR/graceful" > "$TMPDIR/compile.log" 2>&1 || {
  echo "FAIL: perry compile failed"
  tail -20 "$TMPDIR/compile.log"
  exit 1
}
"$TMPDIR/graceful" > "$OUT" 2>&1 || {
  echo "FAIL: compiled binary exited non-zero"
  cat "$OUT"
  exit 1
}

expect() {
  if ! grep -qF "$1" "$OUT"; then
    echo "FAIL: missing expected line: $1"
    echo "--- actual output ---"
    cat "$OUT"
    exit 1
  fi
}

expect "validate valid: false"
expect "compile rejected: true CompileError"
expect "mentions api: true"
expect "mentions issue: true"
expect "Module threw: true CompileError"
expect "Module mentions issue: true"
expect "loader result: degraded"
expect "program continues"

echo "PASS: WebAssembly namespace degrades gracefully (#6558 baseline)"
