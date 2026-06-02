#!/bin/bash
# Regression: external native-library packages must be resolvable from the
# package root and TypeScript exports must explicitly forward to manifest
# `js_*` symbols. This mirrors the shape emitted by `perry native init`.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

if ! command -v cc >/dev/null 2>&1; then
  echo "SKIP: cc not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

LIBDIR="$TMPDIR/node_modules/entry-test-lib"
mkdir -p "$LIBDIR/src" "$LIBDIR/target/release"

cat > "$LIBDIR/native.c" << 'EOF'
double js_entry_test_answer(double value) {
    return value + 1.0;
}
EOF

cc -c "$LIBDIR/native.c" -o "$LIBDIR/native.o"
ar rcs "$LIBDIR/target/release/libentry_test.a" "$LIBDIR/native.o"

cat > "$LIBDIR/package.json" << 'EOF'
{
  "name": "entry-test-lib",
  "version": "0.1.0",
  "main": "src/index.ts",
  "types": "src/index.ts",
  "perry": {
    "nativeLibrary": {
      "module": "entry-test-lib",
      "functions": [
        { "name": "js_entry_test_answer", "params": ["f64"], "returns": "f64" }
      ],
      "targets": {
        "macos": { "crate": "", "lib": "libentry_test.a" },
        "linux": { "crate": "", "lib": "libentry_test.a" }
      }
    }
  }
}
EOF

cat > "$LIBDIR/src/index.ts" << 'EOF'
declare function js_entry_test_answer(value: number): number;

export function answer(): number {
  return js_entry_test_answer(41);
}
EOF

cat > "$TMPDIR/main.ts" << 'EOF'
import { answer } from "entry-test-lib";
console.log("answer=" + answer());
EOF

cat > "$TMPDIR/package.json" << 'EOF'
{
  "name": "entry-test-app",
  "version": "0.1.0",
  "dependencies": { "entry-test-lib": "0.1.0" }
}
EOF

cd "$TMPDIR"
COMPILE_OUTPUT=$("$PERRY" compile main.ts --output test_bin --no-cache 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
}

RUN_OUTPUT=$(./test_bin 2>&1)
EXPECTED="answer=42"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: package-root native binding did not call the FFI symbol"
echo "Expected: $EXPECTED"
echo "Got:      $RUN_OUTPUT"
exit 1
