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

cat >"$TMPDIR/main.js" <<'JS'
class FixtureError extends Error {
  constructor() {
    super("boom");
    this.name = "FixtureError";
  }
}

const error = new FixtureError();
const aggregate = new AggregateError([1, 2], "many");
console.log(JSON.stringify({
  own: Object.prototype.hasOwnProperty.call(error, "errors"),
  inValue: "errors" in error,
  type: typeof error.errors,
  undef: error.errors === undefined,
  aggregateLength: aggregate.errors.length,
  aggregateFirst: aggregate.errors[0],
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

echo "PASS: missing Error.errors property is undefined"
