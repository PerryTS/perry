// P3 B4a controls (DESIGN-p3 rev 3.1 §3): every JS operation that once decoded
// the 0x7FFE `C.prototype` word now sees the prototype object, and a bound
// method of a class prototype runs its owner's body with the call-time `this`.
// One block per path: descriptors, defineProperty, has_own, delete, symbol
// get, proxy, weakref, instanceof, then bound methods (declared classes and
// per-evaluation classes, whose method values carry the prototype object).

const tag = Symbol("tag");
class A {
  v = 1;
  m(): string { return "A.m:" + (this === undefined ? "undef" : typeof this === "object" ? (this as any).v : String(this)); }
  get g(): number { return 2; }
  set g(_x: number) {}
  static s(): string { return "A.s"; }
  [tag](): string { return "A.sym:" + (this as any)?.v; }
  #p(): string { return "A.#p:" + this.v; }
  callP(): string { return this.#p(); }
}
class B extends A {
  m(): string { return "B.m>" + super.m(); }
}

// descriptors
const dm = Object.getOwnPropertyDescriptor(A.prototype, "m")!;
const dg = Object.getOwnPropertyDescriptor(A.prototype, "g")!;
console.log("desc m", typeof dm.value, dm.writable, dm.enumerable, dm.configurable);
console.log("desc g", typeof dg.get, typeof dg.set, dg.enumerable, dg.configurable);
console.log("desc none", Object.getOwnPropertyDescriptor(A.prototype, "v"), Object.getOwnPropertyDescriptor(B.prototype, "g"));
console.log("descs", Object.keys(Object.getOwnPropertyDescriptors(A.prototype)).join("|"));
console.log("names", Object.getOwnPropertyNames(A.prototype).join("|"), Object.getOwnPropertySymbols(A.prototype).length);

// defineProperty
Object.defineProperty(A.prototype, "k", { value: 7, writable: false, enumerable: true, configurable: true });
Object.defineProperty(A.prototype, "acc", { get() { return "acc:" + (this as any).v; }, configurable: true });
const a = new A();
console.log("define", (a as any).k, (a as any).acc, Object.keys(A.prototype).join("|"),
  Object.getOwnPropertyDescriptor(A.prototype, "k")!.writable);
Object.defineProperty(A.prototype, "m", { value: function (this: any) { return "redefined:" + this.v; }, configurable: true, writable: true });
console.log("redefine", a.m(), new B().m());

// has_own
console.log("hasOwn", Object.hasOwn(A.prototype, "m"), Object.hasOwn(A.prototype, "v"), Object.hasOwn(A.prototype, "constructor"),
  A.prototype.hasOwnProperty("g"), Object.prototype.hasOwnProperty.call(B.prototype, "m"), Object.hasOwn(B.prototype, "g"),
  "m" in B.prototype, "g" in B.prototype, "zz" in A.prototype);

// delete
console.log("delete", delete (A.prototype as any).k, Object.hasOwn(A.prototype, "k"), (a as any).k);
console.log("delete m", delete (A.prototype as any).m, typeof (a as any).m, typeof new B().m);

// symbol get
console.log("symbol", typeof (A.prototype as any)[tag], (A.prototype as any)[tag].call({ v: 9 }), (a as any)[tag](),
  (A.prototype as any)[Symbol.iterator], Object.prototype.toString.call(A.prototype));
(A.prototype as any)[Symbol.toStringTag] = "Ay";
console.log("toStringTag", Object.prototype.toString.call(a), String(a));

// proxy
const px = new Proxy(A.prototype, {});
const pa: any = Object.create(px);
pa.v = 5;
console.log("proxy", typeof px.callP, px.constructor === A, "g" in px, Object.getPrototypeOf(px) === Object.prototype,
  pa.g, typeof (px as any)[tag]);
console.log("proxy target", Reflect.ownKeys(px).length === Reflect.ownKeys(A.prototype).length);

// weakref
const wr = new WeakRef(A.prototype);
const wm = new WeakMap<object, string>([[A.prototype, "a"], [B.prototype, "b"]]);
const ws = new WeakSet<object>([A.prototype]);
console.log("weak", wr.deref() === A.prototype, wm.get(A.prototype), wm.get(Object.getPrototypeOf(new B())), ws.has(A.prototype),
  ws.has(B.prototype), wm.has(Object.getPrototypeOf(a)));

// instanceof
console.log("instanceof", A.prototype instanceof A, B.prototype instanceof A, B.prototype instanceof B, a instanceof A,
  Object.create(A.prototype) instanceof A, A.prototype.isPrototypeOf(new B()), Object.getPrototypeOf(B.prototype) === A.prototype);
class Hy { static [Symbol.hasInstance](x: any) { return x === A.prototype; } }
console.log("hasInstance", (A.prototype as any) instanceof Hy, (a as any) instanceof Hy);

// bound methods of class prototypes (declared class)
class C {
  v = "c";
  m(this: any, x?: string): string { return "C.m:" + (this === undefined ? "undef" : this.v) + (x ?? ""); }
  static st(this: any): string { return "C.st:" + (this === C); }
}
const o = { v: "o" };
const bm = C.prototype.m.bind(o);
const f = C.prototype.m;
console.log("bind", bm(), bm("!"), bm.name, f.call(o), f.call(new C(), "?"), f.apply(o, ["#"]));
let threw = "no";
try { (f as any)(); } catch (e) { threw = (e as Error).constructor.name; }
console.log("bare call", threw, C.prototype.m.call(undefined as any) === "C.m:undef");
const sb = C.st.bind(null);
console.log("static bind", C.st(), (C.st as any).call(C), typeof sb);

// bound methods of class prototypes (a per-evaluation class: one prototype per call)
function make(tagv: string) {
  return class Ev {
    v = tagv;
    m(this: any): string { return "Ev.m:" + tagv + ":" + (this === undefined ? "undef" : this.v); }
    #q(): string { return "Ev.#q:" + this.v; }
    q(): string { return this.#q(); }
    static who(): string { return "Ev.who:" + tagv; }
  };
}
const E1 = make("one");
const E2 = make("two");
const e1 = new E1();
const em = E1.prototype.m;
console.log("eval", em.call({ v: "x" }), em.call(e1), E1.prototype.m.bind(new E2())(), E2.prototype.m.call(e1),
  E1.prototype.m === E2.prototype.m, E1.prototype !== E2.prototype, e1.q(), E1.who());
const eb = e1.m;
console.log("eval detached", eb.call({ v: "d" }), typeof eb, eb.name, Object.getPrototypeOf(e1) === E1.prototype);
const ed = Object.getOwnPropertyDescriptor(E1.prototype, "m")!;
console.log("eval desc", typeof ed.value, ed.value === E1.prototype.m, Object.hasOwn(E1.prototype, "q"),
  delete (E1.prototype as any).q, typeof (e1 as any).q, typeof new E2().q);

// copied onto another class's prototype (mysql2's Command copy)
class P { r(this: any): string { return "P.r:" + this.constructor.name; } }
class Q {}
(Q.prototype as any).r = P.prototype.r;
console.log("copied", (new Q() as any).r(), new P().r());

// super through a bound method value
class S1 { v = "s1"; m(this: any): string { return "S1.m:" + this.v; } }
class S2 extends S1 { m(this: any): string { return "S2.m>" + super.m(); } }
const sm = S2.prototype.m;
console.log("super", sm.call(new S1()), sm.call({ v: "s" }), sm.bind(new S2())());
