// An instance of a class EXPRESSION inherits from that evaluation's
// prototype, not the class declaration's. Family layouts (keyless and keyed
// classes) claim the class's own prototype as their identity, so an instance
// linked to an evaluation prototype must leave the family layout: otherwise a
// shape-proved reader (the symbol-member lookup a method call takes) resolved
// the declaration's copy of the method, and a generator created through
// `c[Symbol.iterator]()` linked to the wrong `.prototype`.

function keyless() {
  return class {
    *[Symbol.iterator]() {
      yield 1;
    }
  };
}
function keyed() {
  return class {
    x = 1;
    *[Symbol.iterator]() {
      yield 2;
    }
  };
}
function presuper(n: number) {
  class Base {}
  return class extends Base {
    constructor() {
      if (n < 0) throw new Error("neg");
      super();
    }
    *[Symbol.iterator]() {
      yield n;
    }
  };
}

for (const [name, make] of [
  ["keyless", keyless],
  ["keyed", keyed],
  ["presuper", () => presuper(3)],
] as const) {
  const A: any = make();
  const B: any = make();
  const a: any = new A();
  const b: any = new B();
  const m = a[Symbol.iterator];
  const it = a[Symbol.iterator]();
  console.log(
    name,
    Object.getPrototypeOf(a) === A.prototype,
    m === A.prototype[Symbol.iterator],
    Object.getPrototypeOf(it) === m.prototype,
    Object.getPrototypeOf(b[Symbol.iterator]()) === b[Symbol.iterator].prototype,
    A.prototype[Symbol.iterator] !== B.prototype[Symbol.iterator],
    [...a].join(","),
  );
  const k: any = Symbol.iterator;
  console.log(name, "var-key", Object.getPrototypeOf(a[k]()) === m.prototype);
}

class Decl {
  *[Symbol.iterator]() {
    yield 9;
  }
}
const d: any = new Decl();
console.log("decl", Object.getPrototypeOf(d[Symbol.iterator]()) === d[Symbol.iterator].prototype, [...d].join(","));
