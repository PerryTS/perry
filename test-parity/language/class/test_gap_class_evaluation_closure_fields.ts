function factory(prefix: string) {
  return (suffix: string) => {
    const tag = prefix + suffix;
    const C = class {
      field = tag;
      static tag = tag;
      method() { return this.field; }
      get value() { return this.field + ":get"; }
      set value(v: string) { this.field = v; }
      static method() { return this.tag; }
    };
    (C.prototype as any).extra = tag;
    const inside = new C();
    console.log("inside", inside.field, (inside as any).extra);
    return C;
  };
}
const make = factory("p:");
const A = make("a");
const B = make("b");
const a = new A();
const b = new B();
console.log("closure", a.field, b.field, a.method(), b.method(), a.value, b.value,
  A.tag, B.tag, A.method(), B.method(), (a as any).extra, (b as any).extra);
a.value = "changed";
console.log("setter", a.field, b.field, a.value, b.value);
console.log("identity", A === B, A.prototype === B.prototype,
  Object.getPrototypeOf(a) === A.prototype, Object.getPrototypeOf(b) === B.prototype,
  a instanceof A, a instanceof B, b instanceof A, b instanceof B);

function inherited(tag: string) {
  const baseCap = tag + ":base";
  const subCap = tag + ":sub";
  const Base = class { field = baseCap; m() { return baseCap; } };
  const Sub = class extends Base { child = subCap; n() { return subCap; } };
  (Base.prototype as any).extra = tag;
  const sub = new Sub();
  console.log("subclass", sub.field, sub.child, sub.m(), sub.n(), (sub as any).extra,
    sub instanceof Base, Object.getPrototypeOf(sub) === Sub.prototype);
  const Empty = class extends Base {};
  const empty = new Empty();
  console.log("base-only", empty.field, empty.m(), (empty as any).extra,
    empty instanceof Base, Object.getPrototypeOf(empty) === Empty.prototype);
  return Sub;
}
const S1 = inherited("x");
const S2 = inherited("y");
const s1 = new S1();
const s2 = new S2();
console.log("inherited-identity", s1.m(), s2.m(), s1.n(), s2.n(),
  s1 instanceof S1, s1 instanceof S2, s2 instanceof S1, s2 instanceof S2);
