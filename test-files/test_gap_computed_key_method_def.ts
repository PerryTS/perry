// #321 / #1785: class methods defined with a *computed* key —
// `[KEY](args) { ... }` where KEY is a string literal, a local const, or a
// (cross-module) const member expression — were silently DROPPED during HIR
// lowering (`return Err("Unsupported method key")` for any non-well-known
// computed key). `instance["KEY"]` then read `undefined`.
//
// This was the root of effect's `FiberRuntime` op-dispatch hang (#321): the
// fiber evaluation loop dispatches `this[(cur)._op](cur)` where `_op` is an op
// code string ("OnSuccess", "Sync", ...) and the handlers are defined as
// `[OpCodes.OP_ON_SUCCESS](op) {...}`. With the handlers missing, `cur` never
// advanced and `Effect.map(...)` looped forever.
//
// Fix: a generic computed-key method lowers to a per-instance closure field
// keyed by the runtime-evaluated key expression (`this[expr] = function (...)
// {...}`), so `this` binds dynamically and dynamic dispatch `recv[k]()` finds
// it. Compared byte-for-byte against `node --experimental-strip-types`.

const KEY = "myOp";
const SECOND = "second";

class C {
  base = 10;
  // literal-string computed key
  ["litKey"](x: number): number {
    return x + 1;
  }
  // local-const computed key + `this` access
  [KEY](x: number): number {
    return this.base + x;
  }
  // another local-const key, calls a sibling computed-key method via `this[k]`
  [SECOND](x: number): number {
    return (this as any)["myOp"](x) * 2;
  }
  // a plain method must still work alongside computed-key methods
  plain(x: number): number {
    return x - 1;
  }
}

const c: any = new C();
console.log("typeof c[KEY]:", typeof c[KEY]);
console.log("c.litKey(5):", c["litKey"](5));
console.log("c.myOp(5):", c["myOp"](5)); // base(10) + 5 = 15
console.log("c.second(5):", c["second"](5)); // myOp(5)=15, *2 = 30
console.log("c.plain(5):", c.plain(5));

// Dynamic dispatch by a runtime-chosen key (effect's `this[op]` shape).
const op = "myOp";
console.log("dynamic c[op](3):", c[op](3)); // 13

// Truly-dynamic key (array-sourced, not const-foldable) — exercises the
// dynamic-dispatch `this`-binding path (`js_native_call_method_value`). A
// method stored as a per-instance closure field must still bind `this` when
// reached via a runtime key, which is effect's `this[(cur)._op](cur)` shape.
const dynKey = ["myOp"][0];
console.log("array-key c[dynKey](4):", c[dynKey](4)); // base(10)+4 = 14

// A custom Symbol used as a computed method key (effect's `[Hash.symbol]()` /
// `[Equal.symbol]()` shape). The method installs under the symbol and is
// callable with `this` bound.
const TAG = Symbol("tag");
class WithSym {
  v = 42;
  [TAG](): number {
    return this.v;
  }
}
const ws: any = new WithSym();
console.log("symbol-key method:", ws[TAG]());

// NOTE: *static* computed-key methods (`static [SKEY](n) {...}`) are a deferred
// follow-up — they need closure-valued static-field dispatch in codegen that
// isn't wired up yet, so this fix scopes to instance methods (what effect's
// FiberRuntime op handlers are).

// Class EXPRESSION with a computed-key method (the #1785 heap-object path).
const EXPR_KEY = "run";
const Klass = class {
  mult = 3;
  [EXPR_KEY](n: number): number {
    return this.mult * n;
  }
};
const k: any = new Klass();
console.log("class-expr run(4):", k["run"](4)); // 12
