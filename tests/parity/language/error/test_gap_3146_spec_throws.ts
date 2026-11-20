// Issue #3146 — Perry must raise the spec-mandated TypeError / RangeError
// where it previously silently succeeded. Every line compares byte-for-byte
// against `node --experimental-strip-types`.
//
// Covered:
//   1. `.toString()` member call on a nullish base → TypeError (the value is
//      a property read on undefined/null, unlike abstract ToString which
//      stringifies to "undefined"/"null").
//   2. `new <TypedArray>(negativeLength)` → RangeError (ToIndex).
//   3. `Number.prototype.toString(radix)` with radix ∉ [2, 36] → RangeError.

function caught(label: string, f: () => void): void {
  try {
    f();
    console.log(label + " -> NO THROW");
  } catch (e) {
    console.log(label + " -> " + String(e));
  }
}

// 1. Nullish member calls.
caught("undefined.toString()", () => {
  var u: any;
  u.toString();
});
caught("null.toString()", () => {
  (null as any).toString();
});
caught("null.foo()", () => {
  (null as any).foo();
});

// 2. TypedArray length validation.
caught("new Int32Array(-1)", () => {
  new Int32Array(-1);
});
caught("new Float64Array(-5)", () => {
  new Float64Array(-5);
});
caught("new Uint8Array(-2)", () => {
  new Uint8Array(-2);
});

// 3. Radix validation.
caught("(123).toString(40)", () => {
  (123).toString(40);
});
caught("(123).toString(1)", () => {
  (123).toString(1);
});
caught("(123).toString(0)", () => {
  (123).toString(0);
});

// Abstract ToString of a nullish value stays "undefined"/"null" (no throw).
console.log("String(undefined) =", String(undefined));
console.log("String(null) =", String(null));
console.log("`${undefined}` =", `${undefined}`);

// Valid radix conversions still work.
console.log("(255).toString(16) =", (255).toString(16));
console.log("(255).toString(2) =", (255).toString(2));
console.log("(123).toString() =", (123).toString());
console.log("(123).toString(10) =", (123).toString(10));
console.log("(-255).toString(16) =", (-255).toString(16));

// Valid TypedArray lengths still work; a fractional length truncates (no throw).
console.log("new Int32Array(3).length =", new Int32Array(3).length);
console.log("new Int32Array(2.5).length =", new Int32Array(2.5).length);
console.log("new Uint8Array(0).length =", new Uint8Array(0).length);

// ToIndex edge cases: truncate toward zero BEFORE the negativity check, so a
// negative fraction that truncates to 0 does NOT throw, but a negative integer
// (or ±Infinity) does. The error text shows the original value.
console.log("new Int32Array(-0.5).length =", new Int32Array(-0.5 as any).length);
console.log("new Uint8Array(-0.9).length =", new Uint8Array(-0.9 as any).length);
caught("new Int32Array(-1.5)", () => {
  new Int32Array(-1.5 as any);
});
caught("new Int32Array(Infinity)", () => {
  new Int32Array(Infinity as any);
});
caught("new Uint8Array(-2) [fast-path]", () => {
  new Uint8Array(-2 as any);
});
caught("new Uint8Array(Infinity)", () => {
  new Uint8Array(Infinity as any);
});
