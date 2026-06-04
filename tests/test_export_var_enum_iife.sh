#!/usr/bin/env bash
# Regression: TypeScript-emitted JS enums use `export var X;` followed by an
# IIFE that mutates X. The no-init exported var still needs a cross-module
# getter, including through a barrel re-export.

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

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/trace-flags.js" <<'JS'
export var TraceFlags;
(function (TraceFlags) {
  TraceFlags[TraceFlags["NONE"] = 0] = "NONE";
  TraceFlags[TraceFlags["SAMPLED"] = 1] = "SAMPLED";
})(TraceFlags || (TraceFlags = {}));
JS

cat >"$TMPDIR/index.js" <<'JS'
export { TraceFlags } from "./trace-flags.js";
JS

cat >"$TMPDIR/main.ts" <<'TS'
import { TraceFlags as Direct } from "./trace-flags.js";
import { TraceFlags as Barrel } from "./index.js";

console.log(Direct.NONE);
console.log(Barrel.SAMPLED);
TS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-auto-optimize \
    "$TMPDIR/main.ts" -o "$TMPDIR/export-var-enum-iife" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -ne 0 ]]; then
    echo "FAIL: export-var enum IIFE failed to compile/link"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -120
    exit 1
fi

output="$("$TMPDIR/export-var-enum-iife")"
if [[ "$output" != $'0\n1' ]]; then
    echo "FAIL: expected enum values 0 and 1, got:"
    printf '%s\n' "$output" | sed 's/^/    /'
    exit 1
fi

echo "PASS"
