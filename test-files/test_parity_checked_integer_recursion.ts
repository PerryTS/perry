// Actual-value entry checks and every-operation integer exactness.
function bounded(n: number): number {
  if (n !== n) return n;
  if (n < 2) return n;
  if (n > 8) return n;
  return bounded(n - 1) + bounded(n - 2);
}
for (const n of [-4, -0, 0, 1, 1.5, 2.5, 7, NaN, Infinity, -Infinity]) {
  const v = bounded(n);
  console.log("bounded", n, v, 1 / v);
}
const s: any = "1";
console.log("string", typeof bounded(s), bounded(s));
try { bounded(3n as any); console.log("missing type error"); }
catch (e) { console.log("bigint", e instanceof TypeError); }
function rounded(n: number): number {
  if (n < 1) return 9007199254740992;
  return (rounded(n - 1) + 1) - rounded(n - 1);
}
console.log("rounding", rounded(1), rounded(2), rounded(3));
function widen(n: number): number {
  if (n < 1) return 9007199254740992;
  return widen(n - 1) + widen(n - 1);
}
console.log("wide", widen(3));
let trace = "";
function effectful(n: number): number {
  trace += n;
  if (n < 1) return 0;
  return effectful(n - 1) + effectful(n - 1);
}
console.log("effects", effectful(3), trace);
function forever(n: number): number { return forever(n); }
try { forever(Number("1")); console.log("missing stack error"); }
catch (e) { console.log("stack", e instanceof RangeError); }

function crossing(n: number): number {
  if (n < 1) return 2147483647;
  return crossing(n - 1) + 1;
}
const seed: number = Number("1.0");
console.log("i32-overflow", crossing(seed), crossing(seed + 1));
console.log("i32-min", bounded(-2147483648));

try { forever(Number("1.5")); console.log("missing fractional stack error"); }
catch (e) { console.log("fractional-stack", e instanceof RangeError); }
function additive(n: number): number {
  if (n < 3) return n;
  return additive(n - 1) + additive(n - 3) + 2;
}
for (const n of [-0, 0, 1, 2.5, 7]) {
  console.log("additive", additive(Number(n)), 1 / additive(Number(n)));
}
function edge(n: number): number {
  if (n < 1) return 2147483600;
  return edge(n - 1) + 1;
}
console.log("range-exit", edge(Number("40")), edge(Number("80")));
function ternary(n: number): number {
  if (n < 3) return 1;
  return ternary(n - 1) + ternary(n - 2) + ternary(n - 3);
}
console.log("three-edges", ternary(Number("7")));
function tailRange(n: number): number {
  if (n < 1) return n;
  return tailRange(n - 1);
}
for (const n of [Number("1000000000"), Number("1000000000000"), Infinity, NaN]) {
  try { tailRange(n); console.log("missing bounded stack error"); }
  catch (e) { console.log("bounded-stack", e instanceof RangeError); }
}
console.log("proof-bigint-base", typeof additive(1n as any));
try { additive(5n as any); console.log("missing proof type error"); }
catch (e) { console.log("proof-bigint", e instanceof TypeError); }
console.log("proof-string", typeof additive("1" as any));
let proofConversions = 0;
const proofObject: any = { valueOf() { proofConversions++; return 4; } };
console.log("proof-coercion", additive(proofObject), proofConversions);
