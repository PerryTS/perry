// Object.keys / Object.values / Object.entries over ordinary objects of every
// storage kind: ordered keys, attributes, deletes, accessors, fresh results.
function show(label: string, o: any): void {
  console.log(label, JSON.stringify(Object.keys(o)), JSON.stringify(Object.values(o)), JSON.stringify(Object.entries(o)));
}
class Point { x: number; y: number; label: string;
  constructor(x: number, y: number) { this.x = x; this.y = y; this.label = "p" + x; } }
class Point3 extends Point { z: number; constructor(x: number, y: number, z: number) { super(x, y); this.z = z; } }

show("literal", { a: 1, b: "two", c: true, d: null, e: [1, 2] });
show("class", new Point(1, 2));
show("subclass", new Point3(1, 2, 3));
const dyn: any = {}; for (let i = 0; i < 12; i++) dyn["k" + i] = i * i;
show("dyn", dyn);
show("index", { 10: "ten", b: 1, 2: "two", a: 2, 1: "one" });
const ne: any = { a: 1, b: 2, c: 3 };
Object.defineProperty(ne, "hidden", { value: 9, enumerable: false });
Object.defineProperty(ne, "shown", { value: 8, enumerable: true });
show("nonenum", ne);
const del: any = { a: 1, b: 2, c: 3, d: 4 }; delete del.b; delete del.d; del.e = 5;
show("deleted", del);
const sym = Symbol("s"); const sy: any = { a: 1, [sym]: 2, b: 3 };
show("symbol", sy);
const nul: any = Object.create(null); nul.x = 1; nul.y = "y";
show("nullproto", nul);
const big: any = {}; for (let i = 0; i < 70; i++) big["p" + i] = i;
const bv = Object.values(big); const be = Object.entries(big);
console.log("big", Object.keys(big).length, bv.length, bv[0], bv[69], be[35][0], be[35][1]);
const getters: any = { a: 1, get g() { return this.a + 10; }, z: 3 };
show("getter", getters);
const killer: any = { a: 1, get b() { delete this.c; return 2; }, c: 3, d: 4 };
console.log("killer values", JSON.stringify(Object.values(killer)));
const killer2: any = { a: 1, get b() { delete this.c; return 2; }, c: 3, d: 4 };
console.log("killer entries", JSON.stringify(Object.entries(killer2)));
const dp: any = { a: 1 }; Object.defineProperty(dp, "acc", { get() { return 42; }, enumerable: true });
show("defineGetter", dp);
show("frozen", Object.freeze({ q: 1, r: 2 }));
const m: any = { a: 1, b: 2 };
const mk = Object.keys(m), mv = Object.values(m), me = Object.entries(m);
m.c = 3; m.a = 100; mk.push("zz"); mv[0] = -1; me[0][1] = -2;
console.log("fresh", JSON.stringify(mk), JSON.stringify(mv), JSON.stringify(me), JSON.stringify(Object.entries(m)));
console.log("fresh2", Object.values(m) !== Object.values(m), Object.entries(m)[0] !== Object.entries(m)[0]);
function fn() {} (fn as any).extra = 1; (fn as any).more = "m";
show("function", fn);
show("array", [5, 6, , 8]);
show("special", { n: NaN, z: -0, u: undefined, i: Infinity, o: { deep: [1] } });
show("empty", {});
show("string", "hi");
let acc = 0;
for (let r = 0; r < 2000; r++) { const o: any = { a: r, b: r + 1, c: r + 2 }; for (const [k, v] of Object.entries(o)) acc += v + k.length; for (const v of Object.values(o)) acc += v; }
console.log("loop", acc);
