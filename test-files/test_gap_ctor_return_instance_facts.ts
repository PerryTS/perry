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
