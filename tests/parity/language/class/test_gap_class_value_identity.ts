// #11414: a class used as a value must be a real function object, never
// bit-identical to a number.
class A {
  static s = 7;
  x = 1;
  static make() { return new this(); }
}
class B extends A {
  static t = 9;
}
const one: any = 1;
const two: any = 2;
const vals: any[] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
let eq = 0;
for (const v of vals) { if ((v as any) === (A as any)) eq++; if ((v as any) === (B as any)) eq++; }
console.log("num===class", eq);
console.log("typeof", typeof A, typeof B, typeof one, typeof two);
const m = new Map<any, string>();
m.set(A, "A"); m.set(B, "B");
console.log("map", m.get(1), m.get(2), m.get(A), m.get(B), m.size);
m.set(1, "one");
console.log("map2", m.get(A), m.get(1), m.size);
const wm = new WeakMap<any, string>();
wm.set(A, "wa");
console.log("weakmap", wm.get(A), wm.has(B));
let threw = "no";
try { wm.set(one, "x"); } catch (e) { threw = "yes"; }
console.log("weakmap int key throws", threw);
const s = new Set<any>([A, B, 1, 2]);
console.log("set size", s.size);
console.log("print", String(A).startsWith("class A"), `${B}`.startsWith("class B"));
console.log("name", A.name, B.name, A.length);
console.log("instanceof", new B() instanceof A, new A() instanceof B, (A as any) instanceof Function, (A as any) instanceof Object);
console.log("static inh", (B as any).s, B.t, Object.getPrototypeOf(B) === A);
console.log("static this", (B.make() as any) instanceof B);
console.log("arith", (A as any) + 1 === 2, typeof ((A as any) + 1), Number(A as any));
console.log("json", JSON.stringify({ a: A }), JSON.stringify([A]));
console.log("proto ne num", (A.prototype as any) === (4294967297 as any), typeof A.prototype);
class NT { t: any; constructor() { this.t = new.target; } }
class NT2 extends NT {}
const nt: any = new NT2();
console.log("new.target", nt.t === NT2, typeof nt.t, nt.t === 0, nt.t.name);
function mk(n: number) { return class K { v = n; static tag = n; }; }
const K1 = mk(1), K2 = mk(2);
console.log("expr twice", K1 === K2, K1.tag, K2.tag, new K1().v, new K2().v, new K1() instanceof K2, typeof K1);
const mk2 = new Map<any, number>(); mk2.set(K1, 1); mk2.set(K2, 2);
console.log("expr map", mk2.size, mk2.get(K1), mk2.get(K2));
const arr: any[] = [A, 1];
console.log("indexOf", arr.indexOf(1), arr.indexOf(A), arr.includes(A));
console.log("obj key", Object.is(A, 1), Object.is(A, A));
let callThrew = "no";
try { (A as any)(); } catch (e) { callThrew = (e as any) instanceof TypeError ? "TypeError" : "other"; }
console.log("call throws", callThrew);
console.log("switch", (() => { switch (one as any) { case A: return "A"; default: return "num"; } })());
