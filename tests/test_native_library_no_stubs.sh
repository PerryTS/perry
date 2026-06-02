#!/bin/bash
# Regression test: Perry must NOT generate stubs for functions declared
# in perry.nativeLibrary manifests.
#
# Background: Perry scans linked .o files with `nm` to find undefined symbols,
# then generates stubs for any that aren't defined elsewhere. When a package
# declares perry.nativeLibrary functions, those functions are expected to come
# from a separate native library (.a/.so) linked later. Perry must add them to
# defined_syms so stubs aren't generated.
#
# Without the fix, on Android the stubs (returning 0) in libperry_app.so shadow
# the real implementations in the separate libhone_editor_android.so, causing
# all FFI calls to silently return 0.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"

if [ ! -f "$PERRY" ]; then
    PERRY="$SCRIPT_DIR/../target/debug/perry"
fi
if [ ! -f "$PERRY" ]; then
    echo "SKIP: perry binary not found (build with cargo build --release)"
    exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Create a minimal package with native library functions
mkdir -p "$TMPDIR/node_modules/test-native-lib"

cat > "$TMPDIR/node_modules/test-native-lib/package.json" << 'PKGJSON'
{
  "name": "test-native-lib",
  "version": "0.1.0",
  "perry": {
    "nativeLibrary": {
      "functions": [
        { "name": "native_lib_create", "params": ["f64", "f64"], "returns": "ptr" },
        { "name": "native_lib_render", "params": ["ptr", "f64"], "returns": "void" },
        { "name": "native_lib_get_value", "params": ["ptr"], "returns": "f64" },
        { "name": "native_lib_destroy", "params": ["ptr"], "returns": "void" }
      ]
    }
  }
}
PKGJSON

# Create the TypeScript source that declares and uses these functions
mkdir -p "$TMPDIR/node_modules/test-native-lib/src"
cat > "$TMPDIR/node_modules/test-native-lib/src/index.ts" << 'TSFILE'
declare function native_lib_create(w: number, h: number): number;
declare function native_lib_render(handle: number, value: number): void;
declare function native_lib_get_value(handle: number): number;
declare function native_lib_destroy(handle: number): void;

export function init(): number {
  const handle = native_lib_create(100, 200);
  native_lib_render(handle, 42);
  const v = native_lib_get_value(handle);
  native_lib_destroy(handle);
  return v;
}
TSFILE

# Create the main app
cat > "$TMPDIR/main.ts" << 'MAIN'
import { init } from 'test-native-lib/src/index';
const result = init();
MAIN

cat > "$TMPDIR/package.json" << 'APPPKG'
{
  "name": "test-app",
  "version": "0.1.0",
  "perry": {
    "allow": {
      "nativeLibrary": ["test-native-lib/src/index"]
    }
  },
  "dependencies": {
    "test-native-lib": "0.1.0"
  }
}
APPPKG

# Compile and capture output (link will fail since no real native lib, that's OK)
cd "$TMPDIR"
COMPILE_OUTPUT=$("$PERRY" compile main.ts -o test_binary 2>&1) || true

# The key check: the stub generation output lists each stubbed function.
# Native library functions must NOT appear in that list.
FAIL=0
for func in native_lib_create native_lib_render native_lib_get_value native_lib_destroy; do
    if echo "$COMPILE_OUTPUT" | grep -qF -- "- $func"; then
        echo "FAIL: Perry generated a stub for native library function: $func"
        FAIL=1
    fi
done

if [ "$FAIL" -eq 1 ]; then
    echo ""
    echo "Perry is generating stubs for functions declared in perry.nativeLibrary."
    echo "These stubs (returning 0) shadow real implementations when the native"
    echo "library is linked as a separate .so (e.g. Android)."
    echo ""
    echo "Fix: ensure native library function names are added to defined_syms in"
    echo "compile.rs before the stub generation loop."
    echo ""
    echo "Stub output:"
    echo "$COMPILE_OUTPUT" | grep -E "stubs|^    -"
    exit 1
fi

# Verify the compilation recognized the native library
if ! echo "$COMPILE_OUTPUT" | grep -qF "FFI functions"; then
    echo "FAIL: Perry did not detect the native library manifest"
    echo "$COMPILE_OUTPUT" | head -5
    exit 1
fi

echo "PASS: No stubs generated for native library functions"
exit 0
