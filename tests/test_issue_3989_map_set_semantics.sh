#!/bin/bash
# Regression for #3989: Map/Set constructor iterable semantics and prototype shape.

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="$SCRIPT_DIR/../target/debug/perry"
[ ! -f "$PERRY" ] && PERRY="$SCRIPT_DIR/../target/release/perry"
if [ ! -f "$PERRY" ]; then
  echo "SKIP: perry binary not found (build with cargo build --bin perry)"
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

const mapClear = Object.getOwnPropertyDescriptor(Map.prototype, "clear");
check(!!mapClear && typeof mapClear.value === "function", "Map.prototype.clear descriptor");
check(mapClear.enumerable === false, "Map.prototype.clear non-enumerable");

const mapSize = Object.getOwnPropertyDescriptor(Map.prototype, "size");
check(!!mapSize && typeof mapSize.get === "function", "Map.prototype.size getter");
let mapSizeThrew = false;
try {
  mapSize.get.call({});
} catch (_e) {
  mapSizeThrew = true;
}
check(mapSizeThrew, "Map.prototype.size brand check");

const setSize = Object.getOwnPropertyDescriptor(Set.prototype, "size");
check(!!setSize && typeof setSize.get === "function", "Set.prototype.size getter");
let setSizeThrew = false;
try {
  setSize.get.call({});
} catch (_e) {
  setSizeThrew = true;
}
check(setSizeThrew, "Set.prototype.size brand check");

check(Object.getPrototypeOf(new Map()) === Map.prototype, "Map instance prototype");
check(Object.getPrototypeOf(new Set()) === Set.prototype, "Set instance prototype");
check(Map.prototype.constructor === Map, "Map prototype constructor");
check(Set.prototype.constructor === Set, "Set prototype constructor");
check(new Map([[0, 1]]).size === 1, "Map default constructor inserts");
check(new Set([0]).size === 1, "Set default constructor inserts");

const originalMapSet = Map.prototype.set;
let mapSetCalls = 0;
Map.prototype.set = function(_k: any, _v: any) {
  mapSetCalls = mapSetCalls + 1;
  return this;
};
const observedMap = new Map([[1, 2], [3, 4]]);
check(mapSetCalls === 2, "Map constructor calls overridden set");
check(observedMap.size === 0, "Map constructor uses observable set result path");

let mapClosed = false;
Map.prototype.set = function(_k: any, _v: any) {
  throw "map boom";
};
const mapIterable: any = {};
mapIterable[Symbol.iterator] = function() {
  return {
    next: function() {
      return { done: false, value: [1, 2] };
    },
    return: function() {
      mapClosed = true;
      return { done: true };
    }
  };
};
try {
  new Map(mapIterable);
} catch (_e) {}
check(mapClosed, "Map constructor closes iterator after set failure");
Map.prototype.set = originalMapSet;

const originalSetAdd = Set.prototype.add;
let setAddCalls = 0;
Set.prototype.add = function(_v: any) {
  setAddCalls = setAddCalls + 1;
  return this;
};
const observedSet = new Set([1, 2]);
check(setAddCalls === 2, "Set constructor calls overridden add");
check(observedSet.size === 0, "Set constructor uses observable add result path");

let setClosed = false;
Set.prototype.add = function(_v: any) {
  throw "set boom";
};
const setIterable: any = {};
setIterable[Symbol.iterator] = function() {
  return {
    next: function() {
      return { done: false, value: 1 };
    },
    return: function() {
      setClosed = true;
      return { done: true };
    }
  };
};
try {
  new Set(setIterable);
} catch (_e) {}
check(setClosed, "Set constructor closes iterator after add failure");
Set.prototype.add = originalSetAdd;

console.log("ok");
EOF

cd "$TMPDIR"
"$PERRY" compile main.ts --output test_bin >/dev/null
RUN_OUTPUT=$(./test_bin 2>&1)

if [ "$RUN_OUTPUT" = "ok" ]; then
  echo "PASS"
  exit 0
fi

echo "FAIL: unexpected output"
echo "$RUN_OUTPUT"
exit 1
