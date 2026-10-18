// Inherited method reads and constructor runs in a standalone constructor
// reached through `super(...)` (object model 3.6, PLAN 3c): `this` is an
// instance of the class or of any class descending from it, so `this.m`
// reads the holder slot of the receiver class's chain and the constructor's
// run of stores is proved at the site while every prototype is on its static
// shape; any other receiver, chain or prototype change takes the ordinary
// read and the runtime run check.

class Base {
  declare tag: string;
  declare m: any;
  declare n: any;
  declare k: any;
  declare late: any;
  constructor(tag: string) {
    this.tag = tag;
    this.m = this.m.bind(this);
    this.n = this.n.bind(this);
    this.k = typeof this.k === "function" ? this.k() : "no-k";
    this.hook();
    this.late = this.late === undefined ? "late-unset" : "late-set:" + this.late;
  }
  m() { return "Base.m:" + this.tag; }
  n() { return "Base.n:" + this.tag; }
  k() { return "Base.k"; }
  hook() {}
}
class A extends Base {
  constructor(...args: [string]) { super(...args); }
}
class B extends Base {
  declare extra: number;
  constructor(tag: string) { super(tag); this.extra = 1; }
  n() { return "B.n:" + this.tag; }
}
class C extends B {
  hook() { (this as any).late = "from-hook"; }
}
class D extends Base {}

function show(label: string, v: any) {
  console.log(label, typeof v === "function" ? v() : v);
}
function make(K: any, tag: string) { return new K(tag); }

for (const K of [A, B, C, D, Base]) {
  const o = make(K, K.name);
  show(K.name + ".m", o.m);
  show(K.name + ".n", o.n);
  show(K.name + ".k", o.k);
  show(K.name + ".late", o.late);
}

// A receiver of another class through Reflect.construct.
class Other { m() { return "Other.m"; } n() { return "Other.n"; } k() { return "Other.k"; } hook() {} }
const r = Reflect.construct(Base, ["r"], Other);
show("r.m", r.m);
show("r.n", r.n);
show("r proto", Object.getPrototypeOf(r) === Other.prototype);

// The holder's method replaced after the first births.
(Base.prototype as any).k = function () { return "Base.k2"; };
show("A.k after", make(A, "a2").k);

// An intermediate prototype gains a method named like a read key.
(B.prototype as any).m = function () { return "B.m:" + this.tag; };
show("C.m after", make(C, "c2").m);
show("A.m still", make(A, "a3").m);

// An accessor named like a family key on an intermediate prototype: the
// constructor's adds of it must call the setter.
Object.defineProperty(B.prototype, "late", {
  get() { return (this as any)._late; },
  set(v: any) { (this as any)._late = "setter:" + v; },
  configurable: true,
});
const c3 = make(C, "c3");
show("C.late setter", c3.late);
show("C own late", Object.prototype.hasOwnProperty.call(c3, "late"));
show("A.late plain", make(A, "a4").late);

// A setter on Object.prototype named like a family key.
Object.defineProperty(Object.prototype, "tag", {
  set(v: any) {
    Object.defineProperty(this, "tag", { value: "op:" + v, writable: true, enumerable: true, configurable: true });
  },
  configurable: true,
});
show("D.tag", make(D, "d5").tag);
show("D.m", make(D, "d6").m);
delete (Object.prototype as any).tag;

// An instance relinked before its constructor ends.
class E extends Base {
  constructor(tag: string) {
    super(tag);
    Object.setPrototypeOf(this, { m() { return "relinked"; } });
  }
}
show("E.m own", make(E, "e").m);
show("E keys", Object.keys(make(E, "e2")).join(","));
console.log("done");
