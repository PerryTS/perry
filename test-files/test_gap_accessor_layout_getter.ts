// A named read whose receiver's layout (or its chain's) names an accessor:
// the getter is called with the receiver as `this`, in every arrangement.
const out: string[] = [];
function log(s: string) { out.push(s); }

// 1. A getter on a prototype literal, receivers from Object.create.
const proto = {
  a: 1, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9, j: 10,
  get value() { return (this as any).x * 2; },
};
const objs: any[] = [];
for (let i = 0; i < 50; i++) { const o = Object.create(proto); o.x = i; objs.push(o); }
let s1 = 0;
for (let r = 0; r < 200; r++) for (const o of objs) s1 += o.value;
log("proto getter " + s1);

// 2. An own accessor on a literal.
const own = { n: 5, get v() { return this.n + 1; } };
let s2 = 0;
for (let r = 0; r < 1000; r++) s2 += own.v;
log("own getter " + s2);

// 3. A getter defined on an instance after its birth.
const later: any = { p: 1 };
Object.defineProperty(later, "late", { get() { return this.p + 100; }, configurable: true });
log("late getter " + later.late);
Object.defineProperty(later, "late", { get() { return this.p + 200; }, configurable: true });
log("redefined getter " + later.late);
Object.defineProperty(later, "late", { value: 7, configurable: true });
log("now data " + later.late);

// 4. A setter-only accessor reads undefined.
const so = Object.create({ set w(_v: number) {} });
log("setter only " + so.w);

// 5. A getter that throws.
const thrower = Object.create({ get boom(): number { throw new Error("boom!"); } });
try { log("no " + thrower.boom); } catch (e: any) { log("caught " + e.message); }

// 6. An own data property shadows the prototype's getter; deleting it
// exposes the getter again.
const sh = Object.create(proto);
sh.x = 3;
sh.value = "own";
log("shadowed " + sh.value);
delete sh.value;
log("unshadowed " + sh.value);

// 7. Two hops: the getter two prototypes up.
const mid = Object.create(proto);
mid.m = 1;
const deep = Object.create(mid);
deep.x = 21;
log("two hops " + deep.value);

// 8. Class instance and static getters, and an inherited class getter.
class Box { k: number; constructor(k: number) { this.k = k; } get twice() { return this.k * 2; } static get tag() { return "BOX"; } }
class SubBox extends Box { get thrice() { return this.k * 3; } }
const boxes: any[] = [new Box(1), new SubBox(2), new Box(3)];
let s8 = 0;
for (let r = 0; r < 100; r++) for (const b of boxes) s8 += b.twice;
log("class getters " + s8 + " " + (boxes[1] as any).thrice + " " + (Box as any).tag);

// 9. A getter read in a generator body (outlined sites).
function* gen(list: any[]) { for (const o of list) yield o.value; }
let s9 = 0;
for (const v of gen(objs)) s9 += v;
log("generator " + s9);

// 10. A getter on Object.prototype seen from a plain object.
Object.defineProperty(Object.prototype, "__accProbeExtra", { get() { return typeof this; }, configurable: true });
const plain: any = { q: 1 };
log("object prototype getter " + plain.__accProbeExtra);
delete (Object.prototype as any).__accProbeExtra;
log("removed " + plain.__accProbeExtra);

// 11. Exotic receivers with getters on their prototypes.
const arr: any = [1, 2, 3];
Object.defineProperty(Array.prototype, "accFirst", { get() { return this[0]; }, configurable: true });
log("array getter " + arr.accFirst);
delete (Array.prototype as any).accFirst;
const fn: any = function () {};
Object.defineProperty(Function.prototype, "accName", { get() { return "fn"; }, configurable: true });
log("function getter " + fn.accName);
delete (Function.prototype as any).accName;

// 12. One site, many receivers: data, getter, missing.
const mixed: any[] = [{ value: 1 }, objs[3], {}, Object.create(proto, { x: { value: 10 } }), { value: "s" }];
const seen: string[] = [];
for (const o of mixed) seen.push(String(o.value));
log("mixed " + seen.join(","));

// 13. A getter returning `this`, and `this` of the receiver, not the holder.
const selfp = { get me() { return this; } };
const child = Object.create(selfp);
log("this is receiver " + (child.me === child) + " " + (selfp.me === selfp));

// 14. A getter that changes the receiver's layout while the site runs.
const growp = { get grow() { (this as any)["k" + (this as any).n] = 1; (this as any).n++; return (this as any).n; } };
const gr = Object.create(growp);
gr.n = 0;
let s14 = 0;
for (let r = 0; r < 20; r++) s14 += gr.grow;
log("grow " + s14 + " " + Object.keys(gr).length);

console.log(out.join("\n"));
