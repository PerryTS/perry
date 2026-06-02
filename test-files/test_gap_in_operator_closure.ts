// epic #1785 / #1758: the `in` operator on a CLOSURE (function) value.
//
// Functions are objects in JS, so `key in fn` is valid. Pre-fix,
// `js_object_has_property` treated the closure pointer as an `ObjectHeader`
// and read `(*obj_ptr).keys_array` at the closure's capture-slot offset — a
// NaN-boxed value, not a real `*ArrayHeader` — then SIGSEGV'd in
// `js_array_length`. effect's `dual`-wrapped helpers reach this deep in the
// fiber runtime (`<key> in someClosure`), crashing `import * as S from
// "effect/Schema"` module init (after the #1804/#1809/#1810 fixes let init get
// that far).
//
// Fix: `js_object_has_property` detects a `GC_TYPE_CLOSURE` receiver and
// mirrors the closure read path (`js_object_get_field_by_name`: `length` →
// arity, others → `CLOSURE_DYNAMIC_PROPS`): present-and-not-undefined ⇒ true.
//
// Compared byte-for-byte against `node --experimental-strip-types`.
// (`'name' in fn` / `'prototype' in fn` are a separate pre-existing
// closure-property-completeness gap — perry's `fn.name` also reads undefined —
// so they're intentionally not asserted here.)

const fn: any = () => 1;

// (1) absent key — must be false, NOT a crash (this was the SIGSEGV).
console.log("(1) 'foo' in fn:", "foo" in fn);

// (2) `length` is intrinsic (perry returns the arity).
console.log("(2) 'length' in fn:", "length" in fn);

// (3) a dynamically-assigned property is present.
fn.custom = 42;
console.log("(3) 'custom' in fn:", "custom" in fn);
console.log("(3) 'missing' in fn:", "missing" in fn);

// (4) named function — absent key is false (no crash).
function named() {
  return 2;
}
console.log("(4) 'x' in named:", "x" in (named as any));

// (5) the effect shape: `<key> in fn` where fn is the result of a wrapper.
function makeWrapped(): any {
  const w: any = (x: number) => x + 1;
  w.tag = "wrapped";
  return w;
}
const wrapped = makeWrapped();
console.log("(5) 'tag' in wrapped:", "tag" in wrapped);
console.log("(5) 'nope' in wrapped:", "nope" in wrapped);
