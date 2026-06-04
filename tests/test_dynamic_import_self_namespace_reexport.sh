#!/usr/bin/env bash
# Regression: `export * as Name from "."` on a dynamic-import target must
# resolve "." to the current index module instead of emitting @__perry_ns__.

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

mkdir -p "$TMPDIR/mod"

cat >"$TMPDIR/mod/index.ts" <<'TS'
export const value = 1
export * as Self from "."
TS

cat >"$TMPDIR/main.ts" <<'TS'
const ns = await import("./mod")
console.log(ns.value)
TS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-link --no-auto-optimize \
    "$TMPDIR/main.ts" -o "$TMPDIR/self-ns" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -ne 0 ]]; then
    echo "FAIL: self namespace re-export dynamic import failed to compile"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

if grep -q "@__perry_ns__" "$TMPDIR/compile.log"; then
    echo "FAIL: emitted unresolved bare namespace global"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

echo "PASS"
