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

cat >"$TMPDIR/effect_number_isfinite_property_path.ts" <<'TS'
let failures = 0;

function check(label: string, actual: any, expected: any): void {
  if (actual !== expected) {
    console.log(label + ": expected " + String(expected) + ", got " + String(actual));
    failures = failures + 1;
  }
}

check("direct finite", Number.isFinite(1), true);
check("direct infinity", Number.isFinite(Infinity), false);
check("direct string", Number.isFinite("1"), false);

check("global finite", globalThis.Number.isFinite(1), true);
check("global infinity", globalThis.Number.isFinite(Infinity), false);
check("global string", globalThis.Number.isFinite("1"), false);

if (failures !== 0) {
  throw new Error("Number.isFinite property-path parity failed");
}

console.log("effect Number.isFinite property path ok");
TS

"$PERRY" compile --no-auto-optimize "$TMPDIR/effect_number_isfinite_property_path.ts" \
    -o "$TMPDIR/effect_number_isfinite_property_path" >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/effect_number_isfinite_property_path" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "effect Number.isFinite property path ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: effect Number.isFinite property path"
