// A function object's own data keys are a key-add on its shape: class fields on
// callable instances (CreateDataProperty), Object.assign onto functions ([[Set]],
// which asks the function's chain), and reads through a bound function's link.
const keys = (f: any) => Reflect.ownKeys(f).join(",");
const desc = (f: any, k: string) => JSON.stringify(Object.getOwnPropertyDescriptor(f, k));
class Callable {
  constructor(fn: Function, attach?: object) {
    return Object.assign(Object.setPrototypeOf(fn.bind(this), new.target.prototype), attach);
  }
}
let setterHits = 0;
class Base extends Callable {
  a = 1;
  b: any = "two";
  constructor(attach?: object) { super((x: number) => x + 1, attach); }
  get viaThis() { return "tag:" + (this as any).tag; }
  set guarded(v: any) { setterHits++; }
  m() { return "Base.m"; }
}
Object.defineProperty(Base.prototype, "frozenKey", { value: "proto", writable: false, configurable: true });
class Mid extends Base { c = [3]; }
class Leaf extends Mid { d = null; e = undefined; }
const nodes: any[] = [];
for (let i = 0; i < 4; i++) nodes.push(new Leaf({ tag: "t" + i, kind: "k", id: i }));
const n = nodes[3];
console.log("1", keys(n), n.a, n.b, n.c[0], n.d, n.e, n.tag, n.kind, n.id, n(41));
console.log("2", desc(n, "a"), desc(n, "tag"), desc(n, "name"), n.name, n.length);
console.log("3", n.viaThis, n.m(), n instanceof Leaf, n instanceof Base, typeof n.call, "call" in n);
// a class field named like an inherited setter is a define: own data, setter not run
class FieldOverSetter extends Base { guarded = "own-field"; }
const fos: any = new FieldOverSetter();
console.log("4", setterHits, desc(fos, "guarded"));
// [[Set]] through Object.assign runs the inherited setter and refuses a non-writable
const viaSet: any = new Base({ guarded: 9 });
console.log("5", setterHits, Object.prototype.hasOwnProperty.call(viaSet, "guarded"));
try { new Base({ frozenKey: 1 }); console.log("6 no throw"); } catch (e) { console.log("6", (e as Error).constructor.name); }
// many keys: past the bag's inline room, same shape path for two functions
const wide = (tag: string) => {
  const f: any = function wide() {};
  for (let i = 0; i < 14; i++) f["k" + i] = tag + i;
  return f;
};
const w1 = wide("x"), w2 = wide("y");
console.log("7", keys(w1), w1.k0, w1.k13, w2.k7, w2.k13);
// overwrite, delete, re-add (enumerates last), then defineProperty attributes
w1.k3 = "over";
delete w1.k5;
w1.k5 = "back";
Object.defineProperty(w1, "k6", { enumerable: false });
w1.k6 = "still-writable";
console.log("8", keys(w1), w1.k3, w1.k5, w1.k6, Object.keys(w1).length);
// non-extensible / sealed / frozen functions
const ne: any = function ne() {}; ne.x = 1; Object.preventExtensions(ne);
try { "use strict"; Object.assign(ne, { y: 2 }); console.log("9 no throw"); } catch (e) { console.log("9", (e as Error).constructor.name, ne.y); }
ne.x = 5;
const fr: any = function fr() {}; fr.x = 1; Object.freeze(fr);
try { Object.assign(fr, { x: 2 }); console.log("10 no throw"); } catch (e) { console.log("10", (e as Error).constructor.name, fr.x); }
try { Object.defineProperty(fr, "z", { value: 1, writable: true, enumerable: true, configurable: true }); } catch (e) { console.log("11", (e as Error).constructor.name); }
console.log("12", ne.x, keys(ne), keys(fr));
// a link set after keys exist: keys kept, the new chain intercepts later stores
let late = 0;
const lk: any = function lk() {};
Object.assign(lk, { p: 1, q: 2 });
Object.setPrototypeOf(lk, { set r(v: any) { late++; }, inherited: "yes" });
Object.assign(lk, { r: 3, s: 4 });
console.log("13", keys(lk), late, lk.inherited, lk.p, lk.s, Object.prototype.hasOwnProperty.call(lk, "r"));
// a setter on Function.prototype intercepts a store to any plain function
let fpHits = 0;
Object.defineProperty(Function.prototype, "fpSetter", { set(v) { fpHits++; }, configurable: true });
const plain: any = function plain() {};
plain.fpSetter = 1; plain.other = 2;
console.log("14", fpHits, Object.prototype.hasOwnProperty.call(plain, "fpSetter"), plain.other);
delete (Function.prototype as any).fpSetter;
// bound functions: name/length from the target, inherited data through the link, null data
const target = function namedTarget(a: number, b: number) { return a + b; };
const bf: any = Object.setPrototypeOf(target.bind(null, 1), { data: null, zero: 0, get g() { return "g:" + typeof this; } });
bf.own = "o";
console.log("15", bf.name, bf.length, bf.data, bf.zero, bf.g, bf.own, bf.missing, bf(2), keys(bf));
// an accessor own key moves the function off the fast path; later puts still work
const acc: any = function acc() {};
acc.a = 1;
Object.defineProperty(acc, "g", { get() { return "own-get"; }, configurable: true, enumerable: true });
acc.b = 2;
console.log("16", keys(acc), acc.g, acc.a, acc.b);
// generator and async functions
function* gen() { yield 1; }
(gen as any).tag = "g";
async function asy() {}
(asy as any).tag = "a";
console.log("17", keys(gen), (gen as any).tag, keys(asy), (asy as any).tag);
// a class field over an own configurable non-writable key redefines it with default attributes
class NW1 extends Callable {
  constructor() { super(() => 0); Object.defineProperty(this, "x", { value: "nw", writable: false, enumerable: false, configurable: true }); }
}
class NW2 extends NW1 { x = "field"; }
const nw: any = new NW2();
console.log("18", desc(nw, "x"), keys(nw));
// a read site that saw a key absent sees it after the add
const rd = (f: any) => f.later;
const g1: any = function g1() {}; g1.a = 1;
const before = rd(g1);
g1.later = "added";
console.log("19", before, rd(g1), rd(function other() {}));
// a bound function's intrinsic name/length with no own keys, read through a link
const bf2: any = Object.setPrototypeOf(target.bind(null), { name: "proto-name", length: 99, extra: "x" });
console.log("20", bf2.name, bf2.length, bf2.extra, typeof bf2.prototype);
// a class field over an own writable non-enumerable key makes it enumerable
class NE1 extends Callable {
  constructor() { super(() => 0); Object.defineProperty(this, "y", { value: "ne", writable: true, enumerable: false, configurable: true }); }
}
class NE2 extends NE1 { y = "field"; }
console.log("21", desc(new NE2(), "y"));
// an own key named like a Function.prototype method, added after other keys
const fc: any = function fc() { return "body"; };
fc.a = 1;
fc.call = () => "own-call";
fc.apply = () => "own-apply";
console.log("22", fc.call(), fc.apply(), typeof fc.bind, keys(fc));
// computed-key stores ([[Set]]) see setters and non-writable keys on the chain
let cks = 0;
const ck = (s: string) => s + "";
const chainProto = { set hook(v: any) { cks++; } };
Object.defineProperty(chainProto, "ro", { value: "ro", writable: false });
const cf: any = Object.setPrototypeOf(function cf() {}, chainProto);
cf[ck("first")] = 1;
cf[ck("hook")] = 2;
try { (function () { "use strict"; cf[ck("ro")] = 3; })(); console.log("23 no throw"); } catch (e) { console.log("23", (e as Error).constructor.name); }
cf[ck("second")] = 4;
console.log("24", cks, keys(cf), cf.ro, Object.prototype.hasOwnProperty.call(cf, "hook"));
Object.defineProperty(Function.prototype, "fpHook", { set(v) { cks++; }, configurable: true });
const pf: any = function pf() {};
pf[ck("p1")] = 1;
pf[ck("fpHook")] = 2;
console.log("25", cks, keys(pf));
delete (Function.prototype as any).fpHook;
// a constructor's prototype materialized after other keys keeps its birth place
const o1: any = function o1() {}; o1.x = 1; o1.prototype; 
const o2: any = function o2() {}; o2.x = 1; o2.prototype = { z: 1 };
const o3: any = () => 0; o3.x = 1; o3.prototype = { z: 1 };
console.log("26", keys(o1), keys(o2), keys(o3));
