#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/release/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cat > "$TMPDIR/main.js" << 'EOF'
var failures = "";

function check(condition, label) {
  if (!condition) {
    failures += label + "\n";
  }
}

x = 1;
check(x === 1, "sloppy simple assignment creates backing storage");

var y = { bre\u0061k: x } = { break: 42 };
check(x === 42 && y.break === 42, "escaped reserved destructuring name");

var count = 0;
var caught = false;
try {
  (null).prop = count += 1;
} catch (e) {
  caught = e instanceof TypeError;
}
check(caught && count === 1, "null property assignment evaluates rhs then throws");

count = 0;
caught = false;
try {
  (undefined)["prop"] = count += 1;
} catch (e) {
  caught = e instanceof TypeError;
}
check(caught && count === 1, "undefined computed assignment evaluates rhs then throws");

function computedKey() {
  count += 10;
  return "prop";
}

function computedThrow() {
  count += 10;
  throw new Error("key");
}

class Base {}
class Derived extends Base {
  static setSuperIdentifier() {
    super.prop = count += 1;
  }

  static setSuperComputed() {
    super[computedKey()] = count += 1;
  }

  static setSuperComputedThrows() {
    super[computedThrow()] = count += 1;
  }
}

count = 0;
caught = false;
try {
  Derived.setSuperIdentifier();
} catch (e) {
  caught = e instanceof TypeError;
}
check(caught && count === 1, "super identifier assignment evaluates rhs then throws");

count = 0;
caught = false;
try {
  Derived.setSuperComputed();
} catch (e) {
  caught = e instanceof TypeError;
}
check(caught && count === 11, "super computed assignment evaluates key then rhs");

count = 0;
caught = false;
try {
  Derived.setSuperComputedThrows();
} catch (e) {
  caught = e instanceof Error && !(e instanceof TypeError);
}
check(caught && count === 10, "super computed assignment stops when key throws");

if (failures.length !== 0) {
  throw new Error(failures);
}

console.log("PASS c262 assignment parity");
EOF

cd "$TMPDIR"
"$PERRY" compile main.js --output test_bin --no-cache >/dev/null 2>&1
RUN_OUTPUT=$(./test_bin 2>&1)

EXPECTED="PASS c262 assignment parity"
if [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: c262 assignment parity fixture output mismatch"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
