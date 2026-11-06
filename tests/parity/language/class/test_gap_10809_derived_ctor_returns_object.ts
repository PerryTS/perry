// #10809: a derived constructor that calls super() and then returns an object
// uses that object as the construction result. Returning a Proxy wrapping
// `this` is the node-fetch `Headers` shape. Each Proxy occupies a registry id,
// so the loops below also exercise a Proxy returned from a derived constructor
// well past the first few ids.
class P {
  x: number;
  constructor(x: number) { this.x = x; }
}

class ViaProxy extends P {
  constructor(x: number) {
    super(x);
    return new Proxy(this, {
      get(t: any, k: any, r: any) { return k === "z" ? 42 : Reflect.get(t, k, r); },
    });
  }
}
let sum = 0;
let allInstance = true;
let allProto = true;
for (let i = 0; i < 40; i++) {
  const a: any = new ViaProxy(i);
  sum += a.x + a.z;
  allInstance = allInstance && a instanceof ViaProxy && a instanceof P;
  allProto = allProto && Object.getPrototypeOf(a) === ViaProxy.prototype;
}
console.log("proxy", sum, allInstance, allProto);

// Proxy of a proxy: a two-level chain where each level returns a Proxy.
class Level1 extends P { constructor() { super(1); return new Proxy(this, {}); } }
class Level2 extends Level1 { constructor() { super(); return new Proxy(this, {}); } }
class Level3 extends Level2 { constructor() { super(); return new Proxy(this, {}); } }
let chain = 0;
for (let i = 0; i < 20; i++) chain += (new Level3() as any).x;
console.log("chain", chain, new Level3() instanceof Level1);

class ViaPlain extends P {
  constructor() { super(2); return { plain: true }; }
}
const b: any = new ViaPlain();
console.log("plain", b.plain, b instanceof ViaPlain, b instanceof P,
  Object.getPrototypeOf(b) === Object.prototype, b.x);

// A base constructor returning a primitive: the primitive is ignored.
class BasePrim {
  y = 7;
  constructor() { return 5 as any; }
}
const c0: any = new BasePrim();
console.log("base-primitive", c0.y, c0 instanceof BasePrim);

// A derived constructor returning a primitive throws a TypeError.
class DerivedPrim extends P {
  constructor() { super(3); return 5 as any; }
}
try { new DerivedPrim(); console.log("derived-primitive no throw"); }
catch (e: any) { console.log("derived-primitive", e.constructor.name); }

// A derived constructor returning undefined after super() yields `this`.
class AfterUndef extends P {
  constructor() { super(4); return undefined; }
}
const d: any = new AfterUndef();
console.log("undefined-after-super", d.x, d instanceof AfterUndef);

// Returning undefined without ever calling super() throws.
class NoSuper extends P {
  constructor() { return undefined as any; }
}
try { new NoSuper(); console.log("no-super no throw"); }
catch (e: any) { console.log("no-super", e.constructor.name); }

// Builtin parent, the node-fetch Headers shape.
class Wrapped extends URLSearchParams {
  constructor(init: any) {
    super(init);
    return new Proxy(this, {
      get(t: any, k: any, r: any) {
        const v = Reflect.get(t, k, t);
        return typeof v === "function" ? v.bind(t) : v;
      },
    });
  }
}
const f: any = new Wrapped("a=1&b=2");
console.log("builtin-parent", f.get("b"), f instanceof Wrapped);
