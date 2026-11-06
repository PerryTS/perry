function born(n: number) {
  return { f: () => n, g: (x: number) => n + x, x: n };
}
const a = born(11);
const b = born(17);
console.log(a.f(), b.f(), a.g(2), a.f === b.f);
console.log(Object.keys(a).join(","), Object.getPrototypeOf(a) === Object.prototype);
const d = Object.getOwnPropertyDescriptor(a, "f")!;
console.log(d.writable, d.enumerable, d.configurable);
(a as any).f = () => 23;
console.log(a.f(), b.f());
delete (a as any).g;
(a as any).g = (x: number) => x + 31;
(a as any).extra = 41;
console.log(a.g(3), Object.keys(a).join(","));
const frozen = Object.freeze(born(7));
console.log(frozen.f(), Object.isFrozen(frozen));
let sum = 0;
for (let i = 0; i < 200; i++) {
  const c = born(i);
  const churn = Array.from({ length: 12 }, (_, j) => ({ j, f: () => j }));
  sum += c.f() + churn[5].f();
}
console.log(sum);
function annotated(v: any) {
  return { f: () => 5, n: v as number };
}
for (const value of [13, "text", null, undefined, false]) {
  const c = annotated(value);
  console.log(c.f(), typeof c.n, String(c.n));
}
const calls: string[] = [];
const mixed = {
  f: (() => { calls.push("f"); return () => 19; })(),
  n: (() => { calls.push("n"); return 29; })(),
  known: () => 37,
  method() { return this.n + this.known(); }
};
console.log(calls.join(","), mixed.f(), mixed.method());
