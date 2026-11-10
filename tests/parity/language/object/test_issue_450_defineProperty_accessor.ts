// Regression test for #450 (https://github.com/PerryTS/perry/issues/450):
// `Object.defineProperty(obj, key, { get, set })` registered the accessor
// closures (descriptor round-trip via `Object.getOwnPropertyDescriptor`
// reported `get`/`set` correctly), but invoking the getter via `obj.value`
// dispatched the closure with the WRONG `this` — the descriptor literal
// itself, not `obj`. Inside `get() { return this._backing * 2; }`, `this`
// resolved to the descriptor `{ get(){}, set(){} }` literal whose
// `_backing` was undefined; the multiplication then produced NaN.
//
// Root cause: descriptor-literal method shorthands `get() {...}` /
// `set(v) {...}` were lowered with `captures_this: true` and had their
// reserved this-slot patched to point to the descriptor object at literal
// construction time (the canonical object-literal-method-with-this pattern
// from `expr.rs::lower_object_literal`). At `Object.defineProperty` time
// the descriptor's `get`/`set` closure pointers were copied verbatim into
// the accessor side table — so subsequent `obj.value` reads invoked
// `js_closure_call0(closure)` and the closure body's `this` slot still
// pointed at the descriptor.
//
// Fix in three places:
//   (a) `crates/perry-codegen/src/expr.rs::Expr::Closure` — when allocating
//       a closure whose HIR has `captures_this: true`, OR in the runtime's
//       `CAPTURES_THIS_FLAG` (0x8000_0000) into the cap_count argument so
//       the runtime can detect closures whose last capture slot is the
//       reserved `this` slot. `js_closure_alloc` masks the flag off when
//       computing allocation size but preserves it in the stored
//       `capture_count` field.
//   (b) `crates/perry-runtime/src/closure.rs::clone_closure_rebind_this`
//       — new helper that, given a NaN-boxed closure value and a NaN-boxed
//       receiver, allocates a fresh closure with the same func_ptr and
//       captures verbatim, then overwrites the last (reserved `this`)
//       capture slot with the receiver. Closures without
//       `CAPTURES_THIS_FLAG` (e.g. arrow form `get: () => obj._backing`
//       without inner `this`) pass through unchanged.
//   (c) `crates/perry-runtime/src/object.rs::js_object_define_property`
//       — at descriptor registration time, route the descriptor's `get`
//       and `set` fields through `clone_closure_rebind_this(bits, obj_box)`
//       BEFORE storing in the accessor side table. The original descriptor
//       closures are untouched so callers that re-use the descriptor see
//       no mutation.
//
// Verified against `node --experimental-strip-types` byte-for-byte.

console.log("--- section 1: getter with this returns computed value ---");
const a: any = { _backing: 5 };
Object.defineProperty(a, "value", {
  get() {
    return this._backing * 2;
  },
  set(v: number) {
    this._backing = v;
  },
  enumerable: true,
  configurable: true,
});
console.log("a.value:", a.value);
console.log("a._backing:", a._backing);

console.log("--- section 2: setter with this mutates backing field ---");
a.value = 21;
console.log("a._backing after set:", a._backing);
console.log("a.value after set:", a.value);

console.log("--- section 3: getOwnPropertyDescriptor reports same shape ---");
const desc = Object.getOwnPropertyDescriptor(a, "value");
console.log("desc has get:", typeof desc?.get === "function");
console.log("desc has set:", typeof desc?.set === "function");
console.log("desc enumerable:", desc?.enumerable);
console.log("desc configurable:", desc?.configurable);

console.log("--- section 4: multiple accessors on same object work independently ---");
const b: any = { _x: 1, _y: 10 };
Object.defineProperty(b, "x10", {
  get() {
    return this._x * 10;
  },
  set(v: number) {
    this._x = v;
  },
});
Object.defineProperty(b, "y100", {
  get() {
    return this._y * 100;
  },
  set(v: number) {
    this._y = v;
  },
});
console.log("b.x10:", b.x10);
console.log("b.y100:", b.y100);
b.x10 = 7;
b.y100 = 70;
console.log("b._x:", b._x);
console.log("b._y:", b._y);
console.log("b.x10 after:", b.x10);
console.log("b.y100 after:", b.y100);

console.log("--- section 5: getter-only (no setter) ---");
const c: any = { _val: 100 };
Object.defineProperty(c, "doubled", {
  get() {
    return this._val * 2;
  },
});
console.log("c.doubled:", c.doubled);

console.log("--- section 6: separate objects bind separately ---");
const obj1: any = { _backing: 3 };
const obj2: any = { _backing: 7 };
function makeAccessor(o: any) {
  Object.defineProperty(o, "tripled", {
    get() {
      return this._backing * 3;
    },
  });
}
makeAccessor(obj1);
makeAccessor(obj2);
console.log("obj1.tripled:", obj1.tripled);
console.log("obj2.tripled:", obj2.tripled);
