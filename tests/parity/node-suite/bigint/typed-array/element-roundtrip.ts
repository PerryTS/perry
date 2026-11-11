// #4356 — BigInt64Array / BigUint64Array element get/set must round-trip as
// BigInt (not Number). Reads return a `bigint`, writes ToBigInt-coerce the
// value (a Number throws TypeError), and the prototype/reflection surface
// (join, inspect, set, slice, defineProperty) preserves the BigInt values.

// Element write/read round-trip.
const b = new BigInt64Array(2);
b[0] = 5n;
b[1] = 9n;
console.log("get:", b[0], b[1], typeof b[0]);

// Negative + BigUint64 full-width value (would truncate through f64).
const s = new BigInt64Array(1);
s[0] = -7n;
console.log("neg:", s[0]);
const u = new BigUint64Array(1);
u[0] = 18446744073709551615n;
console.log("u64max:", u[0]);

// Construct from a BigInt array literal.
const c = new BigInt64Array([10n, 20n, 30n]);
console.log("ctor:", c[0], c[1], c[2]);

// join (plain ToString — no trailing `n`) and inspect (with `n`).
console.log("join:", c.join(","));
console.log(c);

// slice keeps BigInt elements (slice-result local is a tracked view).
const sl = c.slice(1);
console.log("slice:", sl.length, sl[0], sl[1]);

// In-place reverse mutates the original view's slots.
c.reverse();
console.log("reverse:", c[0], c[1], c[2]);

// set() from another typed array and from a plain array.
const dstTa = new BigInt64Array(3);
dstTa.set(new BigInt64Array([1n, 2n, 3n]));
console.log("set-ta:", dstTa[0], dstTa[1], dstTa[2]);
const dstArr = new BigInt64Array(3);
dstArr.set([4n, 5n, 6n]);
console.log("set-arr:", dstArr[0], dstArr[1], dstArr[2]);

// Boolean coerces via ToBigInt (true -> 1n).
const t = new BigUint64Array(1);
(t as unknown as boolean[])[0] = true;
console.log("bool:", t[0]);

// defineProperty / Reflect.defineProperty drive IntegerIndexedElementSet.
const d = new BigInt64Array([0n, 0n, 0n]);
Reflect.defineProperty(d, "0", { value: 42n });
Object.defineProperty(d, "1", { value: 43n });
console.log("define:", d[0], d[1]);

// Out-of-bounds canonical index → rejected (Reflect returns false).
console.log("oob:", Reflect.defineProperty(d, "9", { value: 1n }), d[2]);

// A Number value is not convertible: ToBigInt throws a TypeError, no write.
try {
  Object.defineProperty(d, "2", { value: 3 });
  console.log("number-define: no throw");
} catch (e) {
  const err = e as Error;
  console.log("number-define:", err.name, "-", err.message, "->", d[2]);
}
