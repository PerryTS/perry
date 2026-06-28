#!/bin/bash
# Regression: compilePackages subpath exports must resolve to the subpath's
# declared source entry, not fall back to the package root src/index.ts.

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

PKG="$TMPDIR/node_modules/pkg"
CONSUMER="$TMPDIR/node_modules/consumer"
mkdir -p "$PKG/dist/feature" "$PKG/src/feature" "$CONSUMER/src"

cat > "$TMPDIR/package.json" << 'JSON'
{
  "type": "module",
  "perry": {
    "compilePackages": ["pkg", "consumer"],
    "allow": { "compilePackages": ["pkg", "consumer"] }
  }
}
JSON

cat > "$TMPDIR/main.ts" << 'TS'
import { run } from 'consumer'
console.log('value=' + run())
TS

cat > "$PKG/package.json" << 'JSON'
{
  "name": "pkg",
  "type": "module",
  "exports": {
    ".": { "import": { "default": "./src/index.ts" } },
    "./feature": { "import": { "default": "./dist/feature/server.js" } }
  }
}
JSON

cat > "$PKG/src/index.ts" << 'TS'
export const rootOnly = 1
TS

cat > "$PKG/dist/feature/server.js" << 'JS'
export const subValue = 0
JS

cat > "$PKG/src/feature/server.tsx" << 'TS'
export const subValue = 41
TS

cat > "$CONSUMER/package.json" << 'JSON'
{
  "name": "consumer",
  "type": "module",
  "exports": {
    ".": { "import": { "default": "./src/index.ts" } }
  },
  "dependencies": {
    "pkg": "1.0.0"
  }
}
JSON

cat > "$CONSUMER/src/index.ts" << 'TS'
import { subValue } from 'pkg/feature'
export function run() { return subValue + 1 }
TS

cd "$TMPDIR"
COMPILE_OUTPUT=$("$PERRY" compile --no-cache main.ts -o out 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -40
  exit 1
}

RUN_OUTPUT=$(./out 2>&1)
if [ "$RUN_OUTPUT" = "value=42" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: package subpath exports output mismatch"
echo "Expected: value=42"
echo "Got:      $RUN_OUTPUT"
exit 1
