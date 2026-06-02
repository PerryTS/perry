#!/bin/bash
# Automated WASM target test suite
# Usage: ./tests/wasm/run_wasm_tests.sh
#
# Compiles each .ts test file to WASM HTML, then runs it in Node.js
# and compares output against expected output.

set -e
PERRY="./target/release/perry"
DIR="$(cd "$(dirname "$0")" && pwd)"
PASS=0
FAIL=0
ERRORS=""

if [ ! -f "$PERRY" ]; then
  echo "Building perry..."
  cargo build --release -p perry 2>/dev/null
fi

run_test() {
  local name="$1"
  local ts_file="$DIR/$name.ts"
  local expected_file="$DIR/$name.expected"
  local html_file="/tmp/perry_wasm_test_${name}.html"

  # Multi-module convention (issue #1071): a subdirectory `$name/` with an
  # `entry.ts` and any helper modules. The compiler walks imports from
  # entry.ts so siblings can be `import {...} from './helper'`.
  if [ -d "$DIR/$name" ] && [ -f "$DIR/$name/entry.ts" ]; then
    ts_file="$DIR/$name/entry.ts"
  fi

  if [ ! -f "$ts_file" ]; then
    echo "SKIP $name (no .ts file)"
    return
  fi

  # Compile to WASM
  if ! $PERRY "$ts_file" --target wasm -o "$html_file" 2>/dev/null; then
    echo "FAIL $name (compilation error)"
    FAIL=$((FAIL + 1))
    ERRORS="$ERRORS\n  $name: compilation failed"
    return
  fi

  # Run in Node.js
  local actual
  actual=$(node -e "
// Minimal DOM polyfill: wasm_runtime.js injects a <style> tag at module load,
// so 'document' must exist or the require fails before any test code runs.
globalThis.window = globalThis;
const _stub = { id: '', appendChild: () => {}, getElementById: () => null, style: {}, textContent: '' };
const _doc = { createElement: () => Object.assign({}, _stub), getElementById: () => null, title: '' };
_doc.head = _stub; _doc.body = _stub;
globalThis.document = _doc;
let __rafNow = 0;
const __rafTimers = new Map();
globalThis.requestAnimationFrame = (cb) => {
  const id = setTimeout(() => {
    __rafTimers.delete(id);
    __rafNow += 16;
    cb(__rafNow);
  }, 0);
  __rafTimers.set(id, id);
  return id;
};
globalThis.cancelAnimationFrame = (id) => {
  clearTimeout(id);
  __rafTimers.delete(id);
};
const fs = require('fs');
const html = fs.readFileSync('$html_file', 'utf8');
const scripts = [];
const re = /<script>([\s\S]*?)<\/script>/g;
let m;
while ((m = re.exec(html)) !== null) scripts.push(m[1]);
let s = scripts.join('\n');
s = 'const atob=(x)=>Buffer.from(x,\"base64\").toString(\"binary\");const _c=require(\"crypto\");if(!globalThis.crypto)globalThis.crypto={randomUUID:()=>_c.randomUUID(),getRandomValues:(a)=>_c.getRandomValues(a)};\n' + s;
s = s.replace(/^(bootPerryWasm\()/m, 'await \$1');
eval('(async()=>{' + s + '})().catch(e=>{console.error(\"WASM_ERROR:\",e.message);process.exit(1)});');
" 2>&1) || true

  # Compare
  if [ -f "$expected_file" ]; then
    local expected
    expected=$(cat "$expected_file")
    if [ "$actual" = "$expected" ]; then
      echo "PASS $name"
      PASS=$((PASS + 1))
    else
      echo "FAIL $name"
      echo "  expected: $(echo "$expected" | head -3)"
      echo "  actual:   $(echo "$actual" | head -3)"
      FAIL=$((FAIL + 1))
      ERRORS="$ERRORS\n  $name: output mismatch"
    fi
  else
    # No expected file — just check it doesn't crash
    if echo "$actual" | grep -q "WASM_ERROR:"; then
      echo "FAIL $name (runtime error)"
      echo "  $actual"
      FAIL=$((FAIL + 1))
      ERRORS="$ERRORS\n  $name: $actual"
    else
      echo "PASS $name (no crash)"
      PASS=$((PASS + 1))
      # Generate expected file
      echo "$actual" > "$expected_file"
      echo "  (generated $name.expected)"
    fi
  fi

  rm -f "$html_file"
}

echo "=== Perry WASM Target Test Suite ==="
echo ""

# Run all tests
for ts_file in "$DIR"/*.ts; do
  name=$(basename "$ts_file" .ts)
  run_test "$name"
done
# Multi-module tests live in numbered subdirectories with an `entry.ts`.
for dir in "$DIR"/*/; do
  name=$(basename "$dir")
  if [ -f "$dir/entry.ts" ] && [ ! -f "$DIR/$name.ts" ]; then
    run_test "$name"
  fi
done

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
if [ $FAIL -gt 0 ]; then
  echo -e "Failures:$ERRORS"
  exit 1
fi
