function arithmetic(a: any, b: any) {
  return [a + b, a - b, a * b, a / b, a % b, a ** b].map(String).join("|");
}
const values: any[] = [0, -0, 1, -1, 1.25, -1.25, 1e308, -1e308,
  Number.MIN_VALUE, -Number.MIN_VALUE, Infinity, -Infinity, NaN,
  "2", "-2", "", undefined, null, false, true];
function comparisons(a: any, b: any) {
  return [a < b, a <= b, a > b, a >= b, a === b, a !== b, a == b, a != b].join("|");
}
for (const a of values) {
  for (const b of values) console.log(arithmetic(a, b), comparisons(a, b));
}
console.log(arithmetic(3n, 2n));
try { console.log(arithmetic(3n, 2)); } catch (e) { console.log(e instanceof TypeError); }
let calls: string[] = [];
const a = {[Symbol.toPrimitive](hint: string) { calls.push("a:" + hint); return 7; }};
const b = {[Symbol.toPrimitive](hint: string) { calls.push("b:" + hint); return 2; }};
console.log(arithmetic(a, b));
console.log(calls.join(","));
const marker = {};
const throwing = {[Symbol.toPrimitive]() { throw marker; }};
try { console.log(arithmetic(1, throwing)); } catch (e) { console.log(e === marker); }
