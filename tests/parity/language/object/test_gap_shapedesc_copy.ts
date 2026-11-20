// Assign invokes source getters and destination setters; spread defines data.
const sym = Symbol("s");
const log: string[] = [];
const src: any = {};
Object.defineProperty(src, "10", { value: 10, enumerable: true });
Object.defineProperty(src, "2", { value: 2, enumerable: true });
Object.defineProperty(src, "secret", { value: 99 });
Object.defineProperty(src, "first", { get() { log.push("get first"); return 7; }, enumerable: true });
Object.defineProperty(src, sym, { get() { log.push("get symbol"); return 8; }, enumerable: true });
const proto: any = { set first(v: number) { log.push("set first " + v); } };
const assigned: any = Object.assign(Object.create(proto), src);
console.log("assign", Object.keys(assigned).join(","), Object.hasOwn(assigned, "first"),
  assigned[sym], log.join("|"), Object.getPrototypeOf(assigned) === proto);
log.length = 0;
const spread: any = { ...src };
console.log("spread", Object.keys(spread).join(","), spread.first, spread[sym], log.join("|"));
const d: any = Object.getOwnPropertyDescriptor(spread, "first");
console.log("spread-data", d.value, d.writable, d.enumerable, d.configurable, typeof d.get);
const special: any = Object.create(null);
Object.defineProperty(special, "__proto__", { value: { marker: 37 }, enumerable: true });
const as: any = Object.assign({}, special);
const sp: any = { ...special };
console.log("proto-key", as.marker, Object.hasOwn(as, "__proto__"),
  sp.__proto__.marker, Object.hasOwn(sp, "__proto__"), Object.getPrototypeOf(sp) === Object.prototype);
const mutable: any = { a: 1, b: 2 };
const added = Symbol("late");
const target: any = { set a(v: number) { delete mutable.b; mutable.c = 3; mutable[added] = 4; } };
Object.assign(target, mutable);
console.log("snapshot", Object.keys(target).join(","), Object.hasOwn(target, "b"),
  Object.hasOwn(target, "c"), Object.getOwnPropertySymbols(target).length);
for (const restricted of [Object.freeze({ x: 5 }), Object.seal({ x: 6 }), Object.preventExtensions({ x: 7 })]) {
  const a = Object.assign({}, restricted);
  const b = { ...restricted };
  console.log("copy-restricted", a.x, b.x, Object.isExtensible(a), Object.isExtensible(b),
    Object.getOwnPropertyDescriptor(a, "x")!.writable, Object.getOwnPropertyDescriptor(b, "x")!.configurable);
}
