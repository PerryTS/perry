#!/bin/bash
# Regression: computed assignment keys and destructuring member targets must
# preserve Test262/ECMA-262 evaluation order.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$SCRIPT_DIR/.."
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
[ ! -f "$PERRY" ] && PERRY="$REPO_ROOT/target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build -p perry)"
  exit 0
fi

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

cat > "$TMPDIR/main.ts" << 'EOF'
function check(value: boolean, label: string) {
  if (!value) {
    throw label;
  }
}

let computedOrder = "";
const computedReceiver: any = {};
const computedKeyObject: any = {
  toString: function() {
    computedOrder += "key-tostring,";
    return "slot";
  }
};
function computedBase(): any {
  computedOrder += "base,";
  return computedReceiver;
}
function computedKeyValue(): any {
  computedOrder += "key,";
  return computedKeyObject;
}
function computedRhs(): any {
  computedOrder += "rhs,";
  return 99;
}
const computedResult = computedBase()[computedKeyValue()] = computedRhs();
check(computedResult === 99, "computed assignment expression result");
check(computedReceiver.slot === 99, "computed assignment stored value");
check(
  computedOrder === "base,key,rhs,key-tostring,",
  "computed assignment order: " + computedOrder
);

let arrayOrder = "";
const arrayIterable: any = {};
arrayIterable[Symbol.iterator] = function() {
  arrayOrder += "iterator,";
  return {
    next: function() {
      arrayOrder += "next,";
      return { done: false, value: 42 };
    },
    return: function() {
      arrayOrder += "return,";
      return { done: true };
    }
  };
};
const arrayTargetObject: any = {};
const arrayTargetKey: any = {
  toString: function() {
    arrayOrder += "target-key-tostring,";
    return "slot";
  }
};
function arraySource(): any {
  arrayOrder += "source,";
  return arrayIterable;
}
function arrayTarget(): any {
  arrayOrder += "target,";
  return arrayTargetObject;
}
function arrayKey(): any {
  arrayOrder += "target-key,";
  return arrayTargetKey;
}
[arrayTarget()[arrayKey()]] = arraySource();
check(arrayTargetObject.slot === 42, "array destructuring member target value");
check(
  arrayOrder === "source,iterator,target,target-key,next,target-key-tostring,return,",
  "array destructuring member target order: " + arrayOrder
);

let setterOrder = "";
const setterProto: any = {};
Object.defineProperty(setterProto, "slot", {
  set: function(v: any) {
    setterOrder += "setter,";
    this.recorded = v;
  }
});
const setterReceiver: any = Object.create(setterProto);
const setterKey: any = {
  toString: function() {
    setterOrder += "target-key-tostring,";
    return "slot";
  }
};
function setterTarget(): any {
  setterOrder += "target,";
  return setterReceiver;
}
function setterTargetKey(): any {
  setterOrder += "target-key,";
  return setterKey;
}
({ prop: setterTarget()[setterTargetKey()] } = { prop: 11 });
check(setterReceiver.recorded === 11, "object destructuring inherited setter value");
check(
  setterOrder === "target,target-key,target-key-tostring,setter,",
  "object destructuring inherited setter order: " + setterOrder
);

console.log("PASS c262 assignment eval order");
EOF

cd "$TMPDIR"
env PERRY_ALLOW_UNIMPLEMENTED=1 PERRY_NO_AUTO_OPTIMIZE=1 \
  "$PERRY" compile --no-cache main.ts --output test_bin >/dev/null
set +e
RUN_OUTPUT=$(./test_bin 2>&1)
RUN_STATUS=$?
set -e

EXPECTED="PASS c262 assignment eval order"
if [ "$RUN_STATUS" -eq 0 ] && [ "$RUN_OUTPUT" = "$EXPECTED" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: c262 assignment eval order fixture output mismatch"
echo "Status: $RUN_STATUS"
echo "Expected:"
echo "$EXPECTED"
echo ""
echo "Got:"
echo "$RUN_OUTPUT"
exit 1
