// Numeric entry/capture guards establish Numbers, not integer values.
// All three bodies must preserve Number::remainder for the full f64 domain.
function remainder(a: number, b: number): number { return a % b; }
function capturedRemainder(b: number): (a: number) => number {
  return function (a: number): number { return a % b; };
}
class Remainder {
  divisor: number;
  constructor(divisor: number) { this.divisor = divisor; }
  apply(a: number): number { return a % this.divisor; }
}
function show(value: number): string {
  if (Object.is(value, -0)) return "-0";
  return String(value);
}
const cases: number[][] = [
  [31000066, 1000003], [7, 3], [-7, 3], [7, -3], [-7, -3],
  [4, 2], [-4, 2], [4, -2], [-4, -2],
  [0, 5], [-0, 5], [0, -5], [-0, -5],
  [-2147483648, -1], [2147483648, 7], [4294967295, 3],
  [9007199254740991, 7], [9007199254740992, -3],
  [-9007199254740992, 7], [18014398509481984, 3],
  [5.5, 2], [-5.5, 2], [5, 2.5], [1e300, 7], [7, 1e300],
  [5, 0], [5, -0], [0, 0], [NaN, 3], [3, NaN],
  [Infinity, 3], [-Infinity, 3], [3, Infinity], [-3, -Infinity],
];
for (const pair of cases) {
  const a = pair[0];
  const b = pair[1];
  const closure = capturedRemainder(b);
  const receiver = new Remainder(b);
  console.log(show(remainder(a, b)), show(closure(a)), show(receiver.apply(a)), show(receiver.apply(a % b)));
}
// A lying annotation must keep the generic ToNumeric path and operand order.
const events: string[] = [];
const left: any = { valueOf() { events.push("left"); return 17; } };
const right: any = { valueOf() { events.push("right"); return 5; } };
console.log(show(remainder(left, right)), events.join(","));
console.log(show(capturedRemainder("3" as any)("8" as any)));
const mutable = new Remainder(3);
console.log(show(mutable.apply(8)));
(mutable as any).divisor = "5";
console.log(show(mutable.apply(8)));
// Typed Number entry candidates must still reject BigInt values and route
// them through the generic ToNumeric implementation.
const bigLeft: any = -17n;
const bigRight: any = 5n;
console.log("bigint", String(remainder(bigLeft, bigRight)),
  String(capturedRemainder(bigRight)(bigLeft)),
  String(new Remainder(bigRight).apply(bigLeft)));
try {
  remainder(bigLeft, 2);
} catch (error) {
  console.log("mixed-bigint", error instanceof TypeError);
}
(Remainder.prototype as any).apply = function (a: number) { return a + 100; };
console.log(show(mutable.apply(8)));
