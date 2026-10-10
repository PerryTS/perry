// Reassignment outside a comparison graph does not change its local reads.
function classify(value: any): string {
  let c: any = 0;
  c = value;
  const result = c === 45 || c === 46 || c === 95 || c === 126
    || (c >= 48 && c <= 57) || (c >= 65 && c <= 90)
    || (c >= 97 && c <= 122);
  return String(result);
}
for (const value of [45, 46, 95, 126, 48, 57, 65, 90, 97, 122, 32, 47,
  -1, NaN, Infinity, -Infinity, -0, undefined, null, true, false, "48", "x", 48n]) {
  console.log(typeof value, String(value), classify(value));
}
let calls = 0;
const coercible = { valueOf() { calls++; if (typeof globalThis.gc === "function") globalThis.gc(); return 99; } };
console.log("coercion", classify(coercible), calls);
// A closure can mutate a captured binding during ToPrimitive: each later
// comparison must reload that binding, including after a moving collection.
function captured(): string {
  let c: any = 0;
  const source = { valueOf() { c = 200; if (typeof globalThis.gc === "function") globalThis.gc(); return 0; } };
  c = source;
  return String(c >= 100 || c === 200) + ":" + String(c);
}
console.log("captured", captured());
function insideGraph(value: any): string {
  let c: any = value;
  c = value;
  return String(c >= 100 || (c = 200) >= 100) + ":" + String(c);
}
console.log("write", insideGraph(0));
