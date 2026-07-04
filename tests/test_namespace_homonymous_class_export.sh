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

cat >"$TMPDIR/ranges.ts" <<'TS'
export const NUMBER_FORMAT_RANGES = { safeint: [-1, 1] };
export function normalizeParams(value?: unknown) {
  return value ?? {};
}
TS
cat >"$TMPDIR/legacy.ts" <<'TS'
export class util {
  static objectKeys(value: object) {
    return Object.keys(value);
  }
}
TS
cat >"$TMPDIR/checks.ts" <<'TS'
import * as util from "./ranges.js";
import { util as legacyUtil } from "./legacy.js";

export function readRange() {
  return {
    keys: Object.keys(util).sort(),
    range: util.NUMBER_FORMAT_RANGES.safeint,
    normalized: util.normalizeParams(),
    legacyType: typeof legacyUtil,
    legacyKeys: legacyUtil.objectKeys({ a: 1, b: 2 }),
  };
}
TS
cat >"$TMPDIR/main.ts" <<'TS'
import { readRange } from "./checks.js";
console.log(JSON.stringify(readRange()));
TS

EXPECTED='{"keys":["NUMBER_FORMAT_RANGES","normalizeParams"],"range":[-1,1],"normalized":{},"legacyType":"function","legacyKeys":["a","b"]}'
COMPILE_OUT="$(PERRY_ALLOW_PERRY_FEATURES=1 PERRY_RUNTIME_DIR="$REPO_ROOT/target/debug" \
    "$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/main.ts" -o "$TMPDIR/out" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}

OUT="$($TMPDIR/out 2>&1)" || {
    echo "FAIL: compiled binary errored"
    echo "$OUT"
    exit 1
}

if [[ "$OUT" != "$EXPECTED" ]]; then
    echo "FAIL: expected $EXPECTED, got:"
    echo "$OUT"
    exit 1
fi

echo "PASS: namespace homonymous class export"
