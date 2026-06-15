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

cat >"$TMPDIR/math_existing_api_parity.js" <<'JS'
var failures = [];

function check(label, condition) {
  if (!condition) {
    failures.push(label);
  }
}

function isNaNValue(value) {
  return value !== value;
}

function isNegZero(value) {
  return value === 0 && 1 / value === -Infinity;
}

var signFoo = Math.sign("foo");
check("Math.sign string NaN", isNaNValue(signFoo));

var signUndefined = Math.sign(undefined);
check("Math.sign undefined NaN", isNaNValue(signUndefined));

var signNegZero = Math.sign(-0);
check("Math.sign preserves -0", isNegZero(signNegZero));

var truncCount = 0;
function truncArg() {
  truncCount = truncCount + 1;
  return 1.75;
}
var truncValue = Math.trunc(truncArg());
check("Math.trunc value", truncValue === 1);
check("Math.trunc evaluates once", truncCount === 1);

var signCount = 0;
function signArg() {
  signCount = signCount + 1;
  return -0;
}
var signValue = Math.sign(signArg());
check("Math.sign value", isNegZero(signValue));
check("Math.sign evaluates once", signCount === 1);

var signNaNCount = 0;
function signNaNArg() {
  signNaNCount = signNaNCount + 1;
  return "foo";
}
var signNaNValue = Math.sign(signNaNArg());
check("Math.sign coerces to NaN", isNaNValue(signNaNValue));
check("Math.sign NaN evaluates once", signNaNCount === 1);

check("Math.imul NaN", Math.imul(NaN, 5) === 0);
check("Math.imul infinity", Math.imul(Infinity, 5) === 0);
check("Math.imul wrap negative one", Math.imul(4294967295, 5) === -5);
check("Math.imul wrap int max", Math.imul(2147483647, 2) === -2);

var detachedAbs = Math.abs;
var absThrewTypeError = false;
try {
  detachedAbs(1n);
} catch (error) {
  absThrewTypeError = error instanceof TypeError;
}
check("detached Math.abs BigInt TypeError", absThrewTypeError);

var seen = "";
var detachedMin = Math.min;
var minResult = detachedMin(
  { valueOf: function () { seen = seen + "a"; return NaN; } },
  { valueOf: function () { seen = seen + "b"; return 1; } }
);
check("detached Math.min NaN", isNaNValue(minResult));
check("detached Math.min coerces later args", seen === "ab");

seen = "";
var detachedMax = Math.max;
var maxResult = detachedMax(
  { valueOf: function () { seen = seen + "a"; return NaN; } },
  { valueOf: function () { seen = seen + "b"; return 1; } }
);
check("detached Math.max NaN", isNaNValue(maxResult));
check("detached Math.max coerces later args", seen === "ab");

if (failures.length !== 0) {
  throw new Error(failures.join("\n"));
}

console.log("math existing api parity ok");
JS

(
    cd "$TMPDIR"
    "$PERRY" compile --no-cache --no-auto-optimize --trace llvm \
        math_existing_api_parity.js -o math_existing_api_parity \
        >compile.log 2>&1
) || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
}

"$TMPDIR/math_existing_api_parity" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -100
    exit 1
}

EXPECTED="math existing api parity ok"
ACTUAL="$(cat "$TMPDIR/run.log")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected:"
    printf '%s\n' "$EXPECTED" | sed 's/^/    /'
    echo "actual:"
    printf '%s\n' "$ACTUAL" | sed 's/^/    /'
    exit 1
fi

TRACE_DIR="$TMPDIR/.perry-trace/llvm"
if [[ ! -d "$TRACE_DIR" ]]; then
    echo "FAIL: LLVM trace directory was not produced"
    exit 1
fi

for helper in js_math_sign js_math_trunc js_math_imul; do
    if ! grep -R "call double @$helper" "$TRACE_DIR" >/dev/null; then
        echo "FAIL: expected LLVM trace to call $helper"
        exit 1
    fi
done

echo "PASS: existing Math API parity"
