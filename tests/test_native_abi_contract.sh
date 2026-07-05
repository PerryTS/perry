#!/bin/bash
# Focused runtime regression for the native ABI contract:
# native-library returns/params use explicit u32/u64/usize/f32/buffer_len/
# handle/promise ABI reps, Buffer fast paths still materialize as JS-visible
# numbers, and raw handle-like values can cross a dynamic JS boundary only
# through explicit boxing/unboxing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY_PROVIDED=0
if [ -n "${PERRY:-}" ]; then
  PERRY_PROVIDED=1
else
  PERRY="$SCRIPT_DIR/../target/release/perry"
  [ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
  if [ ! -f "$PERRY" ]; then
    echo "SKIP: perry binary not found (build with cargo build --release)"
    exit 0
  fi
fi

case "$PERRY" in
  /*) ;;
  *) PERRY="$(pwd)/$PERRY" ;;
esac

if [ "$PERRY_PROVIDED" -eq 1 ] && [ ! -x "$PERRY" ]; then
  echo "FAIL: perry binary not found at $PERRY"
  exit 1
fi

if ! command -v cc >/dev/null 2>&1; then
  echo "SKIP: cc not available"
  exit 0
fi

if ! command -v ar >/dev/null 2>&1; then
  echo "SKIP: ar not available"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

EVIDENCE_DIR="${PERRY_NATIVE_ABI_EVIDENCE_DIR:-${NATIVE_ABI_EVIDENCE_DIR:-}}"
if [ -n "$EVIDENCE_DIR" ]; then
  case "$EVIDENCE_DIR" in
    /*) ;;
    *) EVIDENCE_DIR="$(pwd)/$EVIDENCE_DIR" ;;
  esac
  mkdir -p "$EVIDENCE_DIR"
fi

write_evidence() {
  local name="$1"
  local text="$2"
  if [ -n "$EVIDENCE_DIR" ]; then
    printf '%s\n' "$text" > "$EVIDENCE_DIR/$name"
  fi
}

copy_native_reps_evidence() {
  if [ -n "$EVIDENCE_DIR" ]; then
    mkdir -p "$EVIDENCE_DIR/native-reps"
    cp "$ARTIFACT_DIR"/*.json "$EVIDENCE_DIR/native-reps/" 2>/dev/null || true
    cp "$ARTIFACT_TEXT" "$EVIDENCE_DIR/native-reps.txt" 2>/dev/null || true
  fi
}

LIBDIR="$TMPDIR/node_modules/native-abi-contract-fixture"
mkdir -p "$LIBDIR/src" "$LIBDIR/target/release"

cat > "$LIBDIR/native.c" << 'EOF'
#include <stdint.h>

static uint64_t HANDLE_SENTINEL = 0xfeed12345678ULL;
static uint64_t PROMISE_SENTINEL = 0xbee812345678ULL;

typedef struct AbiPacket {
    uint32_t tag;
    float gain;
    double total;
    uint32_t count;
} AbiPacket;

uint32_t abi_contract_ret_u32(void) {
    return 4000000000u;
}

uint64_t abi_contract_ret_u64(void) {
    return 4294967297ULL;
}

uintptr_t abi_contract_ret_usize(void) {
    return (uintptr_t)65537u;
}

float abi_contract_ret_f32(void) {
    return 6.25f;
}

uint32_t abi_contract_ret_buffer_len(void) {
    return 12u;
}

uintptr_t abi_contract_ret_handle(void) {
    return (uintptr_t)&HANDLE_SENTINEL;
}

uintptr_t abi_contract_ret_promise(void) {
    return (uintptr_t)&PROMISE_SENTINEL;
}

uint32_t abi_contract_check_all(
    uint32_t u32_value,
    uint64_t u64_value,
    uintptr_t usize_value,
    float f32_value,
    uint32_t buffer_len,
    const uint8_t *buffer_data,
    uintptr_t buffer_byte_len,
    uintptr_t handle,
    uintptr_t promise
) {
    return u32_value == 4000000000u
        && u64_value == 4294967297ULL
        && usize_value == (uintptr_t)65537u
        && f32_value == 6.25f
        && buffer_len == 12u
        && buffer_data != 0
        && buffer_byte_len == 12u
        && buffer_data[0] == 18u
        && buffer_data[1] == 52u
        && handle == (uintptr_t)&HANDLE_SENTINEL
        && promise != 0
        ? 777u
        : 13u;
}

uint32_t abi_contract_check_packet(const AbiPacket *packet) {
    return packet != 0
        && packet->tag == 7u
        && packet->gain == 1.5f
        && packet->total == 2.25
        && packet->count == 4u
        ? 778u
        : 14u;
}
EOF

cc -c "$LIBDIR/native.c" -o "$LIBDIR/native.o"
ar rcs "$LIBDIR/target/release/libnative_abi_contract_fixture.a" "$LIBDIR/native.o"

cat > "$LIBDIR/package.json" << 'EOF'
{
  "name": "native-abi-contract-fixture",
  "version": "0.1.0",
  "perry": {
    "nativeLibrary": {
      "module": "native-abi-contract-fixture",
      "functions": [
        { "name": "abi_contract_ret_u32", "params": [], "returns": "u32" },
        { "name": "abi_contract_ret_u64", "params": [], "returns": "u64" },
        { "name": "abi_contract_ret_usize", "params": [], "returns": "usize" },
        { "name": "abi_contract_ret_f32", "params": [], "returns": "f32" },
        { "name": "abi_contract_ret_buffer_len", "params": [], "returns": "buffer_len" },
        { "name": "abi_contract_ret_handle", "params": [], "returns": "handle" },
        { "name": "abi_contract_ret_promise", "params": [], "returns": "promise" },
        {
          "name": "abi_contract_check_all",
          "params": ["u32", "u64", "usize", "f32", "buffer_len", "buffer+len", "handle", "promise"],
          "returns": "u32"
        },
        {
          "name": "abi_contract_check_packet",
          "params": [
            {
              "kind": "pod",
              "name": "AbiPacket",
              "fields": [
                { "name": "tag", "type": "u32" },
                { "name": "gain", "type": "f32" },
                { "name": "total", "type": "number" },
                { "name": "count", "type": "buffer_len" }
              ]
            }
          ],
          "returns": "u32"
        }
      ],
      "targets": {
        "macos": { "crate": "", "lib": "libnative_abi_contract_fixture.a" },
        "linux": { "crate": "", "lib": "libnative_abi_contract_fixture.a" }
      }
    }
  }
}
EOF

cat > "$LIBDIR/src/index.ts" << 'EOF'
declare function abi_contract_ret_u32(): number;
declare function abi_contract_ret_u64(): number;
declare function abi_contract_ret_usize(): number;
declare function abi_contract_ret_f32(): number;
declare function abi_contract_ret_buffer_len(): number;
declare function abi_contract_ret_handle(): any;
declare function abi_contract_ret_promise(): any;
type AbiPacket = PerryPod<{
  tag: PerryU32;
  gain: PerryF32;
  total: number;
  count: PerryBufferLen;
}>;
declare function abi_contract_check_all(
  u32Value: number,
  u64Value: number,
  usizeValue: number,
  f32Value: number,
  bufferLen: number,
  buffer: Buffer,
  handle: any,
  promise: any
): number;
declare function abi_contract_check_packet(packet: AbiPacket): number;

function throughDynamicBoundary(value: any): any {
  return value;
}

export function runNativeAbiContract(): number {
  const buf = Buffer.alloc(12);
  buf[0] = 18;
  buf[1] = 52;
  buf[2] = 86;
  buf[3] = 120;
  buf[4] = 0;
  buf[5] = 0;
  buf[6] = 200;
  buf[7] = 64;

  const u32Value = abi_contract_ret_u32();
  const u64Value = abi_contract_ret_u64();
  const usizeValue = abi_contract_ret_usize();
  const f32Value = abi_contract_ret_f32();
  const nativeBufferLen = abi_contract_ret_buffer_len();
  const handle = throughDynamicBoundary(abi_contract_ret_handle());
  const nativePromise = throughDynamicBoundary(abi_contract_ret_promise());
  const promise = Promise.resolve(1);
  const packet: AbiPacket = { tag: 7, gain: 1.5, total: 2.25, count: 4 };
  const bufferLen = buf.length;
  const bufferU32 = buf.readUInt32BE(0);
  const bufferF32 = buf.readFloatLE(4);

  if (abi_contract_check_all(u32Value, u64Value, usizeValue, f32Value, bufferLen, buf, handle, promise) !== 777) {
    return 10;
  }
  if (abi_contract_check_all(4000000000, 4294967297, 65537, 6.25, nativeBufferLen, buf, handle, promise) !== 777) {
    return 20;
  }
  if (abi_contract_check_packet(packet) !== 778) {
    return 25;
  }
  if (u32Value !== 4000000000) return 30;
  if (u64Value !== 4294967297) return 40;
  if (usizeValue !== 65537) return 50;
  if (f32Value !== 6.25) return 60;
  if (nativeBufferLen !== 12) return 70;
  if (!nativePromise) return 75;
  if (bufferLen !== 12) return 80;
  if (bufferU32 !== 305419896) return 90;
  if (bufferF32 !== 6.25) return 100;
  return 1;
}
EOF

cat > "$TMPDIR/main.ts" << 'EOF'
import { runNativeAbiContract } from 'native-abi-contract-fixture/src/index';

const result = runNativeAbiContract();
if (result === 1) {
  console.log("PASS");
} else {
  console.log("FAIL");
  console.log(result);
}
EOF

cat > "$TMPDIR/package.json" << 'EOF'
{
  "name": "native-abi-contract-app",
  "version": "0.1.0",
  "perry": {
    "allow": {
      "nativeLibrary": ["native-abi-contract-fixture/src/index"]
    }
  },
  "dependencies": {
    "native-abi-contract-fixture": "0.1.0"
  }
}
EOF

ARTIFACT_DIR="$TMPDIR/native-reps"
mkdir -p "$ARTIFACT_DIR"

cd "$TMPDIR"
COMPILE_OUTPUT=$(PERRY_NATIVE_REPS=1 \
  PERRY_NATIVE_REPS_DIR="$ARTIFACT_DIR" \
  PERRY_VERIFY_NATIVE_REGIONS=1 \
  "$PERRY" compile main.ts --output test_bin 2>&1) || {
  write_evidence "compile.log" "$COMPILE_OUTPUT"
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
}
write_evidence "compile.log" "$COMPILE_OUTPUT"

RUN_OUTPUT=$(./test_bin 2>&1)
write_evidence "runtime.stdout" "$RUN_OUTPUT"
RUN_LAST_LINE=$(printf '%s\n' "$RUN_OUTPUT" | tail -n 1)
if [ "$RUN_LAST_LINE" != "PASS" ]; then
  echo "FAIL: JS-visible native ABI behavior changed"
  echo "Expected: PASS"
  echo "Got:      $RUN_OUTPUT"
  exit 1
fi

ARTIFACT_TEXT="$TMPDIR/native-reps.txt"
shopt -s nullglob
ARTIFACTS=("$ARTIFACT_DIR"/*.json)
shopt -u nullglob
if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "FAIL: native reps artifact missing"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
fi
cat "${ARTIFACTS[@]}" > "$ARTIFACT_TEXT"
copy_native_reps_evidence

if ! grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*[0-9]+' "$ARTIFACT_TEXT"; then
  echo "FAIL: native reps artifact missing numeric schema_version"
  echo "$COMPILE_OUTPUT" | tail -20
  exit 1
fi

for token in \
  '"consumer": "native_library.raw_u32"' \
  '"consumer": "native_library.raw_u64"' \
  '"consumer": "native_library.raw_usize"' \
  '"consumer": "native_library.raw_f32"' \
  '"consumer": "native_library.raw_buffer_len"' \
  '"consumer": "native_library.raw_handle"' \
  '"consumer": "native_library.raw_promise"' \
  '"consumer": "native_library.param.pod"' \
  '"consumer": "BufferNumericRead.native_u32"' \
  '"consumer": "BufferNumericRead.native_f32"' \
  '"consumer": "Buffer.length.native_buffer_len"' \
  '"native_rep_name": "u32"' \
  '"native_rep_name": "u64"' \
  '"native_rep_name": "usize"' \
  '"native_rep_name": "f32"' \
  '"native_rep_name": "buffer_len"' \
  '"native_rep_name": "native_handle"' \
  '"native_rep_name": "promise_boundary"' \
  '"native_rep_name": "pod_record"' \
  '"canonical_kind": "pod"' \
  '"pod_fields"' \
  '"op": "unsigned_int_to_float"' \
  '"op": "float_extend"' \
  '"op": "native_handle_box"' \
  '"op": "promise_box"'; do
  if ! grep -qF "$token" "$ARTIFACT_TEXT"; then
    echo "FAIL: native-reps artifact missing $token"
    echo "$COMPILE_OUTPUT" | tail -20
    exit 1
  fi
done

echo "PASS"
