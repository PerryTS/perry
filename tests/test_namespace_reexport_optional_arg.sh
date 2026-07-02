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

cat >"$TMPDIR/core.ts" <<'TS'
export function _make(params?: any) {
  return { check: "string_format", format: "lowercase", ...(!params ? {} : params) };
}
TS
cat >"$TMPDIR/external.ts" <<'TS'
export { _make as make } from "./core.js";
export function string() {
  return { format: null, parse() {} };
}
TS
cat >"$TMPDIR/main.ts" <<'TS'
import * as z from "./external.js";
const schema = z.string();
const check = z.make();
console.log(JSON.stringify({ schemaFormat: schema.format, checkKeys: Object.keys(check), checkFormat: check.format, hasParse: "parse" in check }));
TS

BIN="$TMPDIR/main"
COMPILE_OUT="$($PERRY compile --no-cache --no-auto-optimize "$TMPDIR/main.ts" -o "$BIN" 2>&1)" || {
    echo "FAIL: perry compile errored"
    echo "$COMPILE_OUT"
    exit 1
}
OUT="$($BIN 2>&1)" || {
    echo "FAIL: compiled program errored"
    echo "$OUT"
    exit 1
}
EXPECTED='{"schemaFormat":null,"checkKeys":["check","format"],"checkFormat":"lowercase","hasParse":false}'
if [[ "$OUT" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected: $EXPECTED"
    echo "actual:   $OUT"
    exit 1
fi

echo "PASS: namespace re-export optional arg"
