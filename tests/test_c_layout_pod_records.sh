#!/bin/bash
# Focused runtime regression for C-layout POD records. The marker types are
# erased at the TypeScript surface; JS-visible reads, writes, returns, and
# dynamic escapes must behave like an ordinary object.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

if [ -z "${PERRY:-}" ]; then
  BUILD_LOG="$TMPDIR/cargo-build.log"
  if ! cargo build -q -p perry >"$BUILD_LOG" 2>&1; then
    echo "FAIL: cargo build -p perry failed"
    tail -40 "$BUILD_LOG"
    exit 1
  fi
  PERRY="$REPO_ROOT/target/debug/perry"
fi

case "$PERRY" in
  /*) ;;
  *) PERRY="$(pwd)/$PERRY" ;;
esac

if [ ! -x "$PERRY" ]; then
  echo "FAIL: perry binary not found at $PERRY"
  exit 1
fi

cat > "$TMPDIR/main.ts" <<'EOF'
type Packet = PerryPod<{
  tag: PerryU32;
  gain: PerryF32;
  total: number;
  count: PerryBufferLen;
}>;

let packet: Packet = { tag: 7, gain: 1.5, total: 2.25, count: 4 };
let lied: Packet = { tag: 7, gain: 1.5, total: 2.25, count: 4 };
let inexact: Packet = { tag: (-1 as any), gain: (1.1 as any), total: 2.25, count: ("x" as any) };

console.log("read=" + packet.tag + "," + packet.gain + "," + packet.total + "," + packet.count);
console.log("init=" + inexact.tag + "," + inexact.count + "," + inexact.gain);
(lied as any).tag = "x";
console.log("lie=" + lied.tag);
packet.tag = 9;

function escapeAny(x: any): any {
  x.extra = 11;
  x.gain = 2.5;
  return x;
}

const escaped = escapeAny(packet);
console.log("after=" + packet.tag + "," + packet.gain + "," + escaped.extra);

function ret(): Packet {
  return packet;
}

console.log("return=" + ret().count);

function makeGetter(): any {
  let captured: Packet = { tag: 7, gain: 1.5, total: 2.25, count: 4 };
  const get = () => captured.tag;
  captured = { tag: 8, gain: 1.5, total: 2.25, count: 4 };
  return get;
}

console.log("capture=" + makeGetter()());
EOF

ARTIFACT_DIR="$TMPDIR/native-reps"
mkdir -p "$ARTIFACT_DIR"

cd "$TMPDIR"
COMPILE_OUTPUT=$(PERRY_NATIVE_REPS=1 \
  PERRY_NATIVE_REPS_DIR="$ARTIFACT_DIR" \
  PERRY_VERIFY_NATIVE_REGIONS=1 \
  "$PERRY" compile main.ts --output test_bin --no-cache 2>&1) || {
  echo "FAIL: compile error"
  echo "$COMPILE_OUTPUT" | tail -40
  exit 1
}

EXPECTED=$'read=7,1.5,2.25,4\ninit=-1,x,1.1\nlie=x\nafter=9,2.5,11\nreturn=4\ncapture=8'
RUN_OUTPUT=$(./test_bin 2>&1)
if [ "$RUN_OUTPUT" != "$EXPECTED" ]; then
  echo "FAIL: JS-visible POD behavior changed"
  echo "Expected:"
  echo "$EXPECTED"
  echo "Got:"
  echo "$RUN_OUTPUT"
  exit 1
fi

ARTIFACT_TEXT="$TMPDIR/native-reps.txt"
shopt -s nullglob
ARTIFACTS=("$ARTIFACT_DIR"/*.json)
shopt -u nullglob
if [ "${#ARTIFACTS[@]}" -eq 0 ]; then
  echo "FAIL: native reps artifact missing"
  exit 1
fi
cat "${ARTIFACTS[@]}" > "$ARTIFACT_TEXT"

if ! grep -Eq '"schema_version"[[:space:]]*:[[:space:]]*[0-9]+' "$ARTIFACT_TEXT"; then
  echo "FAIL: native reps artifact missing numeric schema_version"
  exit 1
fi

for token in \
  '"native_rep_name": "pod_record"' \
  '"pod_layouts"' \
  '"endian": "native"' \
  '"packing": "c"' \
  '"native_rep_name": "u32"' \
  '"native_rep_name": "f32"' \
  '"native_rep_name": "f64"' \
  '"native_rep_name": "buffer_len"' \
  '"consumer": "pod_record_materialize_object"' \
  '"consumer": "pod_record_field_set_dynamic_value"' \
  '"materialization_reason": "pod_dynamic_mutation"'; do
  if ! grep -Fq "$token" "$ARTIFACT_TEXT"; then
    echo "FAIL: native reps artifact missing $token"
    exit 1
  fi
done

echo "PASS"
