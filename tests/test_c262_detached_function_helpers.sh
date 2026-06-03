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

cat >"$TMPDIR/c262_detached_function_helpers.js" <<'JS'
var failures = [];

function check(label, actual, expected) {
  if (actual !== expected) {
    failures.push(label + ": expected " + String(expected) + ", got " + String(actual));
  }
}

var __isArray = Array.isArray;
var __defineProperty = Object.defineProperty;
var __getOwnPropertyDescriptor = Object.getOwnPropertyDescriptor;
var __getOwnPropertyNames = Object.getOwnPropertyNames;
var __join = Function.prototype.call.bind(Array.prototype.join);
var __push = Function.prototype.call.bind(Array.prototype.push);
var __hasOwnProperty = Function.prototype.call.bind(Object.prototype.hasOwnProperty);
var __propertyIsEnumerable = Function.prototype.call.bind(Object.prototype.propertyIsEnumerable);

var obj = {};
__defineProperty(obj, "x", {
  value: 42,
  writable: false,
  enumerable: false,
  configurable: true
});

var desc = __getOwnPropertyDescriptor(obj, "x");
check("detached getOwnPropertyDescriptor value", desc.value, 42);
check("detached getOwnPropertyDescriptor writable", desc.writable, false);
check("detached getOwnPropertyDescriptor enumerable", desc.enumerable, false);
check("detached getOwnPropertyDescriptor configurable", desc.configurable, true);
check("detached hasOwnProperty", __hasOwnProperty(obj, "x"), true);
check("detached propertyIsEnumerable", __propertyIsEnumerable(obj, "x"), false);
check("detached Array.isArray", __isArray(__getOwnPropertyNames(obj)), true);

__push(failures, "sentinel");
check("detached Array.prototype.push", failures.length, 1);
check("detached Array.prototype.join", __join(failures, ","), "sentinel");
failures.length = 0;

var cls;
cls = class {};
var classNameDesc = __getOwnPropertyDescriptor(cls, "name");
check("class assignment inferred name", cls.name, "cls");
check("class dynamic name read", (function(obj, name) { return obj[name]; })(cls, "name"), "cls");
check("class name descriptor value", classNameDesc.value, "cls");
check("class name descriptor writable", classNameDesc.writable, false);
check("class name descriptor enumerable", classNameDesc.enumerable, false);
check("class name descriptor configurable", classNameDesc.configurable, true);
check("class name hasOwnProperty before delete", __hasOwnProperty(cls, "name"), true);
check("class name delete", delete cls["name"], true);
check("class name hasOwnProperty after delete", __hasOwnProperty(cls, "name"), false);
check("class name descriptor after delete", __getOwnPropertyDescriptor(cls, "name"), undefined);

if (failures.length !== 0) {
  throw new Error(__join(failures, "\n"));
}

console.log("c262 detached function helpers ok");
JS

"$PERRY" compile --no-cache --no-auto-optimize "$TMPDIR/c262_detached_function_helpers.js" -o "$TMPDIR/c262_detached_function_helpers" \
    >"$TMPDIR/compile.log" 2>&1 || {
        echo "FAIL: compile failed"
        sed 's/^/    /' "$TMPDIR/compile.log" | tail -80
        exit 1
    }

"$TMPDIR/c262_detached_function_helpers" >"$TMPDIR/run.log" 2>&1 || {
    echo "FAIL: program failed"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
}

if ! grep -q "c262 detached function helpers ok" "$TMPDIR/run.log"; then
    echo "FAIL: expected success marker"
    sed 's/^/    /' "$TMPDIR/run.log" | tail -80
    exit 1
fi

echo "PASS: c262 detached function helpers"
