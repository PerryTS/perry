// #10510: implicit ToPrimitive on objects and Dates takes cheaper paths
// (the `_req` symbol fallback is latched off, `Date.prototype[Symbol.toPrimitive]`
// and `Date.prototype.valueOf` are dispatched directly when they are the
// builtins). Every override below must still be observed.

// Prototype valueOf wrapper (dayjs-style).
function W(this: any, t: number) { this.t = t; }
W.prototype.valueOf = function (this: any) { return this.t; };
const a: any = new (W as any)(5);
const b: any = new (W as any)(7);
console.log(a < b, a - b, a + b, `${a}`, String(a));

// Symbol.toPrimitive on a plain object, with the hint it received.
const hints: string[] = [];
const tp: any = { [Symbol.toPrimitive](hint: string) { hints.push(hint); return hint === "string" ? "S" : 3; } };
console.log(+tp, tp + 1, `${tp}`, tp < 4, hints.join(","));

// Dates with the builtin methods.
const d1 = new Date(1700000000000);
const d2 = new Date(1700000001000);
const anyD1: any = d1;
const anyD2: any = d2;
console.log(+d1, anyD2 - anyD1, d1 < d2, d1 > d2, d1 <= d2);
console.log(typeof (anyD1 + 1), (anyD1 + 1) === d1.toString() + "1");
console.log(Number.isNaN(+new Date(NaN)));

// Date subclass inherits the builtins.
class MyDate extends Date {}
const md: any = new MyDate(1700000002000);
console.log(+md, md - anyD1);

// Own valueOf on a Date instance shadows the prototype.
const own: any = new Date(1700000000000);
own.valueOf = () => 42;
console.log(+own, own - 2);

// Overridden Date.prototype.valueOf is observed, then restored.
const origValueOf = Date.prototype.valueOf;
(Date.prototype as any).valueOf = function () { return 7; };
console.log(+d1, anyD2 - anyD1);
(Date.prototype as any).valueOf = origValueOf;
console.log(+d1);

// Overridden Date.prototype[Symbol.toPrimitive] is observed, then restored.
const origTP = Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive)!;
Object.defineProperty(Date.prototype, Symbol.toPrimitive, {
  value: function (hint: string) { return hint === "number" ? 11 : "custom"; },
  configurable: true,
});
console.log(+d1, anyD2 - anyD1);
Object.defineProperty(Date.prototype, Symbol.toPrimitive, origTP);
console.log(+d1);

// The builtin Date toPrimitive borrowed onto a plain object runs
// OrdinaryToPrimitive on that object.
const borrowed: any = {
  [Symbol.toPrimitive]: (Date.prototype as any)[Symbol.toPrimitive],
  valueOf() { return 9; },
  toString() { return "str"; },
};
console.log(+borrowed, borrowed + "", `${borrowed}`);

// valueOf returning an object falls through to toString for a Date too.
const weird: any = new Date(0);
weird.valueOf = () => ({});
console.log(typeof (+weird), weird - 0 !== weird - 0);

// An explicit valueOf call still brand-checks its receiver.
try {
  (Date.prototype.valueOf as any).call({});
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}
