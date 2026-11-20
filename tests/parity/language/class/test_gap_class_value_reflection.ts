// Class constructors as function objects: reflection, statics, dynamic new,
// class expressions evaluated more than once.
class A {
  static s = 7;
  static get g() { return "g" + this.s; }
  static m() { return this.name; }
  x = 1;
  hi() { return "hi" + this.x; }
}
class B extends A { static t = 9; }
const a: any = A, b: any = B;
console.log("ctor", A.prototype.constructor === A, Object.getPrototypeOf(new A()) === A.prototype);
console.log("proto chain", Object.getPrototypeOf(B) === A, Object.getPrototypeOf(A) === Function.prototype,
  Object.getPrototypeOf(B.prototype) === A.prototype);
console.log("own", a.hasOwnProperty("s"), b.hasOwnProperty("s"), b.hasOwnProperty("t"), "s" in b, "prototype" in a);
console.log("keys", Object.keys(A).join(","), Object.keys(B).join(","));
console.log("names", Object.getOwnPropertyNames(A).sort().join(","));
console.log("statics", B.m(), B.g, a.g, A.m());
a.s = 8;
console.log("static write", A.s, b.s, B.g);
b.s = 5;
console.log("shadow", A.s, B.s, b.hasOwnProperty("s"));
a.dyn = "d";
console.log("dyn", a.dyn, b.dyn, Object.keys(A).includes("dyn"));
delete a.dyn;
console.log("deleted", a.dyn, "dyn" in a);
Object.defineProperty(A, "ro", { value: 3, writable: false, enumerable: false });
console.log("define", a.ro, Object.keys(A).includes("ro"), Object.getOwnPropertyDescriptor(A, "ro")!.writable);
const d = Object.getOwnPropertyDescriptor(A, "prototype")!;
console.log("proto desc", d.writable, d.enumerable, d.configurable);
function mk(c: any, ...args: any[]) { return new c(...args); }
console.log("dyn new", mk(A).hi(), mk(B) instanceof A, mk(B).x);
console.log("reflect", (Reflect.construct(A, []) as any).hi(), Reflect.construct(A, [], B) instanceof B);
const Bound: any = (A as any).bind(null);
console.log("bound", new Bound() instanceof A, typeof Bound);
let t = "no"; try { (A as any).call({}); } catch (e) { t = (e as any).constructor.name; }
console.log("call", t);
console.log("fnproto", typeof (A as any).call, typeof (A as any).apply, (A as any).call === Function.prototype.call, (A as any).bind === Function.prototype.bind);
const list: any[] = [];
for (let i = 0; i < 3; i++) {
  const C = class { static n = i; v = i * 10; static who() { return this.n; } };
  list.push(C);
}
console.log("expr", list[0] === list[1], list.map((c) => c.n).join(","), list.map((c) => new c().v).join(","),
  list.map((c) => c.who()).join(","), new list[1]() instanceof list[1], new list[1]() instanceof list[2]);
const s = new Set(list);
console.log("expr set", s.size, list.map((c) => typeof c).join(","));
class Base { static create(this: any) { return new this(); } kind() { return "base"; } }
class Derived extends Base { kind() { return "derived"; } }
console.log("static this ctor", (Derived.create() as any).kind(), (Base.create() as any).kind());
const reg = new Map<any, string>([[A, "a"], [B, "b"]]);
for (const [k, v] of reg) console.log("iter", k === A ? "A" : k === B ? "B" : "?", v, typeof k);
const wm = new WeakMap<any, number>([[A, 1]]);
console.log("wm", wm.get(A), wm.get(B), wm.has(A));
console.log("eq", (A as any) == 1, (A as any) == (A as any), (A as any) !== (B as any), [A].includes(1 as any));
console.log("num", isNaN(A as any), typeof (+(A as any)), (A as any) < 5, (A as any) > 0);
console.log("str", String(B).length > 0, Object.prototype.toString.call(A));
console.log("obj", Object(A) === A, typeof Object(A));
