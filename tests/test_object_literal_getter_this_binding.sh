#!/usr/bin/env bash
set -euo pipefail

# An object-literal accessor (`{ get k() {...} }`) is a REGULAR (non-arrow)
# function: `this` binds dynamically to the receiver at call time, NOT to the
# object the accessor is defined on. The HIR lowered it with `captures_this:
# true`, capturing `this` at object-construction time — so an INHERITED read
# (`Object.create(proto).k`, where the getter lives on `proto`) saw
# `this === proto` instead of the instance, and `this[k]` / `this.field` came
# back undefined. @hono/node-server's request prototype
# (`get method() { return this[incomingKey].method }`) crashed on every request.

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
const k = Symbol("k");
const proto: any = {
  get method() { return (this as any)[k] ? (this as any)[k].m : "GET"; },
  get plusOne() { return (this as any).x + 1; },
};
// Inherited read: `this` must be the instance, not `proto`.
const inst: any = Object.create(proto);
inst[k] = { m: "POST" };
inst.x = 41;
if (inst.method !== "POST") throw new Error("inherited symbol this: " + inst.method);
if (inst.plusOne !== 42) throw new Error("inherited field this: " + inst.plusOne);

// A second instance off the same prototype sees its OWN state (no static capture).
const inst2: any = Object.create(proto);
inst2[k] = { m: "PUT" };
inst2.x = 9;
if (inst2.method !== "PUT") throw new Error("instance2 method: " + inst2.method);
if (inst2.plusOne !== 10) throw new Error("instance2 plusOne: " + inst2.plusOne);

// Own (non-inherited) accessor still binds `this` to the object.
const own: any = { _v: 5, get v() { return (this as any)._v; } };
if (own.v !== 5) throw new Error("own accessor this: " + own.v);

// Setter `this` also binds to the receiver (inherited).
const sproto: any = { set val(v: number) { (this as any)._v = v * 2; }, get val() { return (this as any)._v; } };
const sinst: any = Object.create(sproto);
sinst.val = 21;
if (sinst.val !== 42) throw new Error("inherited setter this: " + sinst.val);

// A CLASS getter inherited through `Object.create(instance)` must also bind
// `this` to the leaf object (the runtime walks the prototype chain and the
// vtable getter would otherwise observe the prototype instance).
class A { v = 0; get x() { return (this as any).v + 100; } }
const aInst: any = new A();
const leaf: any = Object.create(aInst);
leaf.v = 5;
if (leaf.x !== 105) throw new Error("inherited class getter this: " + leaf.x);

console.log("OK");
TS

OUT="$("$PERRY" run "$TMPDIR/f.ts" 2>&1)" || { echo "FAIL: perry run errored"; echo "$OUT"; exit 1; }
if ! grep -q "^OK$" <<<"$OUT"; then echo "FAIL: expected OK, got:"; echo "$OUT"; exit 1; fi
echo "PASS: object-literal getter binds this dynamically (inherited + own)"
