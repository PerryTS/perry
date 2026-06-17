#!/bin/bash
# Regression: destructuring assignment must match iterator-close and
# target-evaluation order from Test262's assignment/destructuring bucket.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="${PERRY_BIN:-$SCRIPT_DIR/../target/release/perry}"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/debug/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --release)"
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
check(arrayTargetObject.slot === 42, "array destructuring assignment value");
check(
  arrayOrder === "source,iterator,target,target-key,next,target-key-tostring,return,",
  "array destructuring assignment order: " + arrayOrder
);

let objectOrder = "";
const objectSourceObject: any = { prop: 7 };
const objectSourceKey: any = {
  toString: function() {
    objectOrder += "source-key-tostring,";
    return "prop";
  }
};
const objectTargetObject: any = {};
const objectTargetKey: any = {
  toString: function() {
    objectOrder += "target-key-tostring,";
    return "slot";
  }
};
function objectSource(): any {
  objectOrder += "source,";
  return objectSourceObject;
}
function objectKey(): any {
  objectOrder += "source-key,";
  return objectSourceKey;
}
function objectTarget(): any {
  objectOrder += "target,";
  return objectTargetObject;
}
function objectTargetProperty(): any {
  objectOrder += "target-key,";
  return objectTargetKey;
}
({ [objectKey()]: objectTarget()[objectTargetProperty()] } = objectSource());
check(objectTargetObject.slot === 7, "object destructuring assignment value");
check(
  objectOrder === "source,source-key,source-key-tostring,target,target-key,target-key-tostring,",
  "object destructuring assignment order: " + objectOrder
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
check(setterReceiver.recorded === 11, "object destructuring member target setter value");
check(
  setterOrder === "target,target-key,target-key-tostring,setter,",
  "object destructuring member target setter order: " + setterOrder
);

let closeCount = 0;
let caught = "";
const abruptIterable: any = {};
abruptIterable[Symbol.iterator] = function() {
  return {
    next: function() {
      return { done: false, value: undefined };
    },
    return: function() {
      closeCount = closeCount + 1;
      throw "close";
    }
  };
};
function defaultThrow(): any {
  throw "default";
}
try {
  let value: any = null;
  [value = defaultThrow()] = abruptIterable;
} catch (e: any) {
  caught = e;
}
check(closeCount === 1, "iterator return called after abrupt default");
check(caught === "default", "original abrupt completion preserved: " + caught);

console.log("ok");
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null
set +e
RUN_OUTPUT=$(./test_bin 2>&1)
RUN_STATUS=$?
set -e

if [ "$RUN_STATUS" -eq 0 ] && [ "$RUN_OUTPUT" = "ok" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: unexpected output"
echo "Status: $RUN_STATUS"
echo "$RUN_OUTPUT"
exit 1
