// Addition ToPrimitive ordering, boxed-object coercion, error propagation.
// Issues #3562 / #3563 / #3564.

// --- #3562: object / Date ToPrimitive ordering (default hint: valueOf→toString) ---
// (Function-source toString — `f1 + 1` → "function f1(){...}1" — is a separate
// gap: Perry does not retain function source text. Not covered here.)
const o: any = {};
console.log(o + 1);
const a: any = [];
console.log(a + 1);
const v: any = { valueOf() { return 5; } };
console.log(v + 1);
console.log(v + "x");
const t: any = { toString() { return "T"; } };
console.log(t + "!");
const d: any = new Date(0);
console.log((d + "").startsWith("Thu Jan 01 1970"));

// --- #3563: boxed primitive coercion ---
console.log((new Number(2) as any) + 3);
console.log((new String("a") as any) + "b");
console.log((new Boolean(true) as any) + 1);
console.log((new Number(1) as any) + null);
console.log((new String("1") as any) + undefined);
console.log((new Boolean(true) as any) + "1");

// --- #3564: error propagation ---
try {
  const s: any = Symbol();
  console.log(s + 1);
} catch (e: any) {
  console.log("symbol:" + e.name);
}
try {
  const big: any = 1n;
  console.log(big + 1);
} catch (e: any) {
  console.log("bigint:" + e.name);
}
try {
  const thrower: any = { valueOf() { throw new Error("boom"); } };
  console.log(thrower + 1);
} catch (e: any) {
  console.log("thrower:" + e.message);
}
