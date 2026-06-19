#!/usr/bin/env bash
set -euo pipefail

# `Object` is a function, so reading a non-static member resolves up its
# prototype chain — `Object.hasOwnProperty` IS `Object.prototype.hasOwnProperty`,
# a callable. immer's `O.hasOwnProperty.call(proto, "constructor")` (with
# `const O = Object`) depends on this. Before the fix, reading `hasOwnProperty`
# (and `isPrototypeOf` / `propertyIsEnumerable` / `toString` / `valueOf` /
# `toLocaleString`) off the reified `Object` constructor value returned
# `undefined`, so `.call(...)` threw "Function.prototype.call was called on a
# value that is not a function".

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PERRY="${PERRY_BIN:-${PERRY:-$REPO_ROOT/target/release/perry}}"
if [[ ! -x "$PERRY" ]]; then PERRY="$REPO_ROOT/target/debug/perry"; fi
if [[ ! -x "$PERRY" ]]; then
    echo "SKIP: perry binary not found (build with cargo build -p perry)"
    exit 0
fi

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

cat >"$TMPDIR/f.ts" <<'TS'
const O: any = Object;
if (typeof O.hasOwnProperty !== "function") throw new Error("hasOwnProperty not a function");
if (typeof O.isPrototypeOf !== "function") throw new Error("isPrototypeOf not a function");
if (typeof O.propertyIsEnumerable !== "function") throw new Error("propertyIsEnumerable not a function");
const proto = O.getPrototypeOf({ a: 1 });
if (O.hasOwnProperty.call(proto, "constructor") !== true) throw new Error("call(constructor) wrong");
if (O.hasOwnProperty.call(proto, "nope") !== false) throw new Error("call(nope) wrong");
console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: Object constructor exposes inherited Object.prototype methods"
