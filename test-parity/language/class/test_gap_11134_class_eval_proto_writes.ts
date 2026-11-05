// #11134: a prototype write on ONE evaluation of a function-body class
// expression must land on THAT evaluation's own prototype object — an own,
// enumerable key visible to Object.keys / hasOwnProperty /
// getOwnPropertyNames — and must not be observable through instances of any
// other evaluation. Perry routed literal writes (`C.prototype.z = v`) into a
// side table keyed by the shared class TEMPLATE, and computed writes
// (`C.prototype[k] = v`) into the same table once #11043 taught reflection to
// recognize per-evaluation prototypes — so every evaluation saw every other
// evaluation's methods, and none of them showed up as own keys.

function show(label: string, C: any, key: string) {
  const p = C.prototype;
  console.log(
    label,
    JSON.stringify(Object.keys(p)),
    JSON.stringify(Object.getOwnPropertyNames(p)),
    p.hasOwnProperty(key),
    key in p,
  );
}

// 1. computed key, class with a static field (per-evaluation class object)
function computed(k: string, v: any) {
  const C = class {
    static tag = k;
  };
  C.prototype[k] = v;
  return C;
}
const X = computed("x", 1);
const Y = computed("y", 2);
show("computed X", X, "x");
show("computed Y", Y, "y");
console.log("computed cross", (new X() as any).x, (new X() as any).y, (new Y() as any).x, (new Y() as any).y);

// 2. literal key
function literal(v: any) {
  const C = class {
    static tag = v;
  };
  (C.prototype as any).z = v;
  (C.prototype as any).get = function (this: any) {
    return "z=" + this.z;
  };
  return C;
}
const Z1 = literal(1);
const Z2 = literal(2);
show("literal Z1", Z1, "z");
console.log("literal values", (new Z1() as any).z, (new Z2() as any).z, (new Z1() as any).get(), (new Z2() as any).get());

// 3. aliased prototype (`const p = C.prototype; p.m = …`), private-brand class
function aliased(name: string) {
  const C = class {
    #secret = name;
    reveal() {
      return this.#secret;
    }
  };
  const p: any = C.prototype;
  p["hello_" + name] = function () {
    return "hello " + name;
  };
  p.shared = name;
  return C;
}
const A1 = aliased("a");
const A2 = aliased("b");
show("aliased A1", A1, "shared");
const a1: any = new A1();
const a2: any = new A2();
console.log("aliased values", a1.shared, a2.shared, a1.reveal(), a2.reveal(), typeof a1.hello_b, typeof a2.hello_a, a1.hello_a());

// 4. dynamic heritage (the redis `attachConfig` shape) with a command loop
class Base {
  base() {
    return "base";
  }
}
class Other {
  other() {
    return "other";
  }
}
function attach(BaseClass: any, commands: Record<string, string>) {
  const RESP = 2,
    Class = class extends BaseClass {};
  for (const [name, cmd] of Object.entries(commands)) {
    Class.prototype[name] = function () {
      return cmd + "/" + RESP;
    };
  }
  return Class;
}
const Client = attach(Base, { get: "GET", set: "SET" });
const Multi = attach(Other, { exec: "EXEC" });
show("heritage Client", Client, "get");
const c: any = new Client();
const m: any = new Multi();
console.log("heritage values", c.get(), c.set(), typeof c.exec, c.base(), m.exec(), typeof m.get, m.other());

// 5. a NAMED class expression writing through its own inner binding from a
//    static block (the write runs while the class is being defined). Called
//    through a function value so the factory is not inlined.
function staticBlock(v: number) {
  const C = class C {
    static tag = v;
    static {
      (C.prototype as any).m = v;
      (C.prototype as any)["k" + v] = v;
    }
  };
  return C;
}
const makers: Array<(v: number) => any> = [staticBlock];
const S1 = makers[0](1);
const S2 = makers[0](2);
show("static block S1", S1, "m");
console.log("static block values", (new S1() as any).m, (new S2() as any).m, (new S1() as any).k2, (new S2() as any).k1);

// 6. controls: a module-level class and a function-body class DECLARATION
class Top {}
(Top.prototype as any).t = 1;
show("top", Top, "t");
function decl(v: number) {
  class D {
    get v() {
      return v;
    }
  }
  (D.prototype as any).w = v;
  return D;
}
const D1 = decl(1);
const D2 = decl(2);
console.log("decl", (new D1() as any).w, (new D2() as any).w, (new D1() as any).v, Object.keys(D1.prototype));
