// A Map, Set, Date, array or String wrapper whose OWN [[Prototype]] or own property
// supplies a method: the call reads that method (ECMA-262 Get on the
// receiver), not the family's built-in, even where the compiler knows the
// receiver's kind (a top-level `new Set(...)`) and folds the call.
const mine = function (this: any) { return "mine"; };
function viaProto(v: any, C: any, m: string): any {
  const P: any = { [m]: mine };
  Object.setPrototypeOf(P, C.prototype);
  Object.setPrototypeOf(v, P);
  return v;
}
const s1: any = new Set([1]);
viaProto(s1, Set, "has");
const s2: any = new Set([1]);
viaProto(s2, Set, "union");
const s3: any = new Set([1]);
s3.union = mine;
const s4: any = new Set([1]);
s4.has = mine;
const m1: any = new Map([[1, 2]]);
viaProto(m1, Map, "has");
const m2: any = new Map([[1, 2]]);
viaProto(m2, Map, "get");
const m3: any = new Map([[1, 2]]);
m3.get = mine;
const d1: any = new Date(0);
viaProto(d1, Date, "getTime");
const a1: any = [1, 2];
viaProto(a1, Array, "includes");
const w1: any = new String("ab");
viaProto(w1, String, "padStart");
class S extends Set<number> { has(_v: number): any { return "mine"; } }
const s5: any = new S([1]);
// One function per receiver, so the compiler knows each receiver's kind.
function c0(x: any): string { return String(x.has(1)); }
console.log("set proto has", c0(s1));
function c1(x: any): string { return String(x.union(new Set([2]))); }
console.log("set proto union", c1(s2));
function c2(x: any): string { return String(x.union(new Set([2]))); }
console.log("set own union", c2(s3));
function c3(x: any): string { return String(x.has(1)); }
console.log("set own has", c3(s4));
function c4(x: any): string { return String(x.has(1)); }
console.log("map proto has", c4(m1));
function c5(x: any): string { return String(x.get(1)); }
console.log("map proto get", c5(m2));
function c6(x: any): string { return String(x.get(1)); }
console.log("map own get", c6(m3));
function c7(x: any): string { return String(x.getTime()); }
console.log("date proto getTime", c7(d1));
function c8(x: any): string { return String(x.includes(1)); }
console.log("array proto includes", c8(a1));
function c9(x: any): string { return String(x.has(1)); }
console.log("set subclass has", c9(s5));
function c10(x: any): string { return String(x.padStart(5)); }
console.log("string wrapper proto padStart", c10(w1));
const nw: any = new Number(3.14159);
const NP: any = { toFixed: function () { return "mine"; } };
Object.setPrototypeOf(NP, Number.prototype);
Object.setPrototypeOf(nw, NP);
function c11(x: any): string { return String(x.toFixed(2)); }
console.log("number wrapper proto toFixed", c11(nw));
const ow: any = new String("xy");
ow.padEnd = function () { return "mine"; };
function c12(x: any): string { return String(x.padEnd(4)); }
console.log("string wrapper own padEnd", c12(ow));
// a member added to Set.prototype under a name the direct Set lowering claims
const us = new Set([1, 2]);
const ut = new Set([3]);
Object.getPrototypeOf(us).union = function () { return "mine"; };
function c13(x: any): string { return String(x.union(ut)); }
console.log("set proto added union", String(us.union(ut)), String((us as any)["union"](ut)), c13(us));
delete Object.getPrototypeOf(us).union;
// controls: untouched receivers keep the built-ins
function k0(x: any): string { return String(x.has(1)); }
function k1(x: any): string { return String(x.get(1)); }
function k2(x: any): string { return String(x.getTime()); }
const ks: any = new Set([1]);
const km: any = new Map([[1, 2]]);
const kd: any = new Date(0);
console.log("control set", k0(ks), "map", k1(km), "date", k2(kd));
function k3(x: any): string { return String(x.padStart(5, "-")); }
const kw: any = new String("ab");
console.log("control wrapper", k3(kw), k3("cd"), typeof kw.padStart, kw.length);
