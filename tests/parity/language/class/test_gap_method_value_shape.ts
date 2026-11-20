// #12016: a class method read as a value (not called) is an ordinary [[Get]]
// on the receiver: the prototype's data slot, unless an own property shadows
// it, an accessor answers it, or the prototype changed since the last read.

class Base {
  tag = "base";
  m() {
    return "Base.m:" + this.tag;
  }
  n() {
    return "Base.n";
  }
  get g() {
    return () => "getter-made:" + this.tag;
  }
}

class Derived extends Base {
  tag = "derived";
  m() {
    return "Derived.m/" + super.m();
  }
  superValue() {
    // `super.m` read as a value: the home object's prototype slot.
    const f = super.m;
    return f === Base.prototype.m;
  }
  superBound() {
    const f = super.m;
    return f.call(this);
  }
}

const a = new Base();
const b = new Base();
const d = new Derived();

// Identity: one function object per method, the same across reads and
// across instances of the same class.
console.log("identity same read", a.m === a.m);
console.log("identity across instances", a.m === b.m);
console.log("identity is the prototype slot", a.m === Base.prototype.m);
console.log("override differs", d.m !== a.m, d.m === Derived.prototype.m);
console.log("inherited across classes", d.n === a.n);

// Unbound vs bound.
const unbound = a.m;
try {
  console.log("unbound call", (unbound as any)());
} catch (e) {
  console.log("unbound call throws", (e as Error).constructor.name);
}
const bound = a.m.bind(b);
console.log("bound call", bound(), bound !== a.m, a.m.bind(a) !== a.m.bind(a));
console.log("bound name", bound.name, "length", bound.length);
console.log("call/apply", a.m.call(d), a.m.apply(b, []));
console.log("map with method value", [a, d].map((o) => o.n).every((f) => f === Base.prototype.n));

// Getter-defined method: every read runs the getter (fresh function).
const g1 = a.g;
const g2 = a.g;
console.log("getter method", g1(), g1 === g2);

// An own property shadows the prototype method, including non-functions.
const s = new Base();
console.log("before shadow", s.m === Base.prototype.m);
(s as any).m = function () {
  return "own";
};
console.log("own shadow", s.m(), s.m !== Base.prototype.m, a.m === Base.prototype.m);
(s as any).m = undefined;
console.log("own undefined shadow", s.m === undefined);
delete (s as any).m;
console.log("after delete", s.m === Base.prototype.m);
Object.defineProperty(s, "n", { get: () => () => "own accessor", configurable: true });
console.log("own accessor shadow", s.n());

// Self-binding in a constructor (Zod's ZodType pattern), with subclasses.
class Schema {
  constructor() {
    this.parse = this.parse.bind(this);
    this.check = this.check.bind(this);
  }
  parse(x: number) {
    return this.kind() + ":" + x;
  }
  check(x: number) {
    return x > 0;
  }
  kind() {
    return "schema";
  }
}
class NumSchema extends Schema {
  kind() {
    return "num";
  }
  parse(x: number) {
    return "N" + super.parse(x);
  }
}
class StrSchema extends Schema {
  kind() {
    return "str";
  }
}
const schemas = [new Schema(), new NumSchema(), new StrSchema(), new NumSchema()];
for (const sc of schemas) {
  const p = sc.parse;
  console.log("self-bound", p(1), Object.prototype.hasOwnProperty.call(sc, "parse"), sc.check(2));
}

// Prototype mutation after a read site has seen the method.
function readM(o: Base) {
  return o.m;
}
const first = readM(a);
const replaced = function (this: Base) {
  return "replaced:" + this.tag;
};
Base.prototype.m = replaced;
console.log("proto replace seen", readM(a) === replaced, readM(a) !== first, a.m());
console.log("derived keeps override", d.m === Derived.prototype.m);
delete (Derived.prototype as any).m;
console.log("derived falls through after delete", d.m === replaced, d.m());
Object.setPrototypeOf(b, { m() { return "new proto"; } });
console.log("setPrototypeOf seen", b.m(), readM(b) !== replaced);
Object.defineProperty(Base.prototype, "n", { get: () => () => "proto accessor", configurable: true });
console.log("proto accessor seen", a.n(), d.n());

// Super method values.
console.log("super value", d.superValue(), d.superBound());

// typeof / name / length of a method value.
console.log("typeof", typeof a.n, typeof Derived.prototype.superValue, Derived.prototype.superValue.name, Schema.prototype.parse.length);
