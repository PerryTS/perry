// Reflect.get/set/deleteProperty/isExtensible/preventExtensions/setPrototypeOf, with symbol
// keys, a receiver, a primitive target and an async body (#15381).
const sym = Symbol("k");
const o: any = { a: 1, b: 2 };
o[sym] = 3;
class G { w = 1; get v() { return this.w; } set v(x: any) { this.w = x * 10; } }
const g: any = new G();
const r: any = { w: 9 };
Object.defineProperty(o, "fixed", { value: 0 });
console.log([Reflect.get(o, "a"), Reflect.get(o, sym), Reflect.set(o, "c", 3), o.c, Reflect.set(o, sym, 4), o[sym], Reflect.set(o, "fixed", 5), o.fixed, Reflect.deleteProperty(o, "b"), "b" in o, Reflect.deleteProperty(o, "fixed")].join(","));
console.log([Reflect.get(g, "v", r), Reflect.set(g, "v", 2, r), r.w, g.w, Reflect.get(function q() {} as any, "captures")].join(","));
const p = { hi: () => "hi" };
const e: any = {};
console.log([Reflect.setPrototypeOf(e, p), e.hi(), Reflect.isExtensible(e), Reflect.preventExtensions(e), Reflect.isExtensible(e), Reflect.setPrototypeOf(e, null), Reflect.set(e, "n", 1), e.n].join(","));
const errs: string[] = [];
for (const t of [() => Reflect.get(1 as any, "a"), () => Reflect.set("s" as any, "a", 1), () => Reflect.deleteProperty(true as any, "a"), () => Reflect.isExtensible(1 as any), () => Reflect.preventExtensions(null as any), () => Reflect.setPrototypeOf(1 as any, null)]) {
  try { t(); errs.push("none"); } catch (x: any) { errs.push(x.name); }
}
console.log(errs.join(","));
async function f() {
  const a: any = { x: 1 };
  return [Reflect.get(a, "x"), Reflect.set(a, "y", 2), a.y, Reflect.deleteProperty(a, "x"), a.x, Reflect.get(g, "v", r), Reflect.preventExtensions(a), Reflect.isExtensible(a), Reflect.set(a, "z", 1), Reflect.setPrototypeOf(a, p)].join(",");
}
const lit = { w: 1, get v() { return (this as any).w; }, set v(x: any) { (this as any).w = x + 1; } };
const lr: any = { w: 7 };
console.log([Reflect.get(lit, "v", lr), Reflect.get(lit, "v"), Reflect.set(lit, "v", 4, lr), lr.w, lit.w].join(","));
f().then((v) => console.log("async " + v));
