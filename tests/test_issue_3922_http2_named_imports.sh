#!/usr/bin/env bash
# Regression for #3922: invalid node:http2 named imports must fail during
# import validation, before a compiled program can execute.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"

if [[ ! -x "$PERRY" ]]; then
    PERRY="$REPO_ROOT/target/debug/perry"
fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build --release -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

MARKER="$TMPDIR/executed"
cat >"$TMPDIR/invalid-http2-imports.ts" <<TS
import { Http2SecureServer, listen, close, on, address } from "node:http2";
import { writeFileSync } from "node:fs";
import { env } from "node:process";

const marker = env.ISSUE_3922_MARKER;
if (marker === undefined) {
    throw new Error("missing ISSUE_3922_MARKER");
}
writeFileSync(marker, "executed");
console.log(Http2SecureServer, listen, close, on, address);
TS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 ISSUE_3922_MARKER="$MARKER" "$PERRY" compile --no-cache \
    "$TMPDIR/invalid-http2-imports.ts" -o "$TMPDIR/invalid-http2-imports" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -eq 0 ]]; then
    echo "FAIL: invalid node:http2 named imports compiled"
    exit 1
fi

if [[ -e "$MARKER" ]]; then
    echo "FAIL: invalid node:http2 named import fixture executed"
    exit 1
fi

if ! grep -q "does not provide an export named" "$TMPDIR/compile.log"; then
    echo "FAIL: compile failed for the wrong reason"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
fi

echo "PASS"
