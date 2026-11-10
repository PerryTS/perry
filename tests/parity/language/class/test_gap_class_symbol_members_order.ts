// Known-symbol class members: key order, names, descriptors (node is the oracle).
const tag = Symbol.for("thr1.tag");
class C {
  foo() { return 1; }
  *[Symbol.iterator]() { yield 1; yield 2; }
  [tag]() { return "tagged"; }
  get [Symbol.toStringTag]() { return "CC"; }
  bar() { return 2; }
}
const syms = Object.getOwnPropertySymbols(C.prototype);
console.log(syms.map((s) => s.toString()).join(","));
console.log(Object.getOwnPropertyNames(C.prototype).join(","));
console.log(Reflect.ownKeys(C.prototype).map(String).join(","));
const d = Object.getOwnPropertyDescriptor(C.prototype, Symbol.iterator)!;
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log((C.prototype as any)[Symbol.iterator].name, (C.prototype as any)[tag].name);
const g = Object.getOwnPropertyDescriptor(C.prototype, Symbol.toStringTag)!;
console.log(typeof g.get, g.set, g.enumerable, g.configurable, g.get!.name);
const c = new C();
console.log(Object.prototype.toString.call(c), (c as any)[tag](), [...c].join("+"));
console.log(Symbol.iterator in c, Object.keys(c).length, c.hasOwnProperty(Symbol.iterator));
let n = 0; for (const k in c) n++; console.log("forin", n);
