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

cat >"$TMPDIR/c262_with_parity.js" <<'JS'
var failures = 0;

function check(label, condition) {
  if (!condition) {
    console.log(label);
    failures++;
  }
}

function makeWithReader() {
  var a = { a: 10 };
  with (a) {
    return () => a;
  }
}

check("arrow captures with object environment", makeWithReader()() === 10);

var count = 0;
var scope = { x: 1 };
with (scope) {
  (function() {
    "use strict";
    var caught = false;
    try {
      count++;
      x = (delete scope.x, 2);
      count++;
    } catch (e) {
      caught = e instanceof ReferenceError;
    }
    check("strict with assignment throws ReferenceError after binding deletion", caught);
    count++;
  })();
}

check("strict with assignment evaluates RHS once and resumes", count === 2);
check("strict with assignment leaves deleted property absent", !("x" in scope));

if (failures !== 0) {
  throw new Error("failures: " + failures);
}
JS

pushd "$TMPDIR" >/dev/null
if ! "$PERRY" compile c262_with_parity.js --output c262_with_parity --no-cache >compile.log 2>&1; then
  echo "FAIL: compile failed"
  sed 's/^/    /' compile.log
  exit 1
fi
./c262_with_parity
popd >/dev/null

echo "PASS c262 with parity"
