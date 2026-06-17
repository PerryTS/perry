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

cat >"$TMPDIR/c262_object_numeric_index_assignment.js" <<'JS'
var failures = [];

function check(label, actual, expected) {
  if (actual !== expected) {
    failures.push(label + ": expected " + String(expected) + ", got " + String(actual));
  }
}

var map = {};
map[1] = "one";
map["two"] = 2;
map["3"] = "tre";

check("numeric write creates property readable by numeric key", map[1], "one");
check("numeric write creates string property", map["1"], "one");
check("string write remains readable", map["two"], 2);
check("numeric string write remains readable", map[3], "tre");

var existing = {1: "one", two: 2};
existing[1] = "uno";
check("numeric write overwrites existing numeric literal key", existing[1], "uno");
check("numeric write overwrites existing string key", existing["1"], "uno");

existing["1"] = 1;
check("string numeric write overwrites same key", existing[1], 1);

existing["two"] = "two";
check("computed string write overwrites existing property", existing.two, "two");

existing.two = "duo";
check("dot write still overwrites existing property", existing["two"], "duo");

if (failures.length !== 0) {
  throw new Error(failures.join("\n"));
}

console.log("PASS c262 object numeric index assignment");
JS

"$PERRY" compile --no-cache "$TMPDIR/c262_object_numeric_index_assignment.js" -o "$TMPDIR/c262_object_numeric_index_assignment" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/c262_object_numeric_index_assignment" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

EXPECTED="PASS c262 object numeric index assignment"
RUN_OUTPUT="$(cat "$TMPDIR/run.log")"
if [[ "$RUN_OUTPUT" != "$EXPECTED" ]]; then
    echo "FAIL: c262 object numeric index assignment output mismatch"
    echo "Expected:"
    echo "$EXPECTED"
    echo ""
    echo "Got:"
    echo "$RUN_OUTPUT"
    exit 1
fi

echo "PASS: c262 object numeric index assignment"
