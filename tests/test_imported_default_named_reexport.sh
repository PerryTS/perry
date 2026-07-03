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

cat >"$TMPDIR/locales.js" <<'JS'
export const Code = { invalid_type: "invalid_type" };

const errorMap = (issue, ctx) => {
  let message;
  switch (issue.code) {
    case Code.invalid_type:
      message = `Expected ${issue.expected}, received ${issue.received}`;
      break;
    default:
      message = ctx.defaultError;
  }
  return { message };
};

export default errorMap;
JS

cat >"$TMPDIR/errors.js" <<'JS'
import defaultErrorMap from "./locales.js";
export { defaultErrorMap };
JS

cat >"$TMPDIR/index.js" <<'JS'
export * from "./errors.js";
JS

cat >"$TMPDIR/main.js" <<'JS'
import defaultMap from "./locales.js";
import { defaultErrorMap } from "./index.js";

const issue = { code: "invalid_type", expected: "string", received: "number" };
const ctx = { defaultError: "fallback" };
console.log(JSON.stringify({ direct: defaultMap(issue, ctx), reexport: defaultErrorMap(issue, ctx) }));
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

echo "PASS: imported default named re-export"
