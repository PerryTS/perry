// #9503: a runtime-valued heritage that resolves to Object must bind the
// object returned by the builtin super-constructor as the derived `this`, and
// `new` must publish that same object after the explicit constructor returns.
function mkc(P?: any): any {
  class D extends (P ?? Object) {
    marker = "field";

    constructor(def: any) {
      super(def);
      (this as any).def = def;
    }

    ping(): string {
      return "pong";
    }
  }
  return D;
}

const A = mkc();
const a = new A({ t: 1 });
console.log("default:", a.def.t, a.marker, a.ping(), a instanceof A);

// Keep the value path dynamic even when Object is supplied explicitly.
const B = mkc(Object);
const b = new B({ t: 2 });
console.log("explicit:", b.def.t, b.marker, b.ping(), b instanceof B);

const a2 = new A({ t: 3 });
console.log("distinct:", a !== a2, a2.def.t);

// A capture forces a fresh class object rather than a shared ClassRef. Its
// evaluation-specific prototype must survive the same builtin-super path.
function mkFresh(P: any, label: string): any {
  const captured = label;
  return class extends P {
    #label = captured;

    constructor(def: any) {
      super(def);
      (this as any).def = def;
    }

    read(): string {
      return `${this.#label}:${(this as any).def.t}`;
    }
  };
}

const Fresh = mkFresh(Object, "fresh");
const fresh = new Fresh({ t: 4 });
console.log(
  "fresh:",
  fresh.read(),
  fresh instanceof Fresh,
  Object.getPrototypeOf(fresh) === Fresh.prototype,
);
