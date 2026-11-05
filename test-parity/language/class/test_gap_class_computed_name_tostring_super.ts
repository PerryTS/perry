// THR1: a computed method keeps its .name, toString and super.
class Base {
  hello() { return "base-hello"; }
  *[Symbol.iterator]() { yield "base"; }
}
const key = "dyn";
class Derived extends Base {
  [Symbol.iterator]() { const it = super[Symbol.iterator](); return it; }
  ["lit"]() { return super.hello() + "+lit"; }
  [key]() { return "k:" + super.hello(); }
  get ["acc"]() { return "acc"; }
  set ["acc"](_v) {}
  static [Symbol.hasInstance](x: any) { return x === 42; }
}
const p = Derived.prototype as any;
console.log(p[Symbol.iterator].name, p.lit.name, p.dyn.name);
const ad = Object.getOwnPropertyDescriptor(p, "acc")!;
console.log(ad.get!.name, ad.set!.name);
console.log(p.lit.toString());
console.log(Base.prototype[Symbol.iterator].toString());
const d = new Derived() as any;
console.log([...d].join(), d.lit(), d.dyn(), d.acc, 42 instanceof (Derived as any));
