#!/usr/bin/env bash
# Regression: namespace-reexported var closures with many args must not hit
# the fixed js_closure_call0..16 cap. OpenCode's AppLayer does this via
# `import { Layer } from "effect"; Layer.mergeAll(...52 layers)`.

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

cat >"$TMPDIR/layer.ts" <<'TS'
export const mergeAll = (...items: number[]) => items.length
TS

cat >"$TMPDIR/index.ts" <<'TS'
export * as Layer from "./layer"
TS

cat >"$TMPDIR/main.ts" <<'TS'
import { Layer } from "./index"

export const count = Layer.mergeAll(
  1, 2, 3, 4, 5, 6, 7, 8,
  9, 10, 11, 12, 13, 14, 15, 16,
  17, 18, 19, 20, 21, 22, 23, 24,
)
TS

set +e
env PERRY_NO_AUTO_OPTIMIZE=1 "$PERRY" compile --no-link --no-auto-optimize \
    "$TMPDIR/main.ts" -o "$TMPDIR/many-args" \
    >"$TMPDIR/compile.log" 2>&1
compile_rc=$?
set -e

if [[ "$compile_rc" -ne 0 ]]; then
    echo "FAIL: namespace var closure call with many args failed to compile"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

if grep -q "closure call with .*args (max 16)" "$TMPDIR/compile.log"; then
    echo "FAIL: many-arg namespace var closure call hit fixed closure cap"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -100
    exit 1
fi

echo "PASS"
