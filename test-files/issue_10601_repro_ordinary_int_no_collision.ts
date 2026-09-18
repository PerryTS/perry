// #10601: does an ORDINARY (non-DataView-crafted) integer 5 collide the same
// way when used as an instanceof RHS in a context where 5 classes are
// already registered? Empirically: NO, on this build. Neither a
// loop-accumulated int32 (`plainFive`, forced through arithmetic so codegen
// can prove int32-ness) nor an Int32Array element read reproduces the
// collision -- both correctly throw, matching Node, even though
// `JSValue::int32()` (crates/perry-runtime/src/value/jsvalue.rs) uses the
// exact same `INT32_TAG | (value as u32 as u64)` encoding that
// `class_constructor_ref_value` does. This suggests ordinary numeric
// codegen paths normalize through a genuine f64 conversion (not the
// INT32_TAG bit-OR trick) before a value becomes generically observable,
// so the collision is reachable via DataView's raw-byte write but not
// (as far as this investigation checked) via ordinary JS arithmetic.
class K {}
class A { x = 1; }
class B extends A { y = 2; }
class Even {
  static [Symbol.hasInstance](v: any) { return typeof v === "number" && v % 2 === 0; }
}
class ShortString {
  static [Symbol.hasInstance](v: any) { return typeof v === "string" && v.length < 6; }
}

function plainFive(): number {
  let acc = 0;
  for (let i = 0; i < 5; i++) acc = acc + 1;
  return acc;
}
const five: any = plainFive();
console.log("typeof five:", typeof five, "five === 5:", five === 5);
const newA = new A();
try {
  console.log("newA instanceof five:", newA instanceof (five as any));
} catch (e: any) {
  console.log("newA instanceof five:", e.constructor.name + ": " + e.message);
}

const ta = new Int32Array([5]);
const fiveFromTypedArray: any = ta[0];
try {
  console.log("newA instanceof fiveFromTypedArray:", newA instanceof (fiveFromTypedArray as any));
} catch (e: any) {
  console.log("newA instanceof fiveFromTypedArray:", e.constructor.name + ": " + e.message);
}
