// A `new C()` is C's instance only when no constructor on the chain returns
// an object: the facts about C's fields must not describe the returned value.
function mk(): any { return { z: 7, w: 9 }; }

// direct
class Direct { x = 5; constructor() { return mk(); } }
const d: any = new Direct();
console.log("direct", d.x, d.z, d.w, Object.keys(d));

// via super: the fields are defined on the value the base constructor returns
class Base { constructor() { return mk(); } }
class Derived extends Base { x = 5; }
const v: any = new Derived();
console.log("super", v.x, v.z, Object.keys(v), v instanceof Derived);

// conditional return
class Cond { x = 5; constructor(k: number) { if (k > 1) return mk(); } }
const c1: any = new Cond(0);
const c2: any = new Cond(2);
console.log("cond", c1.x, c2.x, c2.z, c1 instanceof Cond, c2 instanceof Cond);

// a returned proxy: reads and stores go through its traps
const log: string[] = [];
class Prox {
  x = 5;
  constructor() {
    return new Proxy({}, {
      get(_t, k) { log.push("get " + String(k)); return 42; },
      set(_t, k) { log.push("set " + String(k)); return true; },
    });
  }
}
const p: any = new Prox();
console.log("proxy", p.x, (p.x = 1), log.join(","));

// a primitive return is ignored: still the instance
class Prim { x = 5; constructor() { return 3 as any; } }
const pr = new Prim();
console.log("prim", pr.x, pr instanceof Prim, Object.keys(pr));

// returning this
class Self { x = 5; constructor() { return this; } }
const s = new Self();
console.log("self", s.x, s instanceof Self);

// in a function: the binding is read after a call
function readX(): unknown { const b: any = new Direct(); return b.x; }
console.log("fn", readX());

// a constructor function returning an object (or its argument)
function Fn(this: any, o: any): any { this.x = 5; return o; }
const f1: any = new (Fn as any)({ z: 3 });
console.log("fnobj", f1.x, f1.z, f1 instanceof (Fn as any));
const f2: any = new (Fn as any)(7);
console.log("fnprim", f2.x, f2 instanceof (Fn as any));

// the other construct paths
const r1: any = Reflect.construct(Fn as any, [{ z: 4 }]);
console.log("reflect", r1.x, r1.z);
const r2: any = Reflect.construct(Direct as any, []);
console.log("reflectclass", r2.x, r2.z);
const Bound: any = (Fn as any).bind(null, { z: 5 });
const b1: any = new Bound();
console.log("bound", b1.x, b1.z);
const Expr = class { x = 5; constructor(o: any) { return o; } };
const e1: any = new Expr({ z: 6 });
console.log("clsexpr", e1.x, e1.z);
class FromFn extends (Fn as any) { y = 1; constructor(o: any) { super(o); } }
const s1: any = new FromFn({ z: 8 });
console.log("superfn", s1.x, s1.z, s1.y);
