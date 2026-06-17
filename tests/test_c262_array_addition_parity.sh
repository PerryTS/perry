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

cat >"$TMPDIR/c262_array_addition_parity.js" <<'JS'
var failures = [];

function check(label, actual, expected) {
  if (actual !== expected) {
    failures.push(label + ": expected " + String(expected) + ", got " + String(actual));
  }
}

function checkThrows(label, ctor, fn) {
  try {
    fn();
    failures.push(label + ": expected " + ctor.name);
  } catch (e) {
    if (!(e instanceof ctor) && e.name !== ctor.name) {
      failures.push(label + ": expected " + ctor.name + ", got " + String(e && e.name));
    }
  }
}

function MyError() {}

var array = [[1, 2], [3], []];
check("array typeof", typeof array, "object");
check("array instanceof", array instanceof Array, true);
check("array toString property", array.toString, Array.prototype.toString);
check("array length", array.length, 3);

var subarray = array[0];
check("subarray 0 typeof", typeof subarray, "object");
check("subarray 0 instanceof", subarray instanceof Array, true);
check("subarray 0 toString property", subarray.toString, Array.prototype.toString);
check("subarray 0 length", subarray.length, 2);
check("subarray 0 value 0", subarray[0], 1);
check("subarray 0 value 1", subarray[1], 2);

subarray = array[1];
check("subarray 1 typeof", typeof subarray, "object");
check("subarray 1 instanceof", subarray instanceof Array, true);
check("subarray 1 toString property", subarray.toString, Array.prototype.toString);
check("subarray 1 length", subarray.length, 1);
check("subarray 1 value 0", subarray[0], 3);

subarray = array[2];
check("subarray 2 typeof", typeof subarray, "object");
check("subarray 2 instanceof", subarray instanceof Array, true);
check("subarray 2 toString property", subarray.toString, Array.prototype.toString);
check("subarray 2 length", subarray.length, 0);
check("nested value 0/0", array[0][0], 1);
check("nested value 0/1", array[0][1], 2);
check("nested value 1/0", array[1][0], 3);

check("assignment expression result participates in following GetValue",
  (c262_parity_eval_order_y = 1) + c262_parity_eval_order_y, 2);

checkThrows("unresolvable lhs is read before sloppy rhs assignment", ReferenceError, function() {
  c262_parity_unbound_x + (c262_parity_unbound_x = 1);
});
checkThrows("computed assignment target reads before later sloppy assignment", ReferenceError, function() {
  var o = {};
  (o[c262_parity_member_x] = 1) + (c262_parity_member_x = 2);
});

var trace = "";
checkThrows("rhs ToPrimitive before lhs ToNumeric", MyError, function() {
  (function() {
    trace += "1";
    return {
      valueOf: function() {
        trace += "3";
        return Symbol("1");
      }
    };
  })() + (function() {
    trace += "2";
    return {
      valueOf: function() {
        trace += "4";
        throw new MyError();
      }
    };
  })();
});
check("rhs ToPrimitive trace", trace, "1234");

trace = "";
checkThrows("symbol addition throws after both ToPrimitive calls", TypeError, function() {
  (function() {
    trace += "1";
    return {
      valueOf: function() {
        trace += "3";
        return 1;
      }
    };
  })() + (function() {
    trace += "2";
    return {
      valueOf: function() {
        trace += "4";
        return Symbol("1");
      }
    };
  })();
});
check("symbol TypeError trace", trace, "1234");

check("object plus function default stringification",
  ({} + function(){return 1}),
  ({}.toString() + function(){return 1}.toString()));
check("function plus object default stringification",
  (function(){return 1} + {}),
  (function(){return 1}.toString() + {}.toString()));
check("function plus function default stringification",
  (function(){return 1} + function(){return 1}),
  (function(){return 1}.toString() + function(){return 1}.toString()));
check("object plus object default stringification",
  ({} + {}),
  ({}.toString() + {}.toString()));

if (failures.length !== 0) {
  throw new Error(failures.join("\n"));
}

console.log("PASS c262 array addition parity");
JS

"$PERRY" compile --no-cache "$TMPDIR/c262_array_addition_parity.js" -o "$TMPDIR/c262_array_addition_parity" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/c262_array_addition_parity" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

EXPECTED="PASS c262 array addition parity"
RUN_OUTPUT="$(cat "$TMPDIR/run.log")"
if [[ "$RUN_OUTPUT" != "$EXPECTED" ]]; then
    echo "FAIL: c262 array/addition parity fixture output mismatch"
    echo "Expected:"
    echo "$EXPECTED"
    echo ""
    echo "Got:"
    echo "$RUN_OUTPUT"
    exit 1
fi

echo "PASS: c262 array/addition parity"
