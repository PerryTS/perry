// #10601: pins down EXACTLY which class collides with the crafted value's
// embedded id (5), using the same declaration order as the real
// `test_gap_10479_instanceof_value_kinds.ts` file up to (and including) its
// 5th class declaration: K=1, A=2, B=3, Even=4, ShortString=5 (class ids are
// assigned sequentially from 1 in declaration order --
// crates/perry-hir/src/lower/lower_module_fn.rs's `lower_module` starts
// `start_class_id` at 1; `fresh_class()` in lower/context.rs increments it
// once per class declaration it lowers).
//
// If the crafted value collides with ShortString's registered id, then
// `sso3 instanceof craftedNaN` resolves through ShortString's
// `[Symbol.hasInstance]` (`typeof v === "string" && v.length < 6`) applied to
// the LHS -- NOT a TypeError. Confirmed: this produces the exact
// true/false/false pattern the issue reports for sso3/newA/null.
//
// Node throws TypeError for all three lines (this exact file only declares 5
// classes, so on Node -- which has no such collision at all -- the crafted
// value is simply not an object). The try/catch below exists only so both
// engines run to completion and the output is directly comparable.
const dv = new DataView(new ArrayBuffer(8));
dv.setUint32(0, 0x7ffe0000, false);
dv.setUint32(4, 5, false);
const craftedNaN: any = dv.getFloat64(0, false);

class K {}
class A { x = 1; }
class B extends A { y = 2; }
class Even {
  static [Symbol.hasInstance](v: any) { return typeof v === "number" && v % 2 === 0; }
}
class ShortString {
  static [Symbol.hasInstance](v: any) { return typeof v === "string" && v.length < 6; }
}

const cell = (f: () => boolean): string => {
  try {
    return f() ? "T" : "F";
  } catch (e: any) {
    return "E:" + e.constructor.name;
  }
};

const newA = new A();
console.log("sso3 (string, len 3) instanceof craftedNaN:", cell(() => ("uri" as any) instanceof craftedNaN));
console.log("newA (object)        instanceof craftedNaN:", cell(() => (newA as any) instanceof craftedNaN));
console.log("null                 instanceof craftedNaN:", cell(() => (null as any) instanceof craftedNaN));
// If the hypothesis is right this prints T/F/F on Perry (post-#10592),
// matching ShortString.@@hasInstance("uri")=true, (object)=false,
// (null)=false -- i.e. the crafted value IS being dispatched as a
// ShortString ref. Node prints E:TypeError/E:TypeError/E:TypeError.
console.log("independently, ShortString[@@hasInstance] on the same LHS values:",
  (ShortString as any)[Symbol.hasInstance]("uri"),
  (ShortString as any)[Symbol.hasInstance](newA),
  (ShortString as any)[Symbol.hasInstance](null));
