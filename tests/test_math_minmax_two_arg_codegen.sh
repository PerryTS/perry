#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

SRC="$TMPDIR/math_minmax_two_arg_codegen.ts"
BIN="$TMPDIR/math_minmax_two_arg_codegen"
OBJ="$TMPDIR/math_minmax_two_arg_codegen.o"

cat >"$SRC" <<'TS'
let failures = 0;

function check(label: string, actual: number, expected: number): void {
  if (actual !== expected) {
    console.log(label + ": expected " + String(expected) + ", got " + String(actual));
    failures = failures + 1;
  }
}

function checkNaN(label: string, actual: number): void {
  if (actual === actual) {
    console.log(label + ": expected NaN, got " + String(actual));
    failures = failures + 1;
  }
}

function checkReciprocal(label: string, actual: number, expected: number): void {
  if (actual !== 0 || 1 / actual !== expected) {
    console.log(label + ": unexpected zero sign");
    failures = failures + 1;
  }
}

function clip(start: number, count: number, rangeStart: number, rangeCount: number): number {
  const drawStart = Math.max(start, rangeStart);
  const drawEnd = Math.min(start + count, rangeStart + rangeCount);
  return drawEnd - drawStart;
}

check("clip overlap", clip(1, 10, 3, 4), 4);
check("min order", Math.min(4, -2), -2);
check("max order", Math.max(4, -2), 4);
checkNaN("min nan", Math.min(NaN, 1));
checkNaN("max nan", Math.max(1, NaN));
checkReciprocal("min signed zero", Math.min(0, -0), -Infinity);
checkReciprocal("max signed zero", Math.max(-0, 0), Infinity);

if (failures !== 0) {
  throw new Error("two-arg Math.min/Math.max regression failed");
}

console.log("two-arg Math.min/Math.max ok");
TS

"$PERRY" compile --no-cache --no-auto-optimize "$SRC" -o "$BIN" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}

"$BIN" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "two-arg Math.min/Math.max ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

(
    cd "$TMPDIR"
    "$PERRY" compile --no-cache --no-auto-optimize --trace llvm --focus clip --no-link \
        "$SRC" -o "$OBJ" >"$TMPDIR/trace-compile.log" 2>&1
) || {
    echo "FAIL: trace compile failed"
    sed 's/^/    /' "$TMPDIR/trace-compile.log" | tail -80
    exit 1
}

TRACE_DIR="$TMPDIR/.perry-trace/llvm"
if [[ ! -d "$TRACE_DIR" ]]; then
    echo "FAIL: LLVM trace directory not found"
    exit 1
fi

if ! grep -R "call double @js_math_min2" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: expected js_math_min2 call in LLVM trace"
    exit 1
fi

if ! grep -R "call double @js_math_max2" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: expected js_math_max2 call in LLVM trace"
    exit 1
fi

if grep -R "call double @js_math_min_array" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: unexpected js_math_min_array call in two-arg LLVM trace"
    exit 1
fi

if grep -R "call double @js_math_max_array" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: unexpected js_math_max_array call in two-arg LLVM trace"
    exit 1
fi

if grep -R "call i64 @js_array_alloc(i32 2)" "$TRACE_DIR" >/dev/null; then
    echo "FAIL: unexpected two-element array allocation in LLVM trace"
    exit 1
fi

echo "PASS: two-arg Math.min/Math.max codegen"
