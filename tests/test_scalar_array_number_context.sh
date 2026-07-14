#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$ROOT/target/release/perry}}"
RUNTIME_DIR="${PERRY_RUNTIME_DIR:-$ROOT/target/release}"
FIXTURE="$ROOT/tests/issue_scalar_array_number_context.js"
WORKDIR="${TMPDIR:-/tmp}/perry-scalar-array-number-$$"
BIN="$WORKDIR/perry-scalar-array-number"
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

set +e
"$BIN" >"$OUT_LOG" 2>&1
rc=$?
set -e

if [[ "$rc" -ne 0 ]]; then
  echo "scalar-array number-context fixture failed (exit $rc):" >&2
  cat "$OUT_LOG" >&2
  exit 1
fi

grep -qx "scalar-array number context ok" "$OUT_LOG" || {
  echo "unexpected output:" >&2
  cat "$OUT_LOG" >&2
  exit 1
}
