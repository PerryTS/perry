// Static and instance initializers, methods and accessors use their evaluation.
class C { field = "outer"; }
function make(tag: string) {
  class C {
    field = tag;
    static tag = tag;
    method() { return tag; }
    get value() { return this.field + ":" + tag; }
    set value(v: string) { this.field = v; }
    static method() { return this.tag; }
  }
  (C.prototype as any).extra = "extra:" + tag;
  const inside = new C();
  console.log("inside", inside.field, (inside as any).extra);
  return C;
}
const A = make("a");
const B = make("b");
const a = new A();
const b = new B();
a.value = "aa";
b.value = "bb";
console.log("factory", a.field, b.field, a.method(), b.method(), a.value, b.value);
console.log("statics", A.tag, B.tag, A.method(), B.method());
console.log("prototypes", (a as any).extra, (b as any).extra,
  Object.getPrototypeOf(a) === A.prototype, Object.getPrototypeOf(b) === B.prototype);
console.log("identity", A === B, A.prototype === B.prototype,
  a instanceof A, a instanceof B, b instanceof A, b instanceof B);
(A.prototype as any).extra = "only-a";
A.tag = "changed-a";
console.log("mutate", (a as any).extra, (b as any).extra, A.method(), B.method());
console.log("outer", new C().field, (new C() as any).extra);

// The run-once proof retains the inner class's ordinary prototype path.
function once(C: any) {
  {
    class C { field = "once-inner"; }
    return new C().field;
  }
}
console.log("once", once(class { field = "once-outer"; }));
