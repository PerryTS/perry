#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/issue_typed_array_backing_gc.js"
WORKDIR="${TMPDIR:-/tmp}/perry-ta-backing-gc-$$"
BIN="$WORKDIR/perry-ta-backing-gc"
COMPILE_LOG="$WORKDIR/compile.log"
OUT_LOG="$WORKDIR/out.log"

mkdir -p "$WORKDIR"
trap 'rm -rf "$WORKDIR"' EXIT

if [[ ! -x "$PERRY" ]]; then
  PERRY="$ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
  echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
  exit 0
fi

env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_RUNTIME_DIR="$RUNTIME_DIR" "$PERRY" compile --no-cache --no-auto-optimize "$FIXTURE" -o "$BIN" \
  >"$COMPILE_LOG" 2>&1 || {
    cat "$COMPILE_LOG" >&2
    exit 1
  }

# Run under the default collector and again with evacuation forced, so both the
# sweep and the relocation path are covered.
for mode in default evacuate; do
  set +e
  if [[ "$mode" == "evacuate" ]]; then
    PERRY_GC_FORCE_EVACUATE=1 "$BIN" >"$OUT_LOG" 2>&1
  else
    "$BIN" >"$OUT_LOG" 2>&1
  fi
  rc=$?
  set -e
  if [[ "$rc" -ne 0 ]]; then
    echo "typed-array backing fixture failed ($mode GC, exit $rc):" >&2
    cat "$OUT_LOG" >&2
    exit 1
  fi
  grep -qx "typed-array backing survived GC" "$OUT_LOG" || {
    echo "unexpected output ($mode GC):" >&2
    cat "$OUT_LOG" >&2
    exit 1
  }
done
