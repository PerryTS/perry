#!/bin/bash
# End-to-end regression for manifest `buffer+len` parameters: every
# ArrayBufferView kind (each typed array, DataView, Buffer, subarrays and
# offset views) and ArrayBuffer pass (view start, BYTE length) without a copy,
# and `{ "kind": "buffer+len", "nullable": true }` passes (NULL, 0) for
# null/undefined/an omitted argument while the strict form throws.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${PERRY:-}" ]; then
  PERRY="$SCRIPT_DIR/../../target/release/perry"
  [ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../../target/debug/perry"
  if [ ! -f "$PERRY" ]; then
    echo "SKIP: perry binary not found (build with cargo build --release)"
    exit 0
  fi
fi
case "$PERRY" in
  /*) ;;
  *) PERRY="$(pwd)/$PERRY" ;;
esac
if ! command -v cc >/dev/null 2>&1 || ! command -v ar >/dev/null 2>&1; then
  echo "SKIP: cc/ar not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

LIBDIR="$TMPDIR/node_modules/native-span-fixture"
mkdir -p "$LIBDIR/src" "$LIBDIR/target/release"

cat > "$LIBDIR/native.c" << 'EOF'
#include <stdint.h>
/* Returns byte_len * 1000 + first byte, or -1 for a NULL pointer
   (-2 when NULL arrives with a nonzero length). Writes 0x7f to the last
   byte so the caller can observe the write through its own view. */
double span_probe(const uint8_t *p, uintptr_t n) {
    if (p == 0) return n == 0 ? -1.0 : -2.0;
    if (n == 0) return 0.0;
    double first = p[0];
    ((uint8_t *)p)[n - 1] = 0x7f;
    return (double)n * 1000.0 + first;
}
double span_probe_nullable(const uint8_t *p, uintptr_t n) { return span_probe(p, n); }
EOF
cc -c "$LIBDIR/native.c" -o "$LIBDIR/native.o"
ar rcs "$LIBDIR/target/release/libnative_span_fixture.a" "$LIBDIR/native.o"

cat > "$LIBDIR/package.json" << 'EOF'
{
  "name": "native-span-fixture",
  "version": "0.1.0",
  "perry": {
    "nativeLibrary": {
      "module": "native-span-fixture",
      "functions": [
        { "name": "span_probe", "params": ["buffer+len"], "returns": "f64" },
        {
          "name": "span_probe_nullable",
          "params": [{ "kind": "buffer+len", "nullable": true }],
          "returns": "f64"
        }
      ],
      "targets": {
        "macos": { "crate": "", "lib": "libnative_span_fixture.a" },
        "linux": { "crate": "", "lib": "libnative_span_fixture.a" }
      }
    }
  }
}
EOF

cat > "$LIBDIR/src/index.ts" << 'EOF'
declare function span_probe(bytes: any): number;
declare function span_probe_nullable(bytes?: any): number;
export { span_probe, span_probe_nullable };
EOF

cat > "$TMPDIR/main.ts" << 'EOF'
import { span_probe, span_probe_nullable } from "native-span-fixture/src/index";

function show(label: string, f: () => number): void {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, "throws", (e as Error).name);
  }
}

const f32 = new Float32Array([1, 2, 3, 4]);
new Uint8Array(f32.buffer)[0] = 9;
show("Float32Array(4)", () => span_probe(f32));
show("Float32Array sees native write", () => new Uint8Array(f32.buffer)[15]);
const sub = f32.subarray(1);
new Uint8Array(sub.buffer, sub.byteOffset)[0] = 5;
show("Float32Array.subarray(1)", () => span_probe(sub));
const ab = new ArrayBuffer(32);
new Uint8Array(ab)[8] = 6;
show("Int32Array(ab, 8, 2)", () => span_probe(new Int32Array(ab, 8, 2)));
show("Int32Array write lands in ab", () => new Uint8Array(ab)[15]);
show("Int8Array(3)", () => span_probe(new Int8Array(3)));
show("Uint16Array(3)", () => span_probe(new Uint16Array(3)));
show("Int16Array(3)", () => span_probe(new Int16Array(3)));
show("Uint32Array(3)", () => span_probe(new Uint32Array(3)));
show("Float64Array(3)", () => span_probe(new Float64Array(3)));
show("BigInt64Array(2)", () => span_probe(new BigInt64Array(2)));
show("BigUint64Array(2)", () => span_probe(new BigUint64Array(2)));
show("Uint8ClampedArray(3)", () => span_probe(new Uint8ClampedArray(3)));
show("Uint8Array(3)", () => span_probe(new Uint8Array(3)));
show("Buffer(5)", () => span_probe(Buffer.alloc(5, 2)));
show("ArrayBuffer(8)", () => span_probe(new ArrayBuffer(8)));
show("DataView(ab, 4, 4)", () => span_probe(new DataView(ab, 4, 4)));
show("number[]", () => span_probe([1, 2, 3]));
show("strict null", () => span_probe(null));
show("strict undefined", () => span_probe(undefined));
show("nullable null", () => span_probe_nullable(null));
show("nullable undefined", () => span_probe_nullable(undefined));
show("nullable omitted", () => span_probe_nullable());
show("nullable Float32Array(2)", () => span_probe_nullable(new Float32Array(2)));
show("nullable object", () => span_probe_nullable({}));
EOF

cat > "$TMPDIR/expected.txt" << 'EOF'
Float32Array(4) 16009
Float32Array sees native write 127
Float32Array.subarray(1) 12005
Int32Array(ab, 8, 2) 8006
Int32Array write lands in ab 127
Int8Array(3) 3000
Uint16Array(3) 6000
Int16Array(3) 6000
Uint32Array(3) 12000
Float64Array(3) 24000
BigInt64Array(2) 16000
BigUint64Array(2) 16000
Uint8ClampedArray(3) 3000
Uint8Array(3) 3000
Buffer(5) 5002
ArrayBuffer(8) 8000
DataView(ab, 4, 4) 4000
number[] throws TypeError
strict null throws TypeError
strict undefined throws TypeError
nullable null -1
nullable undefined -1
nullable omitted -1
nullable Float32Array(2) 8000
nullable object throws TypeError
EOF

cat > "$TMPDIR/package.json" << 'EOF'
{
  "name": "native-span-app",
  "version": "0.1.0",
  "perry": { "allow": { "nativeLibrary": ["native-span-fixture/src/index"] } },
  "dependencies": { "native-span-fixture": "0.1.0" }
}
EOF

cd "$TMPDIR"
if ! COMPILE_OUTPUT=$(PERRY_VERIFY_NATIVE_REGIONS=1 "$PERRY" compile main.ts --output test_bin 2>&1); then
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
fi
./test_bin > actual.txt 2>&1 || true
if ! diff -u expected.txt actual.txt; then
  echo "FAIL: buffer+len span kinds"
  exit 1
fi
echo "PASS"
