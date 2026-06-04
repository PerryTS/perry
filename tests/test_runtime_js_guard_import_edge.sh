#!/usr/bin/env bash
# Regression: when a native source module imports an untrusted JS package,
# Perry's V8-free guard must name the native import edge that introduced it.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi
if [[ "$PERRY" != /* ]]; then
    PERRY="$REPO_ROOT/$PERRY"
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$TMPDIR/local" "$TMPDIR/node_modules/untrusted-js"

cat >"$TMPDIR/package.json" <<'JSON'
{
  "type": "module",
  "perry": {
    "packageAliases": {
      "@local/pkg": "./local/index.ts",
      "@local/type-only": "./local/type-only.ts"
    }
  }
}
JSON

cat >"$TMPDIR/entry.ts" <<'TS'
import { value } from "@local/pkg"
console.log(value)
TS

cat >"$TMPDIR/local/index.ts" <<'TS'
import { value } from "untrusted-js"
export { value }
TS

cat >"$TMPDIR/type-entry.ts" <<'TS'
import { value } from "@local/type-only"
console.log(value)
TS

cat >"$TMPDIR/local/type-only.ts" <<'TS'
import { type Value } from "untrusted-js"

export const value = 42
TS

cat >"$TMPDIR/node_modules/untrusted-js/package.json" <<'JSON'
{
  "name": "untrusted-js",
  "type": "module",
  "main": "index.js"
}
JSON

cat >"$TMPDIR/node_modules/untrusted-js/index.js" <<'JS'
export const value = 42
JS

set +e
(
    cd "$TMPDIR"
    env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-auto-optimize \
        type-entry.ts -o "$TMPDIR/type-only-ok"
) >"$TMPDIR/type-only.log" 2>&1
type_only_rc=$?
set -e

if [[ "$type_only_rc" -ne 0 ]]; then
    echo "FAIL: type-only JS package import failed to compile"
    sed 's/^/    /' "$TMPDIR/type-only.log" | tail -100
    exit 1
fi

output="$("$TMPDIR/type-only-ok")"
if [[ "$output" != "42" ]]; then
    echo "FAIL: expected type-only import case to print 42, got: $output"
    exit 1
fi

set +e
(
    cd "$TMPDIR"
    env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-auto-optimize \
        entry.ts -o "$TMPDIR/runtime-js-guard"
) >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -eq 0 ]]; then
    echo "FAIL: untrusted JS package unexpectedly compiled"
    exit 1
fi

if ! grep -q "JavaScript runtime (V8) support has been removed" "$TMPDIR/compile.log"; then
    echo "FAIL: expected V8-free runtime JS guard"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

if ! grep -q "imported by .*local/index.ts via \`untrusted-js\`" "$TMPDIR/compile.log"; then
    echo "FAIL: missing native import edge in runtime JS diagnostic"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

echo "PASS"
