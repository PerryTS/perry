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

cat >"$TMPDIR/external.js" <<'JS'
export const util = {
  find(arr, checker) {
    for (const item of arr) {
      if (checker(item)) return item;
    }
    return undefined;
  },
};
JS

cat >"$TMPDIR/index.js" <<'JS'
import * as z from "./external.js";
export * from "./external.js";
export { z };
export default z;
JS

cat >"$TMPDIR/main.js" <<'JS'
import { z } from "./index.js";

console.log(JSON.stringify({
  type: typeof z.util.find,
  found: z.util.find([1, 2, 3], (value) => value > 1),
}));
JS

if ! command -v node >/dev/null 2>&1; then
    echo "SKIP: node binary not found"
    exit 0
fi
node "$TMPDIR/main.js" >"$TMPDIR/node.txt"
"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/main.js" \
    -o "$TMPDIR/main" >"$TMPDIR/compile.log" 2>&1 || {
    echo "FAIL: compile failed"
    sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
    exit 1
}
"$TMPDIR/main" >"$TMPDIR/perry.txt" 2>"$TMPDIR/run.log" || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! diff -u "$TMPDIR/node.txt" "$TMPDIR/perry.txt" >"$TMPDIR/diff.log"; then
    echo "FAIL: output mismatch"
    sed 's/^/    /' "$TMPDIR/diff.log"
    exit 1
fi

echo "PASS: namespace subobject method call"
