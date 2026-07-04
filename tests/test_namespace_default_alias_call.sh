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

mkdir -p "$TMPDIR/locales"
cat >"$TMPDIR/locales/es.ts" <<'TS'
function build() {
  return () => "locale-es";
}
export default function () {
  return { localeError: build() };
}
TS
cat >"$TMPDIR/locales/en.ts" <<'TS'
export default function () {
  return { localeError: () => "locale-en" };
}
TS
cat >"$TMPDIR/locales/index.ts" <<'TS'
export { default as en } from "./en.js";
export { default as es } from "./es.js";
TS
cat >"$TMPDIR/defaultVar.ts" <<'TS'
const defaultValue = () => "default-var";
export default defaultValue;
TS
cat >"$TMPDIR/main.ts" <<'TS'
import defaultValue from "./defaultVar.js";
import * as locales from "./locales/index.js";
console.log(JSON.stringify({ defaultValue: defaultValue(), en: locales.en().localeError({}), es: locales.es().localeError({}) }));
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
EXPECTED='{"defaultValue":"default-var","en":"locale-en","es":"locale-es"}'
if [[ "$OUT" != "$EXPECTED" ]]; then
    echo "FAIL: unexpected output"
    echo "expected: $EXPECTED"
    echo "actual:   $OUT"
    exit 1
fi

echo "PASS: namespace default alias call"
