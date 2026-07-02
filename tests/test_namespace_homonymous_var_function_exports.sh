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

cat >"$TMPDIR/function-source.ts" <<'TS'
export function string() {
  return "function-export";
}
TS
cat >"$TMPDIR/var-source.ts" <<'TS'
const stringType = () => "var-export";
export { stringType as string };
TS
cat >"$TMPDIR/main.ts" <<'TS'
import * as current from "./function-source.js";
import * as legacy from "./var-source.js";

const out = {
  currentType: typeof current.string,
  legacyType: typeof legacy.string,
  currentCall: current.string(),
  legacyCall: legacy.string(),
};
console.log(JSON.stringify(out));
TS

EXPECTED='{"currentType":"function","legacyType":"function","currentCall":"function-export","legacyCall":"var-export"}'
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

echo "PASS: namespace homonymous var/function exports"
