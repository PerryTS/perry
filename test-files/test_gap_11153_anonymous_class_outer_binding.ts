// Anonymous class names are inferred for display, not an inner self-binding.
function make(v: number) {
  const C = class {
    static tag = v;
    who() { return C.tag; }
    same() { return this.constructor === C; }
    static init() { (C.prototype as any).z = v; }
    set() { (C.prototype as any).q = v; }
  };
  C.init();
  return C;
}
const factories: any[] = [make];
const A = factories[0](1), B = factories[0](2);
const a = new A(), b = new B();
a.set(); b.set();
console.log(a.who(), b.who(), a.same(), b.same());
console.log(Object.keys(A.prototype), a.z, b.z, a.q, b.q);

function mutable() {
  let Current: any = class { self() { return Current; } };
  const Original = Current;
  const instance = new Original();
  console.log(instance.self() === Original);
  Current = { tag: 'replacement' };
  console.log(instance.self() === Current, instance.self().tag);
}
mutable();

function named() {
  let Outer: any = class Inner { self() { return Inner; } };
  const Original = Outer;
  Outer = { tag: 'replacement' };
  console.log(new Original().self() === Original);
}
named();
