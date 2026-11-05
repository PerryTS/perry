#!/bin/bash
# Regression: native-library FFI calls ignored the package.json manifest
# and used a Perry-double ABI (d-registers) for everything. C functions
# that actually return i64 (handles, pointers) write to x0; reading from
# d0 yielded `0` and the next FFI call dereferenced null.
#
# Fix: `lower_call`'s native-library path now consults `ffi_signatures`:
#   - param `"i64"` → fptosi → I64 register (x-reg on ARM64)
#   - return `"i64"` → declare/call as I64, then sitofp back to f64
#
# This test builds a tiny C library with one i64-returning function
# and one i64-taking-and-returning function, links it via Perry's
# package.json `nativeLibrary` mechanism, and verifies the value
# round-trips. Pre-fix the value would be lost (printed 0 or NaN).

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../../target/debug/perry"
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

LIBDIR="$TMPDIR/node_modules/abi-test-lib"
mkdir -p "$LIBDIR/src" "$LIBDIR/target/release"

# C function with i64 return + one with i64 param-and-return.
# A magic value (0x123456789abcULL) won't survive an f64-d0 ABI mismatch
# because d0's high bits are sign/exponent of an f64, not an integer.
cat > "$LIBDIR/native.c" << 'EOF'
#include <stdint.h>
int64_t abi_test_make_handle(double w, double h) {
    (void)w; (void)h;
    return 0x123456789abcLL;
}
int64_t abi_test_round_trip(int64_t handle) {
    return handle;
}
EOF

# Build to a static archive in the location Perry's nativeLibrary linker
# step expects: <crate>/target/release/<lib>. We use an empty `crate` path
# (= package dir) and pre-build the archive ourselves so Perry skips its
# `cargo build` step (no Cargo.toml present).
cc -c "$LIBDIR/native.c" -o "$LIBDIR/native.o"
ar rcs "$LIBDIR/target/release/libabi_test.a" "$LIBDIR/native.o"

cat > "$LIBDIR/package.json" << EOF
{
  "name": "abi-test-lib",
  "version": "0.1.0",
  "perry": {
    "nativeLibrary": {
      "module": "abi-test-lib",
      "functions": [
        { "name": "abi_test_make_handle", "params": ["f64", "f64"], "returns": "i64" },
        { "name": "abi_test_round_trip", "params": ["i64"], "returns": "i64" }
      ],
      "targets": {
        "macos": { "crate": "", "lib": "libabi_test.a" },
        "linux": { "crate": "", "lib": "libabi_test.a" }
      }
    }
  }
}
EOF

cat > "$LIBDIR/src/index.ts" << 'EOF'
declare function abi_test_make_handle(w: number, h: number): number;
declare function abi_test_round_trip(handle: number): number;
export function check(): number {
  const h = abi_test_make_handle(100, 200);
  return abi_test_round_trip(h);
}
EOF

cat > "$TMPDIR/main.ts" << 'EOF'
import { check } from 'abi-test-lib/src/index';
const v = check();
// 0x123456789abc = 20015998343868. Print as integer to avoid f64 noise.
console.log("value=" + v);
EOF

cat > "$TMPDIR/package.json" << 'EOF'
{
  "name": "abi-test-app",
  "version": "0.1.0",
  "dependencies": { "abi-test-lib": "0.1.0" }
}
EOF

cd "$TMPDIR"
COMPILE_OUTPUT=$("$PERRY" compile main.ts --output test_bin 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -10
  exit 1
}

RUN_OUTPUT=$(./test_bin 2>&1)
EXPECTED="value=20015998343868"

if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: i64 FFI value didn't round-trip"
echo "Expected: $EXPECTED"
echo "Got:      $RUN_OUTPUT"
echo ""
echo "Pre-fix this typically prints 0 (d0 read instead of x0) or NaN."
exit 1
